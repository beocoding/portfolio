use core::cmp::max;
use core::mem::{align_of, size_of};

use crate::scalars::wire_to_buf;

/// Trait to abstract over functionality needed to write values (either owned
/// or referenced). Used in FlatBufferBuilder and implemented for generated
/// types.
pub trait Push: Sized {
    type Output;

    /// # Safety
    ///
    /// dst is aligned to [`Self::alignment`] and has length greater than or equal to [`Self::size`]
    fn push(&self, dst: &mut [u8], written_len: usize);
    #[inline]
    fn size() -> usize {
        size_of::<Self::Output>()
    }
    #[inline]
    fn alignment() -> PushAlignment {
        PushAlignment::new(align_of::<Self::Output>())
    }
}

impl<'a, T: Push> Push for &'a T {
    type Output = T::Output;

    fn push(&self, dst: &mut [u8], written_len: usize) {
        T::push(self, dst, written_len)
    }

    fn size() -> usize {
        T::size()
    }

    fn alignment() -> PushAlignment {
        T::alignment()
    }
}

/// Ensure Push alignment calculations are typesafe (because this helps reduce
/// implementation issues when using FlatBufferBuilder::align).
pub struct PushAlignment(usize);
impl PushAlignment {
    #[inline]
    pub fn new(x: usize) -> Self {
        PushAlignment(x)
    }
    #[inline]
    pub fn value(&self) -> usize {
        self.0
    }
    #[inline]
    pub fn max_of(&self, o: usize) -> Self {
        PushAlignment::new(max(self.0, o))
    }
}

/// Macro to implement Push for EndianScalar types.
macro_rules! impl_push_for_endian_scalar {
    ($ty:ident) => {
        impl Push for $ty {
            type Output = $ty;

            #[inline]
            fn push(&self, dst: &mut [u8], _written_len: usize) {
                wire_to_buf::<$ty>(dst, *self);
            }
        }
    };
}

impl_push_for_endian_scalar!(bool);
impl_push_for_endian_scalar!(u8);
impl_push_for_endian_scalar!(i8);
impl_push_for_endian_scalar!(u16);
impl_push_for_endian_scalar!(i16);
impl_push_for_endian_scalar!(u32);
impl_push_for_endian_scalar!(i32);
impl_push_for_endian_scalar!(u64);
impl_push_for_endian_scalar!(i64);
impl_push_for_endian_scalar!(f32);
impl_push_for_endian_scalar!(f64);