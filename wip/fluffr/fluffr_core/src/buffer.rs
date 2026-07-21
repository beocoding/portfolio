//! # fluffr/flatr_core/src/buffer.rs
//!
//! The write-side of the fluffr format: a backward-growing byte buffer, the
//! `Buffer` trait that abstracts over it, and the runtime merge helpers that
//! the proc-macro calls instead of emitting inline logic per field.
//!
//! ## Buffer layout
//!
//! All data is written from the **high end** of the allocation toward the
//! low end.  `head` is the index of the first written byte; `slot` is the
//! distance from the *end* of the backing `Vec` to `head`.
//!
//! ```text
//! index:  0          head                              len
//!         ┌──────────┬─────────────────────────────────┐
//!         │(garbage) │  written data (high → low addr)  │
//!         └──────────┴─────────────────────────────────┘
//!                    ↑ head
//! slot = len - head  ────────────────────────────────────
//! ```
//!
//! Absolute positions inside the buffer are expressed as **slots** (distance
//! from the end) so they remain valid if the buffer is grown and the data is
//! shifted right.
//!
//! ## Cook-Mertz uninitialized allocation
//!
//! All data in a flatbuffer is only ever accessed through vtable offsets.
//! There is no sequential scan over the raw bytes.  Therefore bytes that have
//! not been explicitly written are never dereferenced, and zero-initializing
//! the backing allocation is pure waste.  Following the Cook-Mertz catalytic
//! computation insight — registers may contain arbitrary "garbage" and clean
//! computation still produces the correct result — both `new()` and `grow()`
//! use `Vec::with_capacity` + `set_len` rather than `vec![0u8; n]`.
//!
//! ## Merge buffer reuse
//!
//! `reset()` resets `head` to `len` and clears the vtable cache without
//! freeing the backing allocation.  Callers hold one `DefaultBuffer` across
//! many merge calls and call `merge_into` which resets on entry.  This
//! eliminates the `malloc + memset` that previously dominated merge cost at
//! small data sizes.
//!
//! ## Merge runtime helpers
//!
//! `merge_inline_list`, `merge_string_list`, `merge_table_list`, and
//! `write_union_slot` are called by the proc-macro-generated `merge_into`
//! method instead of emitting the full logic inline per field.  Moving the
//! bodies here reduces the token count emitted per `#[derive(Table)]` struct
//! and avoids the monomorphization explosion that `#[inline(always)]` on
//! large generic bodies would otherwise cause.

use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher};

use crate::Table;
use crate::serialize::SerializeBytes;

// ── Hasher ────────────────────────────────────────────────────────────────────

/// FNV-1a hasher used exclusively for the vtable deduplication map.
///
/// VTable bytes are short (typically 4–30 bytes) and highly repetitive
/// within a single serialization — all elements of the same list type
/// share an identical vtable.  FNV-1a is fast for short keys and produces
/// good distribution for this workload without the overhead of SipHash.
pub struct VTableHasher(u64);

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME:  u64 = 0x00000100000001b3;

impl Hasher for VTableHasher {
    #[inline(always)]
    fn finish(&self) -> u64 { self.0 }

    #[inline(always)]
    fn write(&mut self, bytes: &[u8]) {
        let mut h = self.0;
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        self.0 = h;
    }
}

/// [`BuildHasher`] that produces [`VTableHasher`] instances.
pub struct BuildVTableHasher;

impl BuildHasher for BuildVTableHasher {
    type Hasher = VTableHasher;
    fn build_hasher(&self) -> Self::Hasher { VTableHasher(FNV_OFFSET) }
}

// ── DefaultBuffer ─────────────────────────────────────────────────────────────

/// The standard [`Buffer`] implementation.
///
/// # Fields
///
/// - `buffer` — the raw backing allocation.  Data grows from the high end
///   toward the low end; bytes below `head` are uninitialized or stale and
///   must never be read.
/// - `head` — index of the first byte of valid data.  Always satisfies
///   `head <= buffer.len()`.
/// - `vtables` — deduplication map from vtable bytes to the slot where that
///   vtable was written.  Prevents writing the same vtable multiple times
///   when many table elements share identical schemas (the common case).
pub struct DefaultBuffer {
    pub buffer:  Vec<u8>,
    pub head:    usize,
    pub vtables: HashMap<Box<[u8]>, usize, BuildVTableHasher>,
}

impl Default for DefaultBuffer {
    fn default() -> Self { Self::new(1024) }
}

