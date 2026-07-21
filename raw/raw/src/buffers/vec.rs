pub use crate::buffers::error::Error;
pub type Result<T> = core::result::Result<T, Error>;

#[derive(Clone, Copy)]
pub struct TailOffset(pub u32);

impl TailOffset {
    pub fn new(total_buffer_size: usize, current_head: usize) -> Self {
        // Handles stay compact (u32); offsets from the tail are small in practice.
        Self((total_buffer_size - current_head) as u32)
    }

    #[inline(always)]
    pub const fn offset(&self) -> usize {
        self.0 as usize
    }

    #[inline(always)]
    pub const fn get_access(&self, total_buffer_size: usize) -> usize {
        total_buffer_size - self.0 as usize
    }
}

// Pre-allocated Vec as a buffer. This is constructed backwards, so the head approaches 0 as more data is added.
// Consequently, the head is also functionally a "free space" tracker.
#[derive(Clone)]
pub struct BufferVec {
    pub data: Vec<u8>,
    pub cursor: usize,
    max_size: usize,
}

impl AsRef<[u8]> for BufferVec {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl BufferVec {
    /// Creates a new `BufferVec` with a specific maximum size limitation.
    /// Allocates an initial capacity of 4 bytes.
    pub fn new(max_size: usize) -> Self {
        Self::with_size(4, max_size)
    }

    pub fn with_size(initial_size: usize, max_size: usize) -> Self {
        let size = initial_size.max(4);
        let cursor = size;
        let data = unsafe {
            let mut v = Vec::<u8>::with_capacity(size);
            v.set_len(size);
            v
        };
        let max_size = max_size.next_power_of_two().max(4);
        Self { data, cursor, max_size }
    }
    #[inline(always)]
    pub const fn size(&self) -> usize {
        self.data.len()
    }

    #[inline(always)]
    pub fn handle(&self) -> TailOffset {
        TailOffset::new(self.data.len(), self.cursor)
    }
    #[inline(always)]
    pub const fn can_fit(&self, needed_bytes: usize) -> bool {
        // If the index of the head is greater than or equal to the bytes we need,
        // we have enough raw space sitting at the front of the vector!
        self.cursor >= needed_bytes
    }

    #[inline(always)]
    pub fn ensure_fit(&mut self, needed_bytes: usize) -> Result<()> {
        if self.can_fit(needed_bytes) {
            return Ok(());
        }
        let payload_len = self.len();
        let size = (payload_len + needed_bytes).next_power_of_two();
        if size > self.max_size {
            return Err(Error::InsufficientSpace);
        }
        // Uninitialized allocation: bytes below the payload are never read
        // before being written (same contract as with_size).
        let mut new = unsafe {
            let mut v = Vec::<u8>::with_capacity(size);
            v.set_len(size);
            v
        };
        new[size - payload_len..].copy_from_slice(self.as_slice());
        self.data = new;
        self.cursor = size - payload_len;
        Ok(())
    }

    pub fn grow_by(&mut self, deficit: usize) -> Result<()> {
        let payload_len = self.len();
        let size = (self.data.len() + deficit).next_power_of_two();
        if size > self.max_size {
            return Err(Error::InsufficientSpace);
        }
        let mut new = unsafe {
            let mut v = Vec::<u8>::with_capacity(size);
            v.set_len(size);
            v
        };
        new[size - payload_len..].copy_from_slice(self.as_slice());
        self.data = new;
        self.cursor = size - payload_len;
        Ok(())
    }
    
    pub const fn len(&self) -> usize {
        self.data.len() - self.cursor
    }

    pub fn is_empty(&self) -> bool {
        self.data.len() == self.cursor
    }

    pub fn reset(&mut self) {
        // Simply snap the head back to the tail (the right wall).
        // The data is now "gone" to the application, costing 0 CPU time.
        self.cursor = self.data.len();
    }

    pub fn clear(&mut self) {
        // 1. Instantly overwrite the entire underlying allocation with 0s
        self.data.fill(0);
        // 2. Reset the head pointer to the far right wall
        self.cursor = self.data.len();
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.data[self.cursor..]
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data[self.cursor..]
    }
}

/// Default implementation allows growth up to the platform's practical limits
/// while maintaining the 4-byte starting capacity.
impl Default for BufferVec {
    fn default() -> Self {
        Self::with_size(4, u32::MAX as usize) // rounds up to 2^32
    }
}