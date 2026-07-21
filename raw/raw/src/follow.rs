use core::marker::PhantomData;

/// Follow is a trait that allows us to access FlatBuffers in a declarative,
/// type safe, and fast way. They compile down to almost no code (after
/// optimizations). Conceptually, Follow lifts the offset-based access
/// patterns of FlatBuffers data into the type system. This trait is used
/// pervasively at read time, to access tables, vtables, vectors, strings, and
/// all other data. At this time, Follow is not utilized much on the write
/// path.
///
/// Writing a new Follow implementation primarily involves deciding whether
/// you want to return data (of the type Self::Inner) or do you want to
/// continue traversing the FlatBuffer.
pub trait Follow<'b> {
    type Inner;
    /// # Safety
    ///
    /// `buf[loc..]` must contain a valid value of `Self` and anything it
    /// transitively refers to by offset must also be valid
    fn follow(buf: &'b [u8], loc: usize) -> Self::Inner;
}

/// FollowStart wraps a Follow impl in a struct type. This can make certain
/// programming patterns more ergonomic.
#[derive(Debug, Default)]
pub struct FollowStart<T>(PhantomData<T>);
impl<'a, T: Follow<'a> + 'a> FollowStart<T> {
    #[inline]
    pub fn new() -> Self {
        Self(PhantomData)
    }

    /// # Safety
    ///
    /// `buf[loc..]` must contain a valid value of `T`
    #[inline]
    pub fn resolve(&'a self, buf: &'a [u8], loc: usize) -> T::Inner {
        T::follow(buf, loc)
    }
}
impl<'a, T: Follow<'a>> Follow<'a> for FollowStart<T> {
    type Inner = T::Inner;
    #[inline]
    fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
        T::follow(buf, loc)
    }
}