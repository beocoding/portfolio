//! # fluffr/core/src/read.rs
//!
//! The read-side of the fluffr format: zero-copy access to serialized data.
//!
//! ## Design
//!
//! No data is copied or allocated during reading.  All accessor methods return
//! either a primitive value (scalars, bools) or a view struct that borrows
//! directly from the input byte slice with lifetime `'a`.
//!
//! ## Key types
//!
//! | Type | Role |
//! |------|------|
//! | [`ReadAt`] | Trait: given `buf` and `offset`, decode a value of type `T` |
//! | [`ListView`] | Zero-copy iterator + random-access view over an array field |
//! | [`RawView`] | Low-level table accessor: holds `buf`, `t_pos`, `v_pos` |
//! | [`HasRawView`] | Marker trait on generated `XxxView` types for `merge_table_list` |
//!
//! ## Slot vs absolute position
//!
//! The write side uses **slots** (distance from the *end* of the buffer) to
//! identify objects.  The read side uses **absolute byte positions** within
//! the finished buffer slice.  `RawView::from_slot` converts between the two.

use std::marker::PhantomData;
use crate::{BitMask, DataType};

// ── ReadAt ────────────────────────────────────────────────────────────────────

/// Decode a value of type `Self` from a byte buffer at a given absolute offset.
///
/// `MODE` determines how the parent table locates this field's data:
/// - `Inline`: the bytes at `field_position` ARE the value.
/// - `Offset`: the bytes at `field_position` are a u32 forward offset; the
///   actual data starts at `field_position + forward_offset`.
///
/// The lifetime `'a` ties the decoded value to the input buffer so that
/// zero-copy reads (string slices, list views) compile correctly.
pub trait ReadAt<'a> {
    /// Whether the parent table stores this value inline or via a forward offset.
    const MODE: DataType;

    /// The type returned by `read_at`.  For scalars this is `Self`; for strings
    /// it is `&'a str`; for arrays it is `ListView<'a, T>`.
    type ReadOutput;

    /// Decode a value from `buf` at absolute byte position `offset`.
    fn read_at(buf: &'a [u8], offset: usize) -> Self::ReadOutput;

    // Specialized read method for enums/unions
    #[inline(always)]
    fn read_with_tag_at(buf: &'a [u8], offset: usize, _tag:u8) -> Self::ReadOutput {
        Self::read_at(buf, offset)
    }

    /// Return the zero/empty default for this type's `ReadOutput`.
    ///
    /// Called by [`ListView::get`] when the forward-offset entry for an element
    /// is `0`, which signals that the element slot is absent.  This only fires
    /// for `Offset`- and `Union`-mode types; `Inline` types (scalars, `bool`,
    /// structs) are always present in their list slot, so their `get` path
    /// never reaches this method.
    ///
    /// The default implementation is `unreachable!()` so that `Inline`-mode
    /// impls pay no code-size cost.  Every `Offset`/`Union` impl must override
    /// this with the appropriate zero/empty value:
    ///
    /// | Type             | Default        |
    /// |------------------|----------------|
    /// | `&str` / `String`| `""`           |
    /// | `Vec<T>`         | empty `ListView`|
    /// | `FileBlob<T>`    | empty slice view|
    /// | generated Tables | `View::default()`|
    /// | generated Unions | `None` variant |
    fn default_output() -> Self::ReadOutput;

    /// Returns the exclusive end address of the value whose payload starts at
    /// `pos`.  Overridden by Table (→ view.block_end()), Struct (→ pos +
    /// size_of), and String (→ pos + 4 + length).  Default returns `pos`
    /// (safe conservative fallback for scalars/unknown types).
    #[inline(always)]
    fn payload_block_end(_buf: &'a [u8], pos: usize) -> usize
    where Self: Sized { pos + size_of::<Self>() }
}

// ── Scalars ───────────────────────────────────────────────────────────────────

/// Generates `ReadAt` for all integer types.
///
/// Uses `read_unaligned` + `from_le` to safely read a possibly-unaligned
/// little-endian integer from an arbitrary byte position.  The unsafe block
/// is sound because `buf.as_ptr().add(offset)` is valid as long as
/// `offset + size_of::<T>() <= buf.len()`, which callers guarantee.
macro_rules! impl_read_scalar {
    ($($t:ty),*) => {$(
        impl<'a> ReadAt<'a> for $t {
            const MODE: DataType = DataType::Inline;
            type ReadOutput = Self;
            #[inline(always)]
            fn read_at(buf: &[u8], offset: usize) -> Self {
                unsafe {
                    <$t>::from_le(
                        (buf.as_ptr().add(offset) as *const $t).read_unaligned()
                    )
                }
            }

            #[inline(always)]
            fn default_output() -> Self {
                0
            }
        }
    )*};
}

impl_read_scalar!(u8, u16, u32, u64, i8, i16, i32, i64, u128, i128);

impl<'a> ReadAt<'a> for f32 {
    const MODE: DataType = DataType::Inline;
    type ReadOutput = Self;
    /// Reads the bit pattern as u32 then reinterprets as f32.
    #[inline(always)]
    fn read_at(buf: &[u8], offset: usize) -> Self {
        Self::from_bits(u32::read_at(buf, offset))
    }
    #[inline(always)]
    fn default_output() -> Self {
        0.
    }
}

impl<'a> ReadAt<'a> for f64 {
    const MODE: DataType = DataType::Inline;
    type ReadOutput = Self;
    #[inline(always)]
    fn read_at(buf: &[u8], offset: usize) -> Self {
        Self::from_bits(u64::read_at(buf, offset))
    }
    #[inline(always)]
    fn default_output() -> Self {
        0.
    }
}

impl<'a> ReadAt<'a> for bool {
    const MODE: DataType = DataType::Inline;
    type ReadOutput = bool;
    #[inline(always)]
    fn read_at(buf: &[u8], offset: usize) -> bool {
        u8::read_at(buf, offset) != 0
    }
    #[inline(always)]
    fn default_output() -> Self {
        false
    }
}

// ── Strings ───────────────────────────────────────────────────────────────────

/// String layout: `[u32 length][UTF-8 bytes...]`
///
/// `read_at` is called with the absolute position of the length prefix.
/// Returns a `&'a str` that borrows directly from the input buffer — no copy.
///
/// # Safety
/// The bytes are assumed to be valid UTF-8 (enforced at write time by
/// `str::as_bytes`).  Using `from_utf8_unchecked` avoids a redundant
/// validation scan on every read.
impl<'a> ReadAt<'a> for &str {
    const MODE: DataType = DataType::Offset;
    type ReadOutput = &'a str;
    #[inline]
    fn read_at(buf: &'a [u8], offset: usize) -> &'a str {
        let len = u32::read_at(buf, offset) as usize;
        unsafe {
            let bytes = std::slice::from_raw_parts(
                buf.as_ptr().add(offset + 4), len
            );
            std::str::from_utf8_unchecked(bytes)
        }
    }

    #[inline(always)]
    fn default_output() -> &'a str { "" }

    #[inline(always)]
    fn payload_block_end(buf: &'a [u8], pos: usize) -> usize {
        pos + 4 + u32::read_at(buf, pos) as usize
    }
}

/// Identical to `&str`; exists so that `Vec<String>` fields use the same
/// read path as `Vec<&str>` fields.  Both return `&'a str`.
impl<'a> ReadAt<'a> for String {
    const MODE: DataType = DataType::Offset;
    type ReadOutput = &'a str;
    #[inline]
    fn read_at(buf: &'a [u8], offset: usize) -> &'a str {
        let len = u32::read_at(buf, offset) as usize;
        unsafe {
            let bytes = std::slice::from_raw_parts(
                buf.as_ptr().add(offset + 4), len
            );
            std::str::from_utf8_unchecked(bytes)
        }
    }

    #[inline(always)]
    fn default_output() -> &'a str { "" }

    #[inline(always)]
    fn payload_block_end(buf: &'a [u8], pos: usize) -> usize {
        pos + 4 + u32::read_at(buf, pos) as usize
    }
}

// ── Vec<T> ────────────────────────────────────────────────────────────────────

/// Reads an array whose length is stored as a u32 length prefix immediately
/// before the element data (or offset table for indirect elements).
///
/// Returns a [`ListView`] that borrows from `buf`.  The ListView's `offset`
/// points to the first element (or first offset-table entry) — i.e. 4 bytes
/// past the length prefix.
impl<'a, T: ReadAt<'a>> ReadAt<'a> for Vec<T> {
    const MODE: DataType = DataType::Offset;
    type ReadOutput = ListView<'a, T>;

    #[inline(always)]
    fn read_at(buf: &'a [u8], offset: usize) -> ListView<'a, T> {
        ListView::new(buf, offset + 4, u32::read_at(buf, offset) as usize)
    }

    #[inline(always)]
    fn default_output() -> ListView<'a, T> { ListView::default() }
}

/// Reads a fixed-size array `[T; N]` without a length prefix.
/// Used for const-size inline buffers where `N` is known at compile time.
impl<'a, T: ReadAt<'a>, const N: usize> ReadAt<'a> for [T; N] {
    const MODE: DataType = DataType::Offset;
    type ReadOutput = ListView<'a, T>;
    #[inline(always)]
    fn read_at(buf: &'a [u8], offset: usize) -> ListView<'a, T> {
        ListView::new(buf, offset, N)
    }

    #[inline(always)]
    fn default_output() -> ListView<'a, T> { ListView::default() }
}

// ── ListView ──────────────────────────────────────────────────────────────────

/// A zero-copy, double-ended iterator and random-access view over an array
/// stored in a flatbuffer.
///
/// # Memory layout
///
/// For inline elements (`T::MODE == Inline`):
/// ```text
/// offset → [elem_0_bytes][elem_1_bytes]...[elem_{len-1}_bytes]
/// ```
///
/// For offset elements (`T::MODE == Offset`):
/// ```text
/// offset → [fwd_off_0: u32][fwd_off_1: u32]...[fwd_off_{n-1}: u32]
///                ↓               ↓
///            [elem_0 data]   [elem_1 data] ...
/// ```
/// Each forward offset is relative to its own position:
/// `abs_pos(i) = (offset + i*4) + forward_offset[i]`.
///
/// # Fields
///
/// - `buf`    — the buffer this view borrows from.
/// - `offset` — absolute byte position of the first element (or offset table).
/// - `len`    — total number of elements including any skipped ones.
/// - `next` / `back` — iterator cursors for `Iterator` and `DoubleEndedIterator`.
///   Both are **indices** into the list (0-based), starting at 0 / len and
///   converging as elements are consumed.
/// - `skip`   — unordered set of indices to omit during iteration.  Empty
///   (`&[]`) when no rows are skipped; the fast path is a single `is_empty()`
///   check so the non-skipping case is unchanged.
///
/// # Skip list
///
/// Call [`with_skip`](Self::with_skip) to attach a skip list after construction.
/// The slice does not need to be sorted.  `get` and `total_len` / `is_empty`
/// are unaffected — they operate on the raw list.  Only the iterator methods
/// (`next`, `next_back`) honour the skip list.
///
/// When the skip list is active `ExactSizeIterator` is not available because
/// the exact remaining count depends on how many skip indices fall within
/// `[next, back)`, which is O(k) to compute.  `size_hint` returns a
/// conservative upper bound of `back - next`.
#[derive(Clone, Copy)]
pub struct ListView<'a, T> {
    pub buf:    &'a [u8],
    pub offset: usize,
    pub len:    usize,
    pub back:   usize,
    pub next:   usize,
    /// Unordered indices to skip during iteration.  `&[]` when not skipping.
    pub skip:   &'a BitMask,
    _marker: PhantomData<T>,
}
static EMPTY_MASK: BitMask = BitMask{
    bits: Vec::new(),
    len: 0,
    count:0,
};
impl<'a, T: ReadAt<'a>> ListView<'a, T> {
    /// Construct a new ListView anchored at `offset` with `len` elements.
    /// No rows are skipped by default; call [`with_skip`](Self::with_skip) to
    /// attach a skip list.
    #[inline(always)]
    pub fn new(buf: &'a [u8], offset: usize, len: usize) -> Self {
        Self { buf, offset, len, back: len, next: 0, skip: &EMPTY_MASK, _marker: PhantomData }
    }

    /// Attach an unordered skip list to this view.
    ///
    /// Elements whose index appears anywhere in `skip` are silently omitted
    /// by `next()` and `next_back()`.  The slice does not need to be sorted.
    /// `get`, `total_len`, and `is_empty` are not affected.
    ///
    /// Replaces any previously attached skip list.
    #[inline(always)]
    pub fn with_skip(mut self, skip: &'a BitMask) -> Self {
        self.skip = skip;
        self
    }

    
    /// Total number of elements in this list, including skipped and already
    /// iterated ones.
    #[inline(always)]
    pub fn total_len(&self) -> usize { self.len }
    
    /// Return `true` if the list contains no elements at all (ignores skip list).
    #[inline(always)]
    pub fn is_empty(&self) -> bool { self.len == 0 }
    
    /// Compute the absolute byte position of element `index`.
    ///
    /// For inline types: `offset + index * size_of::<T>()`.
    /// For offset types: follows the forward-offset entry at
    ///   `offset + index * 4` to get the element's absolute position.
    pub fn abs_pos(&self, index: usize) -> usize {
        if T::MODE.is_inline_flag() {
            self.offset + index * std::mem::size_of::<T>()
        } else {
            let ep   = self.offset + index * 4;
            let jump = u32::read_at(self.buf, ep) as usize;
            if jump == 0 { 0 } else { ep + jump }
        }
    }

    /// Random access to element at `index`.  Panics if `index >= len`.
    /// Ignores the skip list — always returns the element at that raw index.
    ///
    /// For `Offset`- and `Union`-mode types a forward-offset entry of `0`
    /// signals an absent element; this returns [`T::default_output()`] with no
    /// buffer read beyond the entry itself.  `Inline` types (scalars, structs)
    /// are always present and skip this check entirely — the branch folds away
    /// at compile time because `T::MODE` is a `const`.
    #[inline]
    pub fn get(&self, index: usize) -> T::ReadOutput {
        let offset = self.abs_pos(index);
        if offset == 0 {return T::default_output()}
        let tag = u8::read_at(self.buf,self.offset+ 4*self.total_len() + index);
        T::read_with_tag_at(self.buf, offset, tag)
    }

    /// Absolute position of the **last** element.
    ///
    /// Used by `merge_string_list` to compute the end of the string pool:
    /// `last_offset() + 4 + u32::read_at(buf, last_offset())` gives the
    /// exclusive end of the last string's bytes.
    #[inline]
    pub fn last_offset(&self) -> usize {
        self.abs_pos(self.total_len() - 1)
    }

    /// Decode and return the last element without advancing the iterator.
    /// Ignores the skip list.
    #[inline]
    pub fn read_last(&self) -> T::ReadOutput {
        self.get(self.total_len()-1)
    }
}

impl<'a, T: ReadAt<'a>> Iterator for ListView<'a, T> {
    type Item = T::ReadOutput;

    /// Yield the next element from the front (lowest index), honouring the
    /// skip list.
    #[inline]
    fn next(&mut self) -> Option<T::ReadOutput> {
        if !self.skip.is_empty() {
            while self.next < self.back && self.skip.is_set(&self.next) {
                self.next += 1;
            }
        }
        if self.next >= self.back { return None; }
        let item = self.get(self.next);
        self.next += 1;
        Some(item)
    }

    /// Upper bound is `back - next`; may be an overcount when a skip list is
    /// active because some of those indices will be silently stepped over.
    #[inline(always)]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let upper = self.back - self.next;
        if self.skip.is_empty() {
            (upper, Some(upper))
        } else {
            (0, Some(upper))
        }
    }
}

