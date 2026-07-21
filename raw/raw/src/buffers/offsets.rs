use std::marker::PhantomData;
use crate::buffers::{error::{Error, Result}, vec::Error::{InvalidPivot}};

// =========================================================================
// Core Trait Definition
// =========================================================================
pub trait Follow {
    type Inner<'a>;
    /// Reads or references data from the buffer at the specified location coordinate.
    ///
    /// Contract: all accesses are bounds-checked. Malformed or truncated
    /// input yields `Err`, never a panic and never UB — the read path must
    /// assume bytes are adversarial even when they come from our own writer.
    fn follow(buf: &[u8], loc: usize) -> Result<Self::Inner<'_>>;
}

// =========================================================================
// Layout Abstraction Wrappers (Zero-Sized Strategy Blueprints)
// =========================================================================
/// A stored 0 means "absent / skipped at write time" and follows
/// to `None`; any other value is a forward jump relative to this slot.
/// This is exactly the convention the string-array writer emits into its
/// jump table, so `NOffset<&str>` reads one slot of a serialized `[&str]`.
/// 
#[derive(Copy, Clone)]
pub struct UOffset<T>(PhantomData<T>);
#[derive(Copy, Clone)]
pub struct Select<T>(PhantomData<T>);
#[derive(Copy, Clone)]
pub struct SOffset<T>(PhantomData<T>);
#[derive(Copy, Clone)]
pub struct VOffset<T>(PhantomData<T>);

// =========================================================================
// Trait Implementations for Structural Wrappers
// =========================================================================


impl<T: Follow> Follow for UOffset<T> {
    // We just pass the lifetime down into the next strategy layer
    type Inner<'a> = T::Inner<'a>;

    #[inline(always)]
    fn follow(buf: &[u8], loc: usize) -> Result<Self::Inner<'_>> {
        let offset = u32::follow(buf, loc)? as usize;
        let target = loc.checked_add(offset).ok_or(Error::OutOfBounds)?;
        Ok(T::follow(buf, target)?)
    }
}

impl<T: Follow> Follow for SOffset<T> {
    type Inner<'a> = T::Inner<'a>;

    #[inline(always)]
    fn follow(buf: &[u8], loc: usize) -> Result<Self::Inner<'_>> {
        let offset = i32::follow(buf, loc)? as isize;
        if offset == 0 {return Err(InvalidPivot)};
        // Jump backward (or forward, for negative soffsets) from the slot.
        // checked_sub on the signed value: a hostile offset must not wrap.
        let target = (loc as isize)
            .checked_sub(offset)
            .filter(|&t| t >= 0)
            .ok_or(Error::OutOfBounds)? as usize;
        Ok(T::follow(buf, target)?)
    }
}

// =========================================================================
// Tables & VTables
// =========================================================================
//
// A vtable entry is a u16 offset *relative to the table start*, not to the
// vtable slot that stores it. That single fact is why field lookup cannot
// be expressed as a pure `Follow` chain: by the time you are positioned at
// the vtable slot, the table position — the base the u16 is relative to —
// is no longer in scope. (It also has to be table-relative rather than
// slot-relative, because vtables are shared: two tables with the same shape
// point at one deduplicated vtable, so the entries cannot encode anything
// specific to a single table's location.)
//
// The resolution is a carrier struct: `Table::follow` performs the
// SOffset hop and captures both coordinates, and field access happens on
// the resulting `TableRef`, which has everything it needs.
//
// VTable layout (defining the wire convention for the writer to match):
//   [u16 vtable_byte_len][u16 table_byte_len][u16 field_0_off][u16 field_1_off]...
// A field offset of 0, or a field id beyond vtable_byte_len, means the
// field is absent — the latter is what makes schema evolution work, since
// old data simply has a shorter vtable than the new schema expects.

pub struct Table;

