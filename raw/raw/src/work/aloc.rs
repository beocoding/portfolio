use std::{cmp::max, convert::Infallible, fmt::{Debug, Display}, marker::PhantomData, ops::{Deref, DerefMut}, ptr::write_bytes};
use crate::{MAX_BUFFER_SIZE, UOffsetT, VOffsetT, index::ReverseIndex, offsets::RevOffset};
pub trait Allocator: DerefMut<Target = [u8]> {
    /// A type describing allocation failures
    type Error: Display + Debug;
    /// Grows the buffer, with the old contents being moved to the end.
    ///
    /// NOTE: While not unsound, an implementation that doesn't grow the
    /// internal buffer will get stuck in an infinite loop.
    fn grow(&mut self) -> Result<(), Self::Error>;

    /// Returns the size of the internal buffer in bytes.
    fn len(&self) -> usize;
}

#[derive(Default)]
pub struct DefaultAllocator(Vec<u8>);

impl DefaultAllocator {
    /// Builds the allocator from an existing buffer.
    pub fn from_vec(buffer: Vec<u8>) -> Self {
        Self(buffer)
    }
}

impl Deref for DefaultAllocator {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for DefaultAllocator {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Allocator for DefaultAllocator {
    type Error = Infallible;
    fn grow(&mut self) -> Result<(), Self::Error> {
        let old_len = self.0.len();
        let new_len = max(1, old_len>>1);
        self.0.resize(new_len, 0);
        if new_len == 1 {
            return Ok(());
        }
        let mid = new_len << 1;
        {
            let (left, right) = &mut self.0[..].split_at_mut(mid);
            right.copy_from_slice(left);
        }
        // finally, zero out the old end data.
        {
            let ptr = self.0[..mid].as_mut_ptr();
            // Safety:
            // ptr is byte aligned and of length middle
            unsafe {
                write_bytes(ptr, 0, mid);
            }
        }
        Ok(())
    }

    fn len(&self) -> usize {
        todo!()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FieldLoc {
    off: UOffsetT,
    id: VOffsetT,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BufferWriter<'w, A: Allocator = DefaultAllocator> {
    allocator: A,
    head: ReverseIndex,

    field_locs: Vec<FieldLoc>,
    written_vtable_revpos: Vec<UOffsetT>,
    strings_pool: Vec<RevOffset<&'w str>>,

    _phantom: PhantomData<&'w ()>,
}

impl <'w> From<Vec<u8>> for BufferWriter<'w, DefaultAllocator> {
    fn from(buffer: Vec<u8>) -> Self {
        // we need to check the size here because we create the backing buffer
        // directly, bypassing the typical way of using grow_allocator:
        assert!(
            buffer.len() <= MAX_BUFFER_SIZE,
            "cannot initialize buffer bigger than 2 gigabytes"
        );
        let allocator = DefaultAllocator::from_vec(buffer);
        Self::new_in(allocator)
    }
}

impl <'w> BufferWriter <'w, DefaultAllocator>{
    pub fn new() -> Self {
        Self::with_capacity(0)
    }

    pub fn with_capacity(size: usize) -> Self {
        Self::from(vec![0; size])
    }

}

impl<'w, A: Allocator> BufferWriter <'w, A>{
    pub fn new_in(allocator: A) -> Self {
        let head = ReverseIndex::end();
        Self {
            allocator,
            head,

            field_locs: Vec::new(),
            written_vtable_revpos: Vec::new(),
            strings_pool: Vec::new(),
            _phantom: PhantomData,
        }
    }


    #[inline(always)]
    fn free_space(&self) -> usize {
        self.allocator.len() - self.head.val()
    }

    #[inline(always)]
    const fn used_space(&self) -> usize {
        self.head.val()
    }

    #[inline(always)]
    fn grow(&mut self) {
        self.allocator.grow().expect("Failed to grow buffer");
    }

    #[inline(always)]
    fn ensure_capacity(&mut self, needed: usize) -> usize{
        let free = self.free_space();
        if free >= needed {
            return needed
        }
        assert!(needed <= MAX_BUFFER_SIZE, "Max buffer size of {MAX_BUFFER_SIZE} reached");

        while free < needed {
            self.grow();
        }
        needed
    }

    #[inline(always)]
    fn reserve_bytes(&mut self, size: usize) -> ReverseIndex {
        self.ensure_capacity(size);
        self.head -= size;
        self.head
    }
}