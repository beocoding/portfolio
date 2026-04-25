//! # fluffr/flatr_core/src/serialize.rs
//!
//! Traits and implementations for writing Rust values into a [`Buffer`].
//!
//! ## Two-trait design
//!
//! - [`Serialize`] — for typed values (scalars, strings, arrays, table views).
//!   Every serializable type carries a `const MODE: DataType` that tells the
//!   parent table whether to write the value inline or write a 32-bit forward
//!   offset to it.
//!
//! - [`SerializeBytes`] — for raw byte slices.  Used internally to emit vtable
//!   bytes and other untyped data that does not participate in the offset/inline
//!   distinction.
//!
//! ## Write direction
//!
//! All data is written from the **high end** of the buffer toward the low end.
//! After writing a value, `buffer.slot()` is a stable "address" for that value
//! that survives subsequent `grow()` calls.  The `write_to_unchecked` variants
//! skip the capacity check and must only be called when `ensure_capacity` has
//! already been called by the caller (e.g. inside a parent table's pass 1/2).
//!
//! ## Inline vs Offset
//!
//! | `MODE`   | What is written at the field position in the table object |
//! |----------|-----------------------------------------------------------|
//! | `Inline` | The value's bytes directly (scalars, `#[repr(C)]` structs) |
//! | `Offset` | A u32 forward offset to the value (strings, arrays, tables)|
//!
//! Forward offset = `abs_pos_of_value - abs_pos_of_offset_field`, always > 0.

use crate::{DataType, buffer::Buffer, read::{ListView, ReadAt}};
use std::mem::{size_of, align_of};

// ── Serialize ─────────────────────────────────────────────────────────────────

/// Trait for values that can be written into a [`Buffer`].
///
/// The proc-macro generates implementations for every `#[derive(Table)]` and
/// `#[derive(Flat)]` struct; built-in implementations cover all primitive
/// scalars, `String`, `&str`, `Vec<T>`, `&[T]`, and `ListView<T>`.
pub trait Serialize: Sized {
    const SIZE: usize = size_of::<Self>(); // Unions override as 5, Non scalars override as 4
    const ALIGN: usize = align_of::<Self>(); // Non scalars override as 4
    const ALIGNR: usize = Self::ALIGN -1;
    const ALIGN_MASK: usize = !Self::ALIGNR;

    /// Whether this value is written inline at the field position (`Inline`)
    /// or whether a 32-bit forward offset to the value is written instead
    /// (`Offset`).  This constant is inspected by parent arrays and tables
    /// at compile time to choose the correct write strategy.
    const MODE: DataType;

    /// Upper bound on the number of bytes this value and its alignment padding
    /// will consume.  Used by `write_to` and parent tables to call
    /// `ensure_capacity` before entering the unchecked fast path.
    fn size_hint(&self) -> usize;

    /// Write this value into `buffer`, calling `ensure_capacity` first.
    /// Returns the slot of the outermost written unit (length prefix for arrays,
    /// value start for scalars).
    fn write_to<B: Buffer>(&self, buffer: &mut B) -> usize;

    /// Write this value into `buffer` **without** checking capacity first.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that `buffer.head() > size_hint()` before
    /// calling this method.  Violating this will cause a panic on the
    /// slice indexing inside, or — if `debug_assert` is disabled — silent
    /// memory corruption.
    fn write_to_unchecked<B: Buffer>(&self, buffer: &mut B) -> usize;

    /// Return `true` if this value is the default/zero/absent state.
    ///
    /// The proc-macro uses this to skip writing absent fields to the table
    /// object and leave their vtable entry as 0 (absent marker).  Scalars
    /// are absent when equal to 0; strings/arrays when empty; tables when
    /// all their own fields are absent.
    fn is_absent(&self) -> bool;


    // special access method for enum/unions
    #[inline(always)]
    fn tag(&self) -> u8 {
        0
    }
}

/// Trait for writing raw byte sequences without the inline/offset distinction.
///
/// Used for vtable bytes (emitted by `share_vtable`) and similar cases where
/// the data is already in its final wire form.
pub trait SerializeBytes {
    fn size_hint(&self) -> usize;
    fn write_bytes_to<B: Buffer>(&self, buffer: &mut B) -> usize;
    fn write_bytes_to_unchecked<B: Buffer>(&self, buffer: &mut B) -> usize;
}

// ── Dense bytes (&[u8]) ───────────────────────────────────────────────────────