#[derive(Debug)]
pub struct TableRef<'a> {
    buf: &'a [u8],
    /// Absolute position of the table (base for all field offsets).
    loc: usize,
    /// Absolute position of the vtable this table points at.
    vtable: usize,
}

impl Follow for Table {
    type Inner<'a> = TableRef<'a>;

    #[inline(always)]
    fn follow(buf: &[u8], loc: usize) -> Result<TableRef<'_>> {
        let soffset = i32::follow(buf, loc)? as isize;
        let vtable = (loc as isize)
            .checked_sub(soffset)
            .filter(|&t| t >= 0)
            .ok_or(Error::OutOfBounds)? as usize;

        // Eagerly validate the vtable header so field lookups can trust it.
        let vtable_len = u16::follow(buf, vtable)? as usize;
        if vtable_len < 4 {
            return Err(Error::OutOfBounds); // header alone is 4 bytes
        }
        let vtable_end = vtable.checked_add(vtable_len).ok_or(Error::OutOfBounds)?;
        if vtable_end > buf.len() {
            return Err(Error::OutOfBounds);
        }
        Ok(TableRef { buf, loc, vtable })
    }
}

impl<'a> TableRef<'a> {
    /// Resolves field `id` through the vtable. Returns `Ok(None)` when the
    /// field is absent — either its slot holds 0 (default-suppressed at
    /// write time) or the vtable is too short to contain the slot at all
    /// (data written by an older schema).
    #[inline]
    pub fn get<T: Follow>(&self, id: u16) -> Result<Option<T::Inner<'a>>> {
        let vtable_len = u16::follow(self.buf, self.vtable)? as usize;
        let slot = self.vtable + 4 + (id as usize) * 2;
        if slot + 2 > self.vtable + vtable_len {
            return Ok(None); // field id past this vtable: older writer
        }
        let field_off = u16::follow(self.buf, slot)? as usize;
        if field_off == 0 {
            return Ok(None); // field suppressed
        }
        let target = self.loc.checked_add(field_off).ok_or(Error::OutOfBounds)?;
        Ok(Some(T::follow(self.buf, target)?))
    }

    /// Like `get`, but substitutes `default` for absent fields — the usual
    /// shape for scalar table fields with schema defaults.
    #[inline]
    pub fn get_or<T: Follow>(&self, id: u16, default: T::Inner<'a>) -> Result<T::Inner<'a>> {
        Ok(self.get::<T>(id)?.unwrap_or(default))
    }
}

// =========================================================================
// Macro & Innermost Primitive Trait Implementations
// =========================================================================
#[macro_export]
macro_rules! impl_follow_for_primitive {
    ($($ty:ty),*) => {
        $(
            impl Follow for $ty {
                type Inner<'a> = $ty;

                #[inline(always)]
                fn follow(buf: &[u8], loc: usize) -> Result<Self::Inner<'_>> {
                    const SIZE: usize = std::mem::size_of::<$ty>();
                    let end = loc.checked_add(SIZE).ok_or(Error::OutOfBounds)?;
                    let bytes: [u8; SIZE] = buf.get(loc..end)
                        .ok_or(Error::OutOfBounds)?
                        .try_into()
                        .unwrap();
                    Ok(<$ty>::from_le_bytes(bytes))
                }
            }
        )*
    };
}

// Expand for all core standard types
impl_follow_for_primitive!(u8, u16, u32, u64, u128, i8, i16, i32, i64, i128, f32, f64);

