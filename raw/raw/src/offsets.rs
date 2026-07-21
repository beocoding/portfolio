use std::{marker::PhantomData, ops::Deref};

use crate::{SIZE_SOFFSET, SIZE_UOFFSET, SIZE_VOFFSET, SOffsetT, UOffsetT, VOffsetT, follow::Follow, primitives::SIZE_PREFIX_HEADER, push::Push, scalars::{read_scalar, read_scalar_at, wire_to_buf}};


/// WIPOffset contains an UOffsetT with a special meaning: it is the location of
/// data relative to the *end* of an in-progress FlatBuffer. The
/// FlatBufferBuilder uses this to track the location of objects in an absolute
/// way. The impl of Push converts a WIPOffset into a ForwardsUOffset.
#[derive(Debug)]
pub struct RevOffset<T>(UOffsetT, PhantomData<T>);

// We cannot use derive for these two impls, as the derived impls would only
// implement `Copy` and `Clone` for `T: Copy` and `T: Clone` respectively.
// However `WIPOffset<T>` can always be copied, no matter that `T` you
// have.
impl<T> Copy for RevOffset<T> {}
impl<T> Clone for RevOffset<T> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Eq for RevOffset<T> {}

impl<T> PartialEq for RevOffset<T> {
    fn eq(&self, o: &RevOffset<T>) -> bool {
        self.value() == o.value()
    }
}

impl<T> Deref for RevOffset<T> {
    type Target = UOffsetT;
    #[inline]
    fn deref(&self) -> &UOffsetT {
        &self.0
    }
}
impl<'a, T: 'a> RevOffset<T> {
    /// Create a new WIPOffset.
    #[inline]
    pub fn new(o: UOffsetT) -> RevOffset<T> {
        RevOffset(o, PhantomData)
    }

    /// Get the underlying value.
    #[inline(always)]
    pub fn value(self) -> UOffsetT {
        self.0
    }
}

impl<T> Push for RevOffset<T> {
    type Output = UOffset<T>;

    #[inline(always)]
     fn push(&self, dst: &mut [u8], written_len: usize) {
        let n = (SIZE_UOFFSET + written_len - self.value() as usize) as UOffsetT;
        wire_to_buf::<UOffsetT>(dst, n);
    }
}

impl<T> Push for UOffset<T> {
    type Output = Self;
    #[inline(always)]
     fn push(&self, dst: &mut [u8], written_len: usize) {
        self.value().push(dst, written_len);
    }
}

/// ForwardsUOffset is used by Follow to traverse a FlatBuffer: the pointer
/// is incremented by the value contained in this type.
#[derive(Debug)]
pub struct UOffset<T>(UOffsetT, PhantomData<T>);

// We cannot use derive for these two impls, as the derived impls would only
// implement `Copy` and `Clone` for `T: Copy` and `T: Clone` respectively.
// However `ForwardsUOffset<T>` can always be copied, no matter that `T` you
// have.
impl<T> Copy for UOffset<T> {}
impl<T> Clone for UOffset<T> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> UOffset<T> {
    #[inline(always)]
    pub fn value(self) -> UOffsetT {
        self.0
    }
}

impl<'a, T: Follow<'a>> Follow<'a> for UOffset<T> {
    type Inner = T::Inner;
    #[inline(always)]
     fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
        let slice = &buf[loc..loc + SIZE_UOFFSET];
        let off = read_scalar::<UOffsetT>(slice) as usize;
        T::follow(buf, loc + off)
    }
}

/// ForwardsVOffset is used by Follow to traverse a FlatBuffer: the pointer
/// is incremented by the value contained in this type.
#[derive(Debug)]
pub struct VtableOffset<T>(VOffsetT, PhantomData<T>);
impl<T> VtableOffset<T> {
    #[inline(always)]
    pub fn value(&self) -> VOffsetT {
        self.0
    }
}

impl<'a, T: Follow<'a>> Follow<'a> for VtableOffset<T> {
    type Inner = T::Inner;
    #[inline(always)]
     fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
        let slice = &buf[loc..loc + SIZE_VOFFSET];
        let off = read_scalar::<VOffsetT>(slice) as usize;
        T::follow(buf, loc + off)
    }
}

impl<T> Push for VtableOffset<T> {
    type Output = Self;
    #[inline]
     fn push(&self, dst: &mut [u8], written_len: usize) {
        self.value().push(dst, written_len);
    }
}

/// SOffset is used by Follow to traverse a buffer: the pointer
/// is incremented by the *negative* of the value contained in this type.
#[derive(Debug)]
pub struct SOffset<T>(SOffsetT, PhantomData<T>);
impl<T> SOffset<T> {
    #[inline(always)]
    pub fn value(&self) -> SOffsetT {
        self.0
    }
}

impl<'a, T: Follow<'a>> Follow<'a> for SOffset<T> {
    type Inner = T::Inner;
    #[inline(always)]
     fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
        let slice = &buf[loc..loc + SIZE_SOFFSET];
        let off = read_scalar::<SOffsetT>(slice);
        T::follow(buf, (loc as SOffsetT - off) as usize)
    }
}

impl<T> Push for SOffset<T> {
    type Output = Self;

    #[inline]
     fn push(&self, dst: &mut [u8], written_len: usize) {
        self.value().push(dst, written_len);
    }
}

/// SkipSizePrefix is used by Follow to traverse a FlatBuffer: the pointer is
/// incremented by a fixed constant in order to skip over the size prefix value.
pub struct SkipPrefixHeader<T>(PhantomData<T>);
impl<'a, T: Follow<'a> + 'a> Follow<'a> for SkipPrefixHeader<T> {
    type Inner = T::Inner;
    #[inline(always)]
     fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
        T::follow(buf, loc + SIZE_PREFIX_HEADER)
    }
}

/// SkipRootOffset is used by Follow to traverse a FlatBuffer: the pointer is
/// incremented by a fixed constant in order to skip over the root offset value.
pub struct SkipRootOffset<T>(PhantomData<T>);
impl<'a, T: Follow<'a> + 'a> Follow<'a> for SkipRootOffset<T> {
    type Inner = T::Inner;
    #[inline(always)]
     fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
        T::follow(buf, loc + SIZE_UOFFSET)
    }
}

impl<'a> Follow<'a> for bool {
    type Inner = bool;
    #[inline(always)]
     fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
        read_scalar_at::<u8>(buf, loc) != 0
    }
}