/// `SerializeBytes` for raw byte slices — no alignment, no length prefix,
/// bytes are written verbatim from the high end of the buffer.
impl SerializeBytes for &[u8] {
    #[inline(always)]
    fn size_hint(&self) -> usize { self.len() }
    #[inline]
    fn write_bytes_to<B: Buffer>(&self, buffer: &mut B) -> usize {
        buffer.ensure_capacity(self.len());
        self.write_bytes_to_unchecked(buffer)
    }
    #[inline]
    fn write_bytes_to_unchecked<B: Buffer>(&self, buffer: &mut B) -> usize {
        let len = self.len();
        *buffer.head_mut() -= len;
        let head = buffer.head();
        buffer.buffer_mut()[head..head + len].copy_from_slice(self);
        buffer.slot()
    }
}

// ── Scalars ───────────────────────────────────────────────────────────────────

/// Generates `Serialize` for all primitive integer and float types.
///
/// The layout for a scalar `T`:
/// ```text
/// head is decremented by size_of::<T>(), then masked to align_of::<T>()
/// T's little-endian bytes are written at the resulting head position
/// ```
macro_rules! impl_serialize_scalar {
    ($($t:ty),*) => {$(
        impl Serialize for $t {
            const MODE: DataType = DataType::Inline;
            /// `size_of + align_of - 1` — worst-case bytes including alignment padding.
            #[inline(always)]
            fn size_hint(&self) -> usize { size_of::<Self>() + align_of::<Self>() - 1 }
            #[inline]
            fn write_to<B: Buffer>(&self, buffer: &mut B) -> usize {
                buffer.ensure_capacity(size_of::<Self>() + align_of::<Self>() - 1);
                self.write_to_unchecked(buffer)
            }
            #[inline]
            fn write_to_unchecked<B: Buffer>(&self, buffer: &mut B) -> usize {
                let size = size_of::<Self>();
                let mask = align_of::<Self>() - 1;
                *buffer.head_mut() -= size;
                *buffer.head_mut() &= !mask;
                let head = buffer.head();
                buffer.buffer_mut()[head..head + size]
                    .copy_from_slice(&self.to_le_bytes());
                buffer.slot()
            }
            #[inline(always)]
            fn is_absent(&self) -> bool { *self == (0 as $t) }
        }
    )*};
}
impl Serialize for u8 {
    const MODE: DataType = DataType::Inline;
    /// `size_of + align_of - 1` — worst-case bytes including alignment padding.
    #[inline(always)]
    fn size_hint(&self) -> usize { 1 }
    #[inline]
    fn write_to<B: Buffer>(&self, buffer: &mut B) -> usize {
        buffer.ensure_capacity(1);
        self.write_to_unchecked(buffer)
    }
    #[inline]
    fn write_to_unchecked<B: Buffer>(&self, buffer: &mut B) -> usize {
        let size = 1;
        *buffer.head_mut() -= size;
        let head = buffer.head();
        buffer.buffer_mut()[head]= *self;
        buffer.slot()
    }
    #[inline(always)]
    fn is_absent(&self) -> bool { *self == 0 }
}

impl_serialize_scalar!(u16, u32, u64, u128, i8, i16, i32, i64, i128, f32, f64);

// ── Strings ───────────────────────────────────────────────────────────────────

/// Generates `Serialize` for `&str` and `String`.
///
/// String layout in the buffer (high to low):
/// ```text
/// [u32 length prefix][UTF-8 bytes (4-byte aligned)]
/// ```
/// The slot returned is the slot of the length prefix.  The parent table
/// writes a forward offset pointing to this slot.
macro_rules! impl_serialize_str {
    ($($t:ty),*) => {$(
        impl Serialize for $t {
            const SIZE: usize = 4;
            const ALIGN: usize = 4;
            const MODE: DataType = DataType::Offset;
            /// `len + 11` — 4 bytes for length prefix, 4 bytes alignment, 3 bytes
            /// worst-case padding before the u32 align step.
            #[inline(always)]
            fn size_hint(&self) -> usize { self.len() + 11 }
            #[inline]
            fn write_to<B: Buffer>(&self, buffer: &mut B) -> usize {
                buffer.ensure_capacity(Serialize::size_hint(self));
                self.write_to_unchecked(buffer)
            }
            #[inline]
            fn write_to_unchecked<B: Buffer>(&self, buffer: &mut B) -> usize {
                let bytes    = self.as_bytes();
                let data_len = bytes.len();
                // Step 1: write UTF-8 bytes at 4-byte-aligned position.
                *buffer.head_mut() -= data_len;
                *buffer.head_mut() &= !3;
                let data_head = buffer.head();
                buffer.buffer_mut()[data_head..data_head + data_len]
                    .copy_from_slice(bytes);
                // Step 2: write the u32 length prefix immediately before the data.
                (data_len as u32).write_to_unchecked(buffer)
            }
            #[inline(always)]
            fn is_absent(&self) -> bool { self.is_empty() }
        }
    )*};
}