#[macro_export]
macro_rules! impl_follow_for_primitive_arr {
    ($($ty:ty),*) => {
        $(
            // Implemented on `&[$ty]` (Sized) rather than `[$ty]` so the type
            // composes into the strategy wrappers: `UOffset<&[u32]>` and
            // `NOffset<&[u32]>` need `T: Sized` for their `PhantomData<T>`.
            impl Follow for &[$ty] {
                /// Zero-copy on little-endian targets. The wire format is
                /// defined as LE, so big-endian targets must materialize,
                /// converting each element with `from_le_bytes`.
                #[cfg(target_endian = "little")]
                type Inner<'a> = &'a [$ty];
                #[cfg(not(target_endian = "little"))]
                type Inner<'a> = Vec<$ty>;

                #[inline(always)]
                fn follow<'a>(buf: &'a [u8], loc: usize) -> Result<Self::Inner<'a>> {
                    let len = u32::follow(buf, loc)? as usize;
                    let payload_start = loc + 4; // no overflow: u32::follow validated loc+4
                    let payload_size = len
                        .checked_mul(core::mem::size_of::<$ty>())
                        .ok_or(Error::OutOfBounds)?;
                    let payload_end = payload_start
                        .checked_add(payload_size)
                        .ok_or(Error::OutOfBounds)?;
                    let payload_bytes = buf.get(payload_start..payload_end)
                        .ok_or(Error::OutOfBounds)?;

                    #[cfg(target_endian = "little")]
                    {
                        // Positions inside the buffer are aligned by the writer,
                        // but the buffer's own base pointer is only guaranteed
                        // to be 1-aligned (it's a Vec<u8>) — and `follow` can be
                        // handed any subslice. The pointer must be checked, not
                        // assumed, or `from_raw_parts` is UB.
                        let ptr = payload_bytes.as_ptr();
                        if (ptr as usize) % core::mem::align_of::<$ty>() != 0 {
                            return Err(Error::Misaligned);
                        }
                        // SAFETY: bounds checked, alignment checked, all bit
                        // patterns valid for primitive numerics, borrow tied
                        // to the input lifetime.
                        Ok(unsafe { core::slice::from_raw_parts(ptr as *const $ty, len) })
                    }
                    #[cfg(not(target_endian = "little"))]
                    {
                        let mut out = Vec::with_capacity(len);
                        for chunk in payload_bytes.chunks_exact(core::mem::size_of::<$ty>()) {
                            out.push(<$ty>::from_le_bytes(chunk.try_into().unwrap()));
                        }
                        Ok(out)
                    }
                }
            }
        )*
    };
}

impl_follow_for_primitive_arr!(u8, u16, u32, u64, u128, i8, i16, i32, i64, i128, f32, f64);

// =========================================================================
// Innermost Implementations for Complex Payloads (Strings & Slices)
// =========================================================================

// Zero-copy String traversal (Reads u32 length prefix, then borrows bytes directly)
impl Follow for &str {
    // We explicitly name the lifetime 'a here
    type Inner<'a> = &'a str;

    #[inline(always)]
    fn follow<'a>(buf: &'a [u8], loc: usize) -> Result<Self::Inner<'a>> {
        let len = u32::follow(buf, loc)? as usize;
        let payload_start = loc + 4; // no overflow: u32::follow validated loc+4
        let payload_end = payload_start.checked_add(len).ok_or(Error::OutOfBounds)?;
        let byte_slice = buf.get(payload_start..payload_end)
            .ok_or(Error::OutOfBounds)?;

        // Validated by default: `from_utf8_unchecked` on wire input is UB
        // the moment a buffer arrives with invalid UTF-8. If profiling ever
        // shows validation matters, add a separate, explicitly-unsafe
        // `TrustedStr` follower rather than weakening the default.
        std::str::from_utf8(byte_slice).map_err(|_| Error::InvalidUtf8)
    }
}

/// Lazy zero-copy view over a serialized string array:
/// `[u32 count][u32 jump slots...][string records...]`.
/// Each slot is read through `NOffset<&str>` semantics: 0 = skipped entry.
#[derive(Copy, Clone)]
pub struct StrVector<'a> {
    buf: &'a [u8],
    table_start: usize,
    len: usize,
}

impl<'a> StrVector<'a> {
    #[inline(always)]
    pub const fn len(&self) -> usize { self.len }
    #[inline(always)]
    pub const fn is_empty(&self) -> bool { self.len == 0 }