impl<'a, T: ReadAt<'a>> DoubleEndedIterator for ListView<'a, T> {
    #[inline]
    fn next_back(&mut self) -> Option<T::ReadOutput> {
        if !self.skip.is_empty() {
            while self.next < self.back && self.skip.is_set(&(self.back - 1)) {
                self.back -= 1;
            }
        }
        if self.next >= self.back { return None; }
        self.back -= 1;
        Some(self.get(self.back))
    }
}
/// `ExactSizeIterator` is only implemented when no skip list is active, because
/// once skips are present the exact remaining count requires an O(k) scan over
/// the skip slice to count how many entries fall within `[next, back)`.
///
/// If you need the precise remaining count with an active skip list, compute it
/// as: `(back - next) - skip.iter().filter(|&&i| i >= next && i < back).count()`
impl<'a, T: ReadAt<'a>> ExactSizeIterator for ListView<'a, T> {
    #[inline(always)]
    fn len(&self) -> usize {
        debug_assert!(
            self.skip.is_empty(),
            "ExactSizeIterator::len called on a ListView with an active skip list — \
             result is an overcount; use size_hint or compute manually"
        );
        self.back - self.next
    }
}

/// `ReadAt` for `ListView` itself — allows nested arrays (`Vec<Vec<T>>`).
impl<'a, T: ReadAt<'a>> ReadAt<'a> for ListView<'a, T> {
    const MODE: DataType = DataType::Offset;
    type ReadOutput = ListView<'a, T>;
    #[inline]
    fn read_at(buf: &'a [u8], offset: usize) -> Self::ReadOutput {
        ListView::new(buf, offset + 4, u32::read_at(buf, offset) as usize)
    }
    #[inline(always)]
    fn default_output() -> ListView<'a, T> { ListView::default() }
}