impl_serialize_str!(&str, String);

// ── Arrays (&[T] and Vec<T>) ──────────────────────────────────────────────────

/// `Serialize` for `&[T]`.
///
/// Arrays are always `DataType::Offset` from the parent table's perspective —
/// the table writes a forward offset to the array regardless of whether the
/// elements are inline or offset themselves.
///
/// # Case A — Inline elements (`T::MODE == Inline`)
///
/// All elements are written as a single packed memcpy (using `from_raw_parts`
/// to reinterpret the slice as bytes), followed by a u32 length prefix.
/// The element size and alignment are determined at compile time via `size_of`
/// and `align_of`.
///
/// # Case B — Offset elements (`T::MODE == Offset`)
///
/// Elements are written in reverse order (last element first, at the high end)
/// so that element 0 ends up at the lowest address, matching iteration order.
/// A forward-offset table (n × u32) is then written pointing to each element's
/// slot.  Finally the u32 length prefix is written before the offset table.
impl<T: Serialize> Serialize for &[T] {
    const SIZE: usize = 4;
    const ALIGN: usize = 4;
    const MODE: DataType = DataType::Offset;
    #[inline]
    fn size_hint(&self) -> usize {
        match T::MODE {
            DataType::Inline => {
                (self.len() * size_of::<T>()) + align_of::<T>().max(4) + 4
            }
            DataType::Offset => {
                let data: usize = self.iter().map(|s| Serialize::size_hint(s)).sum();
                4 + (self.len() * 4) + data
            }
            DataType::Union => {
                let data: usize = self.iter().map(|s| Serialize::size_hint(s)).sum();
                let offset: usize = (7+5*self.len())&!3;
                data + offset
            }
        }
    }

    #[inline]
    fn write_to<B: Buffer>(&self, buffer: &mut B) -> usize {
        buffer.ensure_capacity(self.size_hint());
        self.write_to_unchecked(buffer)
    }

    #[inline]
    fn write_to_unchecked<B: Buffer>(&self, buffer: &mut B) -> usize {
        match T::MODE {
            DataType::Inline => {
                let len        = self.len();
                let total      = len * size_of::<T>();
                let align_mask = align_of::<T>().max(4) - 1;
                *buffer.head_mut() -= total;
                *buffer.head_mut() &= !align_mask;
                let head = buffer.head();
                // Safety: T: Serialize implies T: Pod (via the Flat derive or
                // built-in impls), so reinterpreting as bytes is valid.
                let src = unsafe {
                    std::slice::from_raw_parts(self.as_ptr() as *const u8, total)
                };
                buffer.buffer_mut()[head..head + total].copy_from_slice(src);
                (len as u32).write_to_unchecked(buffer)
            }
            _ => {
                let len = self.len();
                let mut slots = Vec::with_capacity(len);
                // Write elements in reverse so element 0 is at the lowest address.
                // In &[T]::write_to_unchecked, DataType::Union arm:
                for s in self.iter().rev() {
                    slots.push(Serialize::write_to_unchecked(s, buffer));
                };
                // Align to 4 bytes before the tag section — Table payloads (vtable = 6 bytes,
                // not a multiple of 4) leave head misaligned, causing the jump alignment step
                // to create a gap between tags and jumps so the reader sees the wrong byte.
                *buffer.head_mut() &= !3;
                let union_flag = T::MODE.is_union_flag() as usize;
                let padding = (3 & (4-(len&3))) * union_flag;
                *buffer.head_mut() -= padding;
                for i in (0..len*union_flag).rev() {
                    unsafe { self.get_unchecked(i).tag().write_to_unchecked(buffer) };
                };
                // Write the forward-offset table after all elements.
                // Align first so the jump reflects the actual entry position.
                for target_slot in slots {
                    *buffer.head_mut() -= 4;
                    *buffer.head_mut() &= !3;
                    let head = buffer.head();
                    let jump = if target_slot == 0 { 0u32 }
                        else { (buffer.slot() - target_slot) as u32 };
                    buffer.buffer_mut()[head..head + 4]
                        .copy_from_slice(&jump.to_le_bytes());
                }
                (len as u32).write_to_unchecked(buffer)
            }
        }
    }
    #[inline(always)]
    fn is_absent(&self) -> bool { self.is_empty() }
}