    /// Skipped (empty) entries resolve to `""`.
    #[inline]
    pub fn get(&self, index: usize) -> Result<&'a str> {
        let slot = self.table_start + index * 4;
        let offset = u32::follow(self.buf, slot)? as usize;

        if offset == 0 {
            return Ok(""); // The sentinel check is here
        }

        // Now jump to the target
        let target = slot.checked_add(offset).ok_or(Error::OutOfBounds)?;
        Ok(<&str as Follow>::follow(self.buf, target)?)
    }

    pub fn iter(self) -> impl Iterator<Item = Result<&'a str>> {
        (0..self.len).map(move |i| self.get(i))
    }
}

impl Follow for StrVector<'_> {
    type Inner<'a> = StrVector<'a>;

    #[inline]
    fn follow(buf: &[u8], loc: usize) -> Result<StrVector<'_>> {
        let len = u32::follow(buf, loc)? as usize;
        let table_start = loc + 4; // no overflow: u32::follow validated loc+4
        let table_bytes = len.checked_mul(4).ok_or(Error::OutOfBounds)?;
        let table_end = table_start.checked_add(table_bytes).ok_or(Error::OutOfBounds)?;
        if table_end > buf.len() {
            return Err(Error::OutOfBounds);
        }
        Ok(StrVector { buf, table_start, len })
    }
}

#[cfg(test)]
mod follow {
    use super::*;
    use crate::buffers::bytes::RawBytes;
    use crate::buffers::vec::BufferVec;
use crate::buffers::vec::Error::InvalidUtf8;

    // ---- round trips against the real writer ------------------------------

    #[test]
    fn primitives_follow_written_records() {
        let mut buffer = BufferVec::with_size(1024, 1 << 20);
        let h_u32 = 0xDEADBEEFu32.into_buffer(&mut buffer).unwrap();
        let h_i64 = (-42i64).into_buffer(&mut buffer).unwrap();
        let h_f64 = std::f64::consts::PI.into_buffer(&mut buffer).unwrap();

        let sz = buffer.size();
        assert_eq!(u32::follow(&buffer.data, h_u32.get_access(sz)).unwrap(), 0xDEADBEEF);
        assert_eq!(i64::follow(&buffer.data, h_i64.get_access(sz)).unwrap(), -42);
        assert_eq!(f64::follow(&buffer.data, h_f64.get_access(sz)).unwrap(), std::f64::consts::PI);
    }

    #[test]
    fn str_follow_written_records() {
        let mut buffer = BufferVec::with_size(16, 1 << 20); // tiny: forces growth
        let cases = ["A", "unaligned_3", "a longer string crossing a growth boundary"];
        let handles: Vec<_> = cases.iter().map(|s| s.into_buffer(&mut buffer).unwrap()).collect();

        let sz = buffer.size();
        for (h, expected) in handles.iter().zip(cases.iter()) {
            let got = <&str>::follow(&buffer.data, h.get_access(sz)).unwrap();
            assert_eq!(got, *expected);
        }
    }

    #[test]
    fn num_array_follow_written_records() {
        let mut buffer = BufferVec::with_size(1024, 1 << 20);
        let data: &[u64] = &[u64::MAX, 0, 42];
        let h = data.into_buffer(&mut buffer).unwrap();
        let got = <&[u64]>::follow(&buffer.data, h.get_access(buffer.size())).unwrap();
        assert_eq!(got, data);
    }
    #[test]
    fn str_vector_follows_written_array_with_skips() {
        let mut buffer = BufferVec::with_size(4, 1 << 20);
        let corpus = ["", "first", "", "middle", "tail entry longer than most", ""];
        let h = corpus.as_slice().into_buffer(&mut buffer).unwrap();

        let v = StrVector::follow(&buffer.data, h.get_access(buffer.size())).unwrap();
        assert_eq!(v.len(), corpus.len());
        
        for (i, expected) in corpus.iter().enumerate() {
            // Your StrVector::get() now handles the 0 -> "" translation
            assert_eq!(v.get(i).unwrap(), *expected, "index {i}");
        }
        assert_eq!(v.get(corpus.len()).unwrap_err(), Error::OutOfBounds);

        // Testing UOffset directly (The "Strict" Pointer Chaser)
        let base = h.get_access(buffer.size());
        
        // Index 0 is a skip (0 offset). 
        // Since UOffset is strict, this should now be an error (or however you handle 0).
        // If you decided 0 = Error:
        assert!(UOffset::<&str>::follow(&buffer.data, base).is_err());
        
        // Index 1 is "first". 
        // This is the slot at base + 4.
        assert_eq!(UOffset::<&str>::follow(&buffer.data, base + 4).unwrap(), "first");
    }