impl Buffer for DefaultBuffer {
    /// Allocate an uninitialized buffer of `initial_size` bytes.
    ///
    /// Uses `with_capacity` + `set_len` rather than `vec![0; n]` — see
    /// the module-level Cook-Mertz discussion for why zero-init is safe
    /// to skip here.
    #[inline(always)]
    fn new(initial_size: usize) -> Self {
        let buffer = unsafe {
            let mut v = Vec::<u8>::with_capacity(initial_size);
            v.set_len(initial_size);
            v
        };
        Self {
            buffer,
            head:    initial_size,
            vtables: HashMap::with_hasher(BuildVTableHasher),
        }
    }
    #[inline(always)]
    fn clear(&mut self) {
        self.buffer.clear();
        self.head = self.buffer.len();
        self.vtables.clear();
    }
    fn buffer(&self)         -> &[u8]      { &self.buffer }
    fn buffer_mut(&mut self) -> &mut [u8]  { &mut self.buffer }
    fn head(&self)           -> usize      { self.head }
    fn head_mut(&mut self)   -> &mut usize { &mut self.head }

    /// Grow the buffer to `new_cap` bytes, shifting all existing written data
    /// to the right so that slot-based offsets remain valid.
    ///
    /// Uses `reserve` + `set_len` rather than `resize(n, 0)` for the same
    /// reason as `new`: the new bytes in the low region are never read before
    /// being written.
    fn grow(&mut self, new_cap: usize) {
        debug_assert!(new_cap > self.buffer.len(),
            "grow: new_cap ({new_cap}) must exceed current len ({})", self.buffer.len());

        let old_cap = self.buffer.len();
        let shift   = new_cap - old_cap;

        self.buffer.reserve(shift);
        unsafe { self.buffer.set_len(new_cap); }
        // Move the existing written data (0..old_cap) to (shift..new_cap)
        // so it occupies the same slot-based positions in the new layout.
        self.buffer.copy_within(0..old_cap, shift);
        self.head += shift;
    }

    #[inline(always)]
    fn clear_vtables(&mut self) { self.vtables.clear(); }

    /// Write `vtable` to the buffer if not already present, then patch the
    /// 4-byte vtable-jump field at the start of the table object.
    ///
    /// The vtable jump is a **signed** 32-bit integer stored at `t_pos`:
    ///   `jump = vtable_pos - table_pos`  (negative when vtable is before table)
    ///
    /// Multiple table elements with the same schema share one physical vtable
    /// copy, which is why the dedup map is essential for space efficiency.
    fn share_vtable(&mut self, vtable: &[u8], table_slot: usize) {
        let vtable_slot = if let Some(&slot) = self.vtables.get(vtable) {
            slot
        } else {
            let slot = vtable.write_bytes_to(self);
            self.vtables.insert(Box::from(vtable), slot);
            slot
        };

        let vtable_jump     = (vtable_slot as i32) - (table_slot as i32);
        let table_start_idx = self.len() - table_slot;

        self.buffer_mut()[table_start_idx..table_start_idx + 4]
            .copy_from_slice(&vtable_jump.to_le_bytes());
    }

    #[inline(always)]
    fn load<T: Table>(bytes: &[u8]) -> Self {
        let size = bytes.len();
        let buffer = Self::new(size);
        buffer
    }
}

/// Read the root offset from the first 4 bytes of a finished flatbuffer.
///
/// The root offset is a u32 at position 0 that gives the byte distance from
/// itself to the root table object.  `root_offset + 0 = table position`.
pub fn read_root(data: &[u8]) -> u32 {
    let mut root_bytes = [0u8; 4];
    root_bytes.copy_from_slice(&data[0..4]);
    u32::from_le_bytes(root_bytes)
}

// ── Buffer trait ──────────────────────────────────────────────────────────────

/// Abstraction over a backward-growing byte buffer used during serialization
/// and merging.
///
/// The contract: data is written from `len()` toward 0.  `head()` is the
/// current write frontier.  `slot()` = `len() - head()` is a stable handle
/// to the last object written — it remains valid even if `grow()` is called
/// and the entire allocation is shifted.
///
/// All provided methods are implemented in terms of the required primitives;
/// implementors only need to supply `new`, `head`/`head_mut`,
/// `buffer`/`buffer_mut`, `grow`, `clear_vtables`, `share_vtable`, and
/// `load`.
pub trait Buffer {
    /// Construct a new buffer with at least `initial_capacity` bytes available
    /// for writing.
    fn new(initial_capacity: usize) -> Self;
    fn clear(&mut self);

    /// Return the current write frontier: the index of the first valid byte.
    fn head(&self) -> usize;

    /// Return a mutable reference to the write frontier so callers can
    /// advance it with `*buf.head_mut() -= n`.
    fn head_mut(&mut self) -> &mut usize;

    /// Return the full backing byte slice (including the unwritten low region).
    fn buffer(&self) -> &[u8];

    /// Return a mutable view of the full backing byte slice.
    fn buffer_mut(&mut self) -> &mut [u8];

    /// Write or look up a vtable and patch the jump field of the table object
    /// at `table_slot`.  See [`DefaultBuffer::share_vtable`] for the layout.
    fn share_vtable(&mut self, vtable: &[u8], table_slot: usize);

