//! # fluffr/fluffr_core/src/reverse_index.rs
//!
//! `ReverseIndex` gives a distinct type to values expressed as a distance
//! from the *end* of a buffer — what the rest of the crate calls a "slot".
//! `Buffer::slot()`, the value returned by `Serialize::write_to`, and the
//! `table_slot`/`slot` parameters accepted by [`crate::buffer::Buffer`] are
//! all this quantity. Representing it as a bare `usize` alongside absolute
//! buffer positions (also a bare `usize`) makes the two easy to mix up: they
//! are related by `absolute = len - distance`, the reverse of ordinary
//! pointer arithmetic.

use std::ops::{Add, AddAssign, Sub, SubAssign};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReverseIndex(usize);

impl ReverseIndex {
    /// A reverse index pointing at the end of the buffer (distance 0).
    #[inline(always)]
    pub const fn end() -> Self {
        Self(0)
    }

    /// Wraps a value already known to be a distance from the end — e.g. a
    /// slot returned by `Buffer::slot` or `Serialize::write_to`.
    #[inline(always)]
    pub(crate) const fn from_slot(slot: usize) -> Self {
        Self(slot)
    }

    /// Wraps an absolute buffer position, converting it to a distance from
    /// the end of a buffer of the given length.
    #[inline(always)]
    pub(crate) const fn from_absolute(pos: usize, len: usize) -> Self {
        Self(len - pos)
    }

    /// Converts this reverse index back into an absolute position for a
    /// buffer of the given length.
    #[inline(always)]
    pub const fn invert(self, len: usize) -> usize {
        len - self.0
    }

    /// The raw distance from the end.
    #[inline(always)]
    pub const fn val(&self) -> usize {
        self.0
    }
}

impl Sub<usize> for ReverseIndex {
    type Output = Self;
    #[inline(always)]
    fn sub(self, rhs: usize) -> Self::Output {
        Self(self.0 + rhs)
    }
}

impl SubAssign<usize> for ReverseIndex {
    #[inline(always)]
    fn sub_assign(&mut self, rhs: usize) {
        *self = *self - rhs;
    }
}

impl Add<usize> for ReverseIndex {
    type Output = Self;
    #[inline(always)]
    fn add(self, rhs: usize) -> Self::Output {
        Self(self.0 - rhs)
    }
}

impl AddAssign<usize> for ReverseIndex {
    #[inline(always)]
    fn add_assign(&mut self, rhs: usize) {
        *self = *self + rhs;
    }
}