/// Collect a `ListView<String>` into an owned `Vec<String>`.
impl<'a> From<ListView<'a, String>> for Vec<String> {
    fn from(view: ListView<'a, String>) -> Vec<String> {
        view.map(|s| s.to_string()).collect()
    }
}

/// Collect a `ListView<T>` into an owned `Vec<T>` for scalar/Pod types.
impl<'a, T: ReadAt<'a, ReadOutput = T> + Clone> From<ListView<'a, T>> for Vec<T> {
    fn from(view: ListView<'a, T>) -> Vec<T> {
        view.collect()
    }
}

impl<'a, T> Default for ListView<'a, T> {
    fn default() -> Self {
        Self { buf: &[], offset: 0, len: 0, back: 0, next: 0, skip: &EMPTY_MASK, _marker: PhantomData }
    }
}

// ── RawView ───────────────────────────────────────────────────────────────────

/// Low-level, type-erased view of a single table object.
///
/// Holds the three positions needed to navigate any table:
/// - `buf`   — the entire buffer the table lives in.
/// - `t_pos` — absolute position of the table object (where the vtable jump is).
/// - `v_pos` — absolute position of the vtable (computed from the jump).
///
/// All generated `XxxView` structs wrap a `RawView` as their inner field and
/// delegate field lookups to its methods.
///
/// # Vtable navigation
///
/// ```text
/// t_pos → [vtable_jump: i32][field_0_data]...[field_n_data]
///
/// v_pos = t_pos - vtable_jump   (jump is negative when vtable is before table)
///
/// v_pos → [vtable_size: u16][object_size: u16]
///          [voff_0: u16][voff_1: u16]...[voff_n: u16]
/// ```
///
/// `voff(i) == 0` means field `i` is absent (default value); non-zero means
/// the field data starts at `t_pos + voff(i)`.
#[derive(Clone, Copy, Default,)]
pub struct RawView<'a> {
    pub buf:   &'a [u8],
    /// Absolute position of the table object's first byte (the vtable jump).
    pub t_pos: usize,
    /// Absolute position of the vtable's first byte.
    pub v_pos: usize,
}

