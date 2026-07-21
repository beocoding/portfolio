use std::ops::{Add, AddAssign, Index, IndexMut, Sub, SubAssign};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReverseIndex(usize);

impl ReverseIndex {
    /// Returns an index set to the end.
    ///
    /// Note: Indexing this will result in an out of bounds error.
    #[inline(always)]
    pub const fn end() -> Self {
        Self(0)
    }

    /// Returns a struct equivalent to the range `self..`
    #[inline(always)]
    pub const fn range_to_end(self) -> ReverseIndexRange {
        ReverseIndexRange(self, ReverseIndex::end())
    }

    /// Returns a struct equivalent to the range `self..end`
    #[inline(always)]
    pub const fn range_to(self, end: ReverseIndex) -> ReverseIndexRange {
        ReverseIndexRange(self, end)
    }

    /// Transforms this reverse index into a regular index for the given buffer.
    #[inline(always)]
    pub const fn invert<T>(self, buf: &[T]) -> usize {
        buf.len() - self.0
    }

    /// Returns the number of elements until the end of the range.
    #[inline(always)]
    pub const fn val(&self) -> usize {
        self.0
    }
}


impl Sub<usize> for ReverseIndex {
    type Output = Self;

    fn sub(self, rhs: usize) -> Self::Output {
        Self(self.0 + rhs)
    }
}

impl SubAssign<usize> for ReverseIndex {
    fn sub_assign(&mut self, rhs: usize) {
        *self = *self - rhs;
    }
}

impl Add<usize> for ReverseIndex {
    type Output = Self;

    fn add(self, rhs: usize) -> Self::Output {
        Self(self.0 - rhs)
    }
}

impl AddAssign<usize> for ReverseIndex {
    fn add_assign(&mut self, rhs: usize) {
        *self = *self + rhs;
    }
}

impl<T> Index<ReverseIndex> for [T] {
    type Output = T;

    fn index(&self, index: ReverseIndex) -> &Self::Output {
        let index = index.invert(self);
        &self[index]
    }
}

impl<T> IndexMut<ReverseIndex> for [T] {
    fn index_mut(&mut self, index: ReverseIndex) -> &mut Self::Output {
        let index = index.invert(self);
        &mut self[index]
    }
}


#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReverseIndexRange(ReverseIndex, ReverseIndex);

impl<T> Index<ReverseIndexRange> for [T] {
    type Output = [T];

    fn index(&self, index: ReverseIndexRange) -> &Self::Output {
        let start = index.0.invert(self);
        let end = index.1.invert(self);
        &self[start..end]
    }
}

impl<T> IndexMut<ReverseIndexRange> for [T] {
    fn index_mut(&mut self, index: ReverseIndexRange) -> &mut Self::Output {
        let start = index.0.invert(self);
        let end = index.1.invert(self);
        &mut self[start..end]
    }
}