    // ---- table / vtable mechanics -----------------------------------------

    /// Hand-crafted buffer defining the vtable convention:
    ///   pos 0:  vtable  [len=8][table_len=12][field0_off=4][field1_off=0]
    ///   pos 8:  table   [soffset=8 -> vtable at 0][field0: u32 @ table+4]
    fn craft_table_buffer() -> Vec<u8> {
        let mut buf = vec![0u8; 20];
        buf[0..2].copy_from_slice(&8u16.to_le_bytes());    // vtable byte len
        buf[2..4].copy_from_slice(&12u16.to_le_bytes());   // table byte len
        buf[4..6].copy_from_slice(&4u16.to_le_bytes());    // field 0 at table+4
        buf[6..8].copy_from_slice(&0u16.to_le_bytes());    // field 1 suppressed
        buf[8..12].copy_from_slice(&8i32.to_le_bytes());   // soffset: 8 - 8 = 0
        buf[12..16].copy_from_slice(&0xCAFEBABEu32.to_le_bytes()); // field 0 value
        buf
    }

    #[test]
    fn table_field_resolution() {
        let buf = craft_table_buffer();
        let t = Table::follow(&buf, 8).unwrap();

        // present field
        assert_eq!(t.get::<u32>(0).unwrap(), Some(0xCAFEBABE));
        // suppressed field (slot holds 0)
        assert_eq!(t.get::<u32>(1).unwrap(), None);
        // schema evolution: field id beyond this vtable -> None, not error
        assert_eq!(t.get::<u32>(7).unwrap(), None);
        // default substitution
        assert_eq!(t.get_or::<u32>(1, 777).unwrap(), 777);
        assert_eq!(t.get_or::<u32>(0, 777).unwrap(), 0xCAFEBABE);
    }

    #[test]
    fn table_rejects_malformed_vtables() {
        // soffset pointing before the buffer start
        let mut buf = craft_table_buffer();
        buf[8..12].copy_from_slice(&1000i32.to_le_bytes());
        assert_eq!(Table::follow(&buf, 8).unwrap_err(), Error::OutOfBounds);

        // vtable length shorter than its own header
        let mut buf = craft_table_buffer();
        buf[0..2].copy_from_slice(&2u16.to_le_bytes());
        assert_eq!(Table::follow(&buf, 8).unwrap_err(), Error::OutOfBounds);

        // vtable length running past the buffer
        let mut buf = craft_table_buffer();
        buf[0..2].copy_from_slice(&64u16.to_le_bytes());
        assert_eq!(Table::follow(&buf, 8).unwrap_err(), Error::OutOfBounds);
    }

    // ---- adversarial input: errors, never panics or UB ---------------------