impl<'a> RawView<'a> {
    pub const EMPTY: RawView<'static> = RawView { buf: &[], t_pos: 0, v_pos: 0 };
    /// Construct a `RawView` given the absolute position of the table object.
    ///
    /// The vtable position is computed by reading the signed 32-bit jump stored
    /// at `table_pos`: `v_pos = table_pos - jump`.
    #[inline(always)]
    pub fn new(buf: &'a [u8], table_pos: usize) -> Self {
        let v_pos = (table_pos as i32 - i32::read_at(buf, table_pos)) as usize;
        Self { buf, t_pos: table_pos, v_pos }
    }

    /// Construct from a **slot** (distance from the end of `buf`).
    ///
    /// Converts `slot → absolute position` via `buf.len() - slot`, then
    /// delegates to `new`.
    #[inline(always)]
    pub fn from_slot(buf: &'a [u8], slot: usize) -> Self {
        Self::new(buf, buf.len() - slot)
    }

    /// Read the vtable offset for field `field_idx`.
    ///
    /// The vtable stores one `u16` per field starting at `v_pos + 4` (after
    /// the 2-byte vtable size and 2-byte object size).  A value of 0 means
    /// the field is absent.
    #[inline(always)]
    pub fn voff(&self, field_idx: usize) -> usize {
        u16::read_at(self.buf, self.v_pos + 4 + field_idx * 2) as usize
    }

