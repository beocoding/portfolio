pub mod buffers;
pub mod reflect;
pub mod offsets;
pub mod follow;
pub mod push;

pub mod aloc;
pub mod index;
pub mod scalars;
pub mod primitives;
use core::mem::size_of;

pub mod work;

mod private {
    /// Types that are trivially transmutable are those where any combination of bits
    /// represents a valid value of that type
    ///
    /// For example integral types are TriviallyTransmutable as all bit patterns are valid,
    /// however, `bool` is not trivially transmutable as only `0` and `1` are valid
    pub trait TriviallyTransmutable {}

    impl TriviallyTransmutable for i8 {}
    impl TriviallyTransmutable for i16 {}
    impl TriviallyTransmutable for i32 {}
    impl TriviallyTransmutable for i64 {}
    impl TriviallyTransmutable for u8 {}
    impl TriviallyTransmutable for u16 {}
    impl TriviallyTransmutable for u32 {}
    impl TriviallyTransmutable for u64 {}
}

/// Trait for values that must be stored in little-endian byte order, but
/// might be represented in memory as big-endian. Every type that implements
/// LeScalar is a valid FlatBuffers scalar value.
///
/// The Rust stdlib does not provide a trait to represent scalars, so this trait
/// serves that purpose, too.
///
/// Note that we do not use the num-traits crate for this, because it provides
/// "too much". For example, num-traits provides i128 support, but that is an
/// invalid FlatBuffers type.
pub trait LeScalar: Sized + PartialEq + Copy + Clone {
    type Scalar: private::TriviallyTransmutable;

    fn to_wire(self) -> Self::Scalar;

    fn from_wire(v: Self::Scalar) -> Self;
}

/// Macro for implementing an endian conversion using the stdlib `to_le` and
/// `from_le` functions. This is used for integer types. It is not used for
/// floats, because the `to_le` and `from_le` are not implemented for them in
/// the stdlib.
macro_rules! impl_endian_scalar {
    ($ty:ident) => {
        impl LeScalar for $ty {
            type Scalar = Self;

            #[inline]
            fn to_wire(self) -> Self::Scalar {
                Self::to_le(self)
            }
            #[inline]
            fn from_wire(v: Self::Scalar) -> Self {
                Self::from_le(v)
            }
        }
    };
}

impl_endian_scalar!(u8);
impl_endian_scalar!(i8);
impl_endian_scalar!(u16);
impl_endian_scalar!(u32);
impl_endian_scalar!(u64);
impl_endian_scalar!(i16);
impl_endian_scalar!(i32);
impl_endian_scalar!(i64);

impl LeScalar for bool {
    type Scalar = u8;

    fn to_wire(self) -> Self::Scalar {
        self as u8
    }

    fn from_wire(v: Self::Scalar) -> Self {
        v != 0
    }
}

impl LeScalar for f32 {
    type Scalar = u32;
    /// Convert f32 from host endian-ness to little-endian.
    #[inline]
    fn to_wire(self) -> u32 {
        // Floats and Ints have the same endianness on all supported platforms.
        // <https://doc.rust-lang.org/std/primitive.f32.html#method.from_bits>
        self.to_bits().to_le()
    }
    /// Convert f32 from little-endian to host endian-ness.
    #[inline]
    fn from_wire(v: u32) -> Self {
        // Floats and Ints have the same endianness on all supported platforms.
        // <https://doc.rust-lang.org/std/primitive.f32.html#method.from_bits>
        f32::from_bits(u32::from_le(v))
    }
}

impl LeScalar for f64 {
    type Scalar = u64;

    /// Convert f64 from host endian-ness to little-endian.
    #[inline]
    fn to_wire(self) -> u64 {
        // Floats and Ints have the same endianness on all supported platforms.
        // <https://doc.rust-lang.org/std/primitive.f64.html#method.from_bits>
        self.to_bits().to_le()
    }
    /// Convert f64 from little-endian to host endian-ness.
    #[inline]
    fn from_wire(v: u64) -> Self {
        // Floats and Ints have the same endianness on all supported platforms.
        // <https://doc.rust-lang.org/std/primitive.f64.html#method.from_bits>
        f64::from_bits(u64::from_le(v))
    }
}

/// Place an LeScalar into the provided mutable byte slice. Performs
/// endian conversion, if necessary.
/// # Safety
/// Caller must ensure `s.len() >= size_of::<T>()`
#[inline]
pub fn wire_to_buf<T: LeScalar>(s: &mut [u8], x: T) {
    let size = size_of::<T::Scalar>();
    debug_assert!(
        s.len() >= size,
        "insufficient capacity for emplace_scalar, needed {} got {}",
        size,
        s.len()
    );

    let x_le = x.to_wire();
    unsafe{core::ptr::copy_nonoverlapping(
        &x_le as *const T::Scalar as *const u8,
        s.as_mut_ptr() as *mut u8,
        size,
    )};
}

/// Read an LeScalar from the provided byte slice at the specified location.
/// Performs endian conversion, if necessary.
/// # Safety
/// Caller must ensure `s.len() >= loc + size_of::<T>()`.
#[inline]
pub fn read_scalar_at<T: LeScalar>(s: &[u8], loc: usize) -> T {
    read_scalar(&s[loc..])
}

/// Read an LeScalar from the provided byte slice. Performs endian
/// conversion, if necessary.
/// # Safety
/// Caller must ensure `s.len() > size_of::<T>()`.
#[inline]
pub fn read_scalar<T: LeScalar>(s: &[u8]) -> T {
    let size = size_of::<T::Scalar>();
    debug_assert!(
        s.len() >= size,
        "insufficient capacity for emplace_scalar, needed {} got {}",
        size,
        s.len()
    );

    let mut mem = core::mem::MaybeUninit::<T::Scalar>::uninit();
    // Since [u8] has alignment 1, we copy it into T which may have higher alignment.
    unsafe { 
        core::ptr::copy_nonoverlapping(s.as_ptr(), mem.as_mut_ptr() as *mut u8, size);
        T::from_wire(mem.assume_init())
    }
}
pub const MAX_BUFFER_SIZE: usize = (1u64 << 31) as usize;

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

pub const SIZE_LHEAD: usize = SIZE_UOFFSET;

/// SOffsetT is a relative pointer from tables to their vtables.
pub type SOffsetT = i32;

/// UOffsetT is used represent both for relative pointers and lengths of vectors.
pub type UOffsetT = u32;

/// VOffsetT is a relative pointer in vtables to point from tables to field data.
pub type VOffsetT = u16;