    #[test]
    fn truncated_input_errors_not_panics() {
        let tiny = [0u8; 2];
        assert_eq!(UOffset::<u32>::follow(&tiny, 0).unwrap_err(), Error::OutOfBounds);
        assert_eq!(SOffset::<u32>::follow(&tiny, 0).unwrap_err(), Error::OutOfBounds);
        assert_eq!(UOffset::<&str>::follow(&tiny, 0).unwrap_err(), Error::OutOfBounds);
        assert_eq!(u64::follow(&tiny, 0).unwrap_err(), Error::OutOfBounds);
        assert_eq!(<&str>::follow(&tiny, 0).unwrap_err(), Error::OutOfBounds);
        assert_eq!(Table::follow(&tiny, 0).unwrap_err(), Error::OutOfBounds);

        // loc past the end entirely
        assert_eq!(u8::follow(&tiny, 5).unwrap_err(), Error::OutOfBounds);
    }

    #[test]
    fn hostile_lengths_and_offsets_error() {
        // string length prefix far past buffer end
        let mut evil = vec![0u8; 8];
        evil[0..4].copy_from_slice(&(1_000_000u32).to_le_bytes());
        assert_eq!(<&str>::follow(&evil, 0).unwrap_err(), Error::OutOfBounds);

        // array count * elem size overflowing usize must not wrap
        let mut wrap = vec![0u8; 8];
        wrap[0..4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(<&[u128]>::follow(&wrap, 0).unwrap_err(), Error::OutOfBounds);

        // SOffset whose subtraction would go negative
        let mut back = vec![0u8; 8];
        back[0..4].copy_from_slice(&100i32.to_le_bytes());
        assert_eq!(SOffset::<u32>::follow(&back, 0).unwrap_err(), Error::OutOfBounds);

        // invalid UTF-8 is an error, not UB
        let mut bad = vec![0u8; 8];
        bad[0..4].copy_from_slice(&2u32.to_le_bytes());
        bad[4] = 0xFF;
        bad[5] = 0xFE;
        assert_eq!(<&str>::follow(&bad, 0).unwrap_err(), InvalidUtf8);
    }

    #[test]
    fn misaligned_array_pointer_errors() {
        // Deterministically misaligned: back the bytes with a Vec<u64> so the
        // base pointer is 8-aligned, then place a u64 array record at loc 0 —
        // its payload starts at base+4, provably misaligned for u64.
        let mut backing: Vec<u64> = vec![0; 4];
        let bytes: &mut [u8] = unsafe {
            core::slice::from_raw_parts_mut(backing.as_mut_ptr() as *mut u8, 32)
        };
        bytes[0..4].copy_from_slice(&2u32.to_le_bytes()); // count = 2
        // payload would be bytes[4..20]

        let result = <&[u64]>::follow(bytes, 0);
        assert_eq!(result.unwrap_err(), Error::Misaligned);

        // Same data at loc 4: payload at base+8, correctly aligned
        bytes[4..8].copy_from_slice(&2u32.to_le_bytes());
        assert!(<&[u64]>::follow(bytes, 4).is_ok());
    }

    #[test]
    fn composed_wrappers() {
        // 1. UOffset<&str>: A forward u32 pointing at a string record.
        let mut buf = vec![0u8; 24];
        // Jump +8 -> record starts at 8
        buf[0..4].copy_from_slice(&8u32.to_le_bytes()); 
        // String length prefix at 8
        buf[8..12].copy_from_slice(&5u32.to_le_bytes()); 
        // String data at 12
        buf[12..17].copy_from_slice(b"hello"); 
        
        // Now returns "hello" directly, no Option needed
        assert_eq!(UOffset::<&str>::follow(&buf, 0).unwrap(), "hello");

        // 2. Nested: UOffset<UOffset<u32>>
        let mut buf = vec![0u8; 16];
        // Point 0 -> 4
        buf[0..4].copy_from_slice(&4u32.to_le_bytes());  
        // Point 4 -> 8
        buf[4..8].copy_from_slice(&4u32.to_le_bytes());  
        // Value at 8
        buf[8..12].copy_from_slice(&0xABCDu32.to_le_bytes());
        
        // Nested UOffset returns u32 directly, no Option needed
        assert_eq!(UOffset::<UOffset<u32>>::follow(&buf, 0).unwrap(), 0xABCD);
    }
}