    /// Return `true` if field `field_idx` is present (vtable offset != 0).
    #[inline(always)]
    pub fn is_present(&self, field_idx: usize) -> bool {
        self.voff(field_idx) != 0
    }

    /// Follow an `Offset`-mode field: read the forward offset at
    /// `t_pos + voff(field_idx)` and return the absolute position of the
    /// pointed-to data.
    #[inline]
    pub fn indirect_idx(&self, field_idx: usize) -> usize {
        let field_pos = self.t_pos + self.voff(field_idx);
        field_pos + u32::read_at(self.buf, field_pos) as usize
    }

    #[inline(always)]
    pub fn vtable_bytes(&self) -> &'a [u8] {
        let vt_size = u16::read_at(self.buf, self.v_pos) as usize;
        &self.buf[self.v_pos..self.v_pos + vt_size]
    }
}

// ── HasRawView ────────────────────────────────────────────────────────────────

/// Implemented by every generated `XxxView<'a>` type.
///
/// This trait exists so that [`merge_table_list`](crate::merge_table_list) can
/// access the vtable position and block boundaries of a table element view
/// without knowing its concrete type at compile time.  Without this trait the
/// generic function would require a type parameter for each table type, causing
/// a separate monomorphization per list field per table — exactly the
/// compile-time cost we are trying to avoid.
///
/// Both methods are `#[inline(always)]` in the generated impls so the
/// indirection has no runtime cost.
pub trait HasRawView<'a> {
    /// Return a reference to the inner `RawView` for this view.
    fn raw_view(&self) -> &RawView<'a>;

    /// Return the exclusive end byte position of this table's data block in
    /// the source buffer.  Equivalent to the generated `block_end()` method.
    ///
    /// Used by `merge_table_list` to determine the size of the block to copy
    /// when merging: `block_size = block_end() - t_pos`.
    fn block_end_dyn(&self) -> usize;
}

impl<'a, T: ReadAt<'a>> PartialEq for ListView<'a, T>
where
    T::ReadOutput: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        if self.total_len() != other.total_len() { return false; }
        (0..self.total_len()).all(|i| self.get(i) == other.get(i))
    }
}

impl<'a, T: ReadAt<'a>> std::fmt::Debug for ListView<'a, T>
where
    T::ReadOutput: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut list = f.debug_list();
        for i in 0..self.total_len() {
            list.entry(&self.get(i));
        }
        list.finish()
    }
}

// ── Vec<T> ↔ ListView<T> PartialEq ───────────────────────────────────────────

impl<'a, T: ReadAt<'a>> PartialEq<ListView<'a, T>> for Vec<T>
where
    T: PartialEq<T::ReadOutput>,
{
    fn eq(&self, other: &ListView<'a, T>) -> bool {
        if self.len() != other.total_len() { return false; }
        self.iter().enumerate().all(|(i, v)| *v == other.get(i))
    }
}

impl<'a, T: ReadAt<'a>> PartialEq<Vec<T>> for ListView<'a, T>
where
    T: PartialEq<T::ReadOutput>,
{
    fn eq(&self, other: &Vec<T>) -> bool { other == self }
}