/// `Serialize` for `Vec<T>` — delegates to the `&[T]` implementation.
impl<T: Serialize> Serialize for Vec<T> {
    const SIZE: usize = 4;
    const MODE: DataType = DataType::Offset;
    #[inline]
    fn size_hint(&self) -> usize     { Serialize::size_hint(&self.as_slice()) }
    #[inline]
    fn write_to<B: Buffer>(&self, b: &mut B) -> usize {
        Serialize::write_to(&self.as_slice(), b)
    }
    #[inline]
    fn write_to_unchecked<B: Buffer>(&self, b: &mut B) -> usize {
        Serialize::write_to_unchecked(&self.as_slice(), b)
    }
    #[inline(always)]
    fn is_absent(&self) -> bool { self.is_empty() }
}

// ── ListView<T> ───────────────────────────────────────────────────────────────

/// `Serialize` for [`ListView`] — enables zero-copy re-serialization of a
/// view directly into a new buffer without materializing a `Vec`.
///
/// For inline element types, the source bytes are memcopied directly from the
/// view's backing buffer slice — no element-by-element dispatch.  For offset
/// types, the offset table must be rewritten because absolute positions change
/// in the destination buffer; element values are re-serialized individually.
impl<'a, T> Serialize for ListView<'a, T>
where
    T: ReadAt<'a>,
    T::ReadOutput: Serialize,
{
    const SIZE: usize = 4;
    const MODE: DataType = DataType::Offset;

    #[inline]
    fn size_hint(&self) -> usize {
        match <T::ReadOutput as Serialize>::MODE {
            DataType::Inline => {
                let elem_size = size_of::<T::ReadOutput>();
                let align     = align_of::<T::ReadOutput>().max(4);
                4 + self.len() * elem_size + align
            }
            DataType::Offset => {
                let data: usize = (0..self.len())
                    .map(|i| Serialize::size_hint(&self.get(i)))
                    .sum();
                4 + self.len() * 4 + data
            }
            DataType::Union => {
                let data: usize = (0..self.len())
                    .map(|i| Serialize::size_hint(&self.get(i)))
                    .sum();
                let offset: usize = (7+5*self.len())&!3;
                data + offset
            }
        }
    }

    #[inline]
    fn write_to<B: Buffer>(&self, buffer: &mut B) -> usize {
        buffer.ensure_capacity(Serialize::size_hint(self));
        self.write_to_unchecked(buffer)
    }

    #[inline]
    fn write_to_unchecked<B: Buffer>(&self, buffer: &mut B) -> usize {
        match <T::ReadOutput as Serialize>::MODE {
            DataType::Inline => {
                let len = self.len();
                // Fast path: one memcpy of the packed element bytes.
                let elem_size  = size_of::<T::ReadOutput>();
                let total      = len * elem_size;
                let align_mask = align_of::<T::ReadOutput>().max(4) - 1;
                *buffer.head_mut() -= total;
                *buffer.head_mut() &= !align_mask;
                let head = buffer.head();
                buffer.buffer_mut()[head..head + total]
                    .copy_from_slice(&self.buf[self.offset..self.offset + total]);
                (len as u32).write_to_unchecked(buffer)
            }
            _ => {
                let len = self.len();
                let mut slots = Vec::with_capacity(len);
                // Write elements in reverse so element 0 is at the lowest address.
                for i in (0..len).rev() {
                    slots.push(Serialize::write_to_unchecked(&self.get(i), buffer));
                }
                *buffer.head_mut() &= !3;
                let union_flag =T::MODE.is_union_flag() as usize;
                let padding = (3 & (4-(len&3))) * union_flag;
                *buffer.head_mut() -= padding;
                for i in (0..len*union_flag).rev() {
                    self.get(i).tag().write_to_unchecked(buffer);
                };
                // Write the forward-offset table after all elements.
                // Align first so the jump reflects the actual entry position.
                for target_slot in slots {
                    *buffer.head_mut() -= 4;
                    *buffer.head_mut() &= !3;
                    let head = buffer.head();
                    let jump = (buffer.slot() - target_slot) as u32;
                    buffer.buffer_mut()[head..head + 4]
                        .copy_from_slice(&jump.to_le_bytes());
                }
                (len as u32).write_to_unchecked(buffer)
            }
        }
    }
    #[inline(always)]
    fn is_absent(&self) -> bool { self.is_empty() }
}