    /// Grow the backing allocation to exactly `new_cap` bytes, shifting all
    /// existing written data toward the high end.  Must update `head` by
    /// `new_cap - old_cap`.
    fn grow(&mut self, new_cap: usize);

    /// Deserialize a finished flatbuffer into this buffer type.
    fn load<T>(bytes: &[u8]) -> Self where T: Table;

    // ── Provided ──────────────────────────────────────────────────────────────

    /// Clear the vtable deduplication cache.  Called by [`reset`](Self::reset).
    fn clear_vtables(&mut self);

    /// Reset the buffer for reuse **without** freeing the backing allocation.
    ///
    /// Resets `head` to `len()` (as if the buffer were freshly allocated) and
    /// clears the vtable cache.  The bytes below `head` are now garbage — the
    /// Cook-Mertz contract guarantees they will not be read before being
    /// overwritten.
    ///
    /// Called automatically by `merge_into` on entry so the caller only needs
    /// to allocate once and reuse across many merge calls.
    #[inline(always)]
    fn reset(&mut self) {
        let cap = self.len();
        *self.head_mut() = cap;
        self.clear_vtables();
    }

    /// Total capacity of the backing allocation in bytes.
    #[inline(always)]
    fn len(&self) -> usize { self.buffer().len() }

    /// Ensure `additional_size` bytes are available below `head`, growing the
    /// buffer if necessary.
    #[inline(always)]
    fn ensure_capacity(&mut self, additional_size: usize) {
        if self.head() <= additional_size {
            let new_cap = (self.len() + additional_size).next_power_of_two();
            self.grow(new_cap);
        }
    }

    /// Distance from the end of the buffer to the current write frontier.
    ///
    /// This is the stable "slot" address of the most-recently-written object.
    /// It remains valid across `grow()` calls because growing shifts data
    /// right by exactly the amount `head` is incremented.
    #[inline(always)]
    fn slot(&self) -> usize { self.len() - self.head() }

    /// Align `head` downward to `alignment` bytes (must be a power of two).
    #[inline(always)]
    fn align(&mut self, alignment: usize) {
        *self.head_mut() &= !(alignment - 1);
    }

    /// The finished, readable byte slice starting at `head`.
    ///
    /// Only valid after `finish()` has been called to write the root prefix.
    #[inline(always)]
    fn bytes(&self) -> &[u8] { &self.buffer()[self.head()..] }

    /// Write the 4-byte root prefix and return the finished buffer slice.
    ///
    /// The root prefix is a u32 at the lowest address that encodes
    /// `table_pos - prefix_pos`, i.e. the byte distance from the prefix to
    /// the root table object.  Readers call [`read_root`] to find the table.
    #[inline(always)]
    fn finish(&mut self, slot: usize) -> &[u8] {
        self.ensure_capacity(4);
        let table_pos = self.len() - slot;
        *self.head_mut() -= 4;
        let head = self.head();
        let relative_offset = (table_pos - head) as u32;
        self.buffer_mut()[head..head + 4]
            .copy_from_slice(&relative_offset.to_le_bytes());
        self.bytes()
    }
}

// ── Blanket impl: &mut B: Buffer ──────────────────────────────────────────────

/// Forwarding impl so that `merge_inline_list` and friends, which are generic
/// over `B: Buffer` and receive `out: &mut B`, can call `Buffer` methods
/// without an extra wrapper type.
///
/// `new` and `load` are not meaningful on a mutable reference and will panic
/// if called — callers must always allocate the owned `B` first and pass a
/// `&mut` to the helpers.
impl<B: Buffer> Buffer for &mut B {
    #[inline(always)]
    fn new(_: usize) -> Self {
        panic!("Buffer::new on &mut B — allocate the owned B first")
    }
    #[inline(always)]
    fn clear(&mut self) {
        (**self).clear()
    }
    #[inline(always)]
    fn head(&self)             -> usize       { (**self).head() }
    #[inline(always)]
    fn head_mut(&mut self)     -> &mut usize  { (**self).head_mut() }
    #[inline(always)]
    fn buffer(&self)           -> &[u8]       { (**self).buffer() }
    #[inline(always)]
    fn buffer_mut(&mut self)   -> &mut [u8]   { (**self).buffer_mut() }
    #[inline(always)]
    fn grow(&mut self, n: usize)              { (**self).grow(n) }
    #[inline(always)]
    fn clear_vtables(&mut self)               { (**self).clear_vtables() }
    #[inline(always)]
    fn share_vtable(&mut self, vt: &[u8], slot: usize) {
        (**self).share_vtable(vt, slot)
    }
    #[inline(always)]
    fn load<T: crate::Table>(_: &[u8]) -> Self {
        panic!("Buffer::load on &mut B — not supported")
    }
}
