use core::mem::size_of;

use crate::follow::Follow;
use crate::scalars::read_scalar_at;

pub const MAX_BUFFER_SIZE: usize = (1u64 << 31) as usize;

pub const VTABLE_METADATA_FIELDS: usize = 2;

pub const SIZE_U8: usize = size_of::<u8>();
pub const SIZE_I8: usize = size_of::<i8>();

pub const SIZE_U16: usize = size_of::<u16>();
pub const SIZE_I16: usize = size_of::<i16>();

pub const SIZE_U32: usize = size_of::<u32>();
pub const SIZE_I32: usize = size_of::<i32>();

pub const SIZE_U64: usize = size_of::<u64>();
pub const SIZE_I64: usize = size_of::<i64>();

pub const SIZE_F32: usize = size_of::<f32>();
pub const SIZE_F64: usize = size_of::<f64>();

pub const SIZE_SOFFSET: usize = SIZE_I32;
pub const SIZE_UOFFSET: usize = SIZE_U32;
pub const SIZE_VOFFSET: usize = SIZE_I16;

pub const SIZE_PREFIX_HEADER: usize = SIZE_UOFFSET;

/// SOffsetT is a relative pointer from tables to their vtables.
pub type SOffsetT = i32;

/// UOffsetT is used represent both for relative pointers and lengths of vectors.
pub type UOffsetT = u32;

/// VOffsetT is a relative pointer in vtables to point from tables to field data.
pub type VOffsetT = u16;

/// Follow trait impls for primitive types.
///
/// Ideally, these would be implemented as a single impl using trait bounds on
/// EndianScalar, but implementing Follow that way causes a conflict with
/// other impls.
macro_rules! impl_follow_for_endian_scalar {
    ($ty:ident) => {
        impl<'a> Follow<'a> for $ty {
            type Inner = $ty;
            #[inline(always)]
             fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
                read_scalar_at::<$ty>(buf, loc)
            }
        }
    };
}

impl_follow_for_endian_scalar!(u8);
impl_follow_for_endian_scalar!(u16);
impl_follow_for_endian_scalar!(u32);
impl_follow_for_endian_scalar!(u64);
impl_follow_for_endian_scalar!(i8);
impl_follow_for_endian_scalar!(i16);
impl_follow_for_endian_scalar!(i32);
impl_follow_for_endian_scalar!(i64);
impl_follow_for_endian_scalar!(f32);
impl_follow_for_endian_scalar!(f64);