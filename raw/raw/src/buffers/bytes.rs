use crate::buffers::vec::{BufferVec, Error, TailOffset};
use std::mem::{size_of, align_of};

pub type Result<T> = core::result::Result<T, Error>;

/// Calculates the exact number of padding bytes (0-3) needed to align 
/// any arbitrary size to the next 4-byte (32-bit) boundary.
#[inline(always)]
pub const fn padding_for(bytes: usize, alignment: usize) -> usize {
    let mask = alignment - 1;
    (!bytes).wrapping_add(1) & mask
}

/// Calculates the TOTAL size rounded up to the next alignment boundary.
#[inline(always)]
pub const fn align_up_to(bytes: usize, alignment: usize) -> usize {
    let mask = alignment - 1;
    (bytes + mask) & !mask
}
#[inline(always)]
pub const fn align_down_to(bytes: usize, alignment: usize) -> usize {
    let mask = alignment - 1;
    bytes & !mask
}

// =============================================================================
// UNIFIED CORE TRAIT
// =============================================================================

pub trait RawBytes {
    fn size(&self)-> usize;
    fn align(&self)-> usize;
    fn with_payload_slice<F, R>(&self, f: F) -> R where F: FnOnce(&[u8]) -> R;

    #[inline(always)]
    fn into_buffer(&self, buffer: &mut BufferVec) -> Result<TailOffset> {
        let size = self.size() as isize;
        let align = self.align() as isize;

        let mut target = (buffer.cursor as isize - size) & !(align - 1);
        if target < 0 {
            buffer.grow_by((-target) as usize)?;
            target = (buffer.cursor as isize - size) & !(align - 1);
        }
        let target = target as usize;

        self.with_payload_slice(|src| {
            buffer.data[target..target + size as usize].copy_from_slice(src);
        });
        buffer.cursor = target;
        Ok(buffer.handle())
    }

    /// Caller must guarantee `buffer.cursor >= self.size() + self.align() - 1`
    /// (i.e. `ensure_fit(SIZE_HINT)` has been called) before calling this.
    #[inline(always)]
    fn into_buffer_unchecked(&self, buffer: &mut BufferVec) -> Result<TailOffset> {
        let size = self.size();
        let align = self.align();
        let target = (buffer.cursor - size) & !(align - 1);
        self.with_payload_slice(|src| {
            buffer.data[target..target + size as usize].copy_from_slice(src);
        });
        buffer.cursor = target;
        Ok(buffer.handle())
    }
}

// =============================================================================
// STRINGS — skipped entirely when empty (TailOffset(0) sentinel)
// =============================================================================

impl RawBytes for str {
    /// Full record size (payload + 4-byte length header).
    /// Empty strings serialize to nothing: size 0, sentinel handle.
    #[inline(always)]
    fn size(&self) -> usize {
        let len = self.len();
        if len == 0 { 0 } else { len + 4 }
    }

    #[inline(always)]
    fn align(&self) -> usize {
        4
    }

    #[inline(always)]
    fn with_payload_slice<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R
    {
        f(self.as_bytes())
    }

    #[inline(always)]
    fn into_buffer(&self, buffer: &mut BufferVec) -> Result<TailOffset> {
        let len = self.len();
        if len == 0 { return Ok(TailOffset(0)) }; // Sentinel for None
        let size = (len + 4) as isize;

        // `size` already includes the header, so aligning the record start
        // down reserves everything — no extra -4.
        let calc = |cursor: isize| (cursor - size) & !3;
        let mut header_start = calc(buffer.cursor as isize);

        if header_start < 0 {
            buffer.grow_by((-header_start) as usize)?;
            header_start = calc(buffer.cursor as isize);
            debug_assert!(header_start >= 0);
        }

        let header_start = header_start as usize;
        let payload_start = header_start + 4;
        let payload_end = payload_start + len; // payload only — header written separately

        // Zero-copy stream writes
        buffer.data[payload_start..payload_end].copy_from_slice(self.as_bytes());
        buffer.data[header_start..payload_start]
            .copy_from_slice(&(len as u32).to_le_bytes());

        buffer.cursor = header_start;
        Ok(buffer.handle())
    }

    #[inline(always)]
    fn into_buffer_unchecked(&self, buffer: &mut BufferVec) -> Result<TailOffset> {
        let len = self.len();
        if len == 0 { return Ok(TailOffset(0)) }; // Sentinel for None
        let header = (buffer.cursor - (len + 4)) & !3;
        let target = header + 4;

        buffer.data[target..target + len].copy_from_slice(self.as_bytes());
        buffer.data[header..target].copy_from_slice(&(len as u32).to_le_bytes());

        buffer.cursor = header;
        Ok(buffer.handle())
    }
}

#[macro_export]
macro_rules! impl_raw_bytes_primitive {
    ($($ty:ty),*) => {
        $(
            impl RawBytes for $ty {
                #[inline(always)]
                fn size(&self) -> usize {
                    std::mem::size_of::<Self>()
                }

                #[inline(always)]
                fn align(&self) -> usize {
                    std::mem::align_of::<Self>()
                }
                
                #[inline(always)]
                fn with_payload_slice<F, R>(&self, f: F) -> R 
                where 
                    F: FnOnce(&[u8]) -> R 
                {
                    let bytes = self.to_le_bytes();
                    f(&bytes)        
                }
            }
        )*
    };
}

#[macro_export]
macro_rules! impl_raw_num_arr {
    ($($ty:ty),*) => {$(
        impl RawBytes for [$ty] {
            /// Payload + 4-byte length header, no padding.
            #[inline(always)]
            fn size(&self) -> usize {
                self.len() * size_of::<$ty>() + 4
            }

            #[inline(always)]
            fn align(&self) -> usize {
                align_of::<$ty>().max(4)
            }

            #[inline(always)]
            fn with_payload_slice<F, R>(&self, f: F) -> R
            where F: FnOnce(&[u8]) -> R {
                let payload = self.len() * size_of::<$ty>();
                if cfg!(target_endian = "little") {
                    let raw = unsafe {
                        std::slice::from_raw_parts(self.as_ptr() as *const u8, payload)
                    };
                    f(raw)
                } else {
                    let mut tmp = Vec::with_capacity(payload);
                    for item in self.iter() {
                        tmp.extend_from_slice(&item.to_le_bytes());
                    }
                    f(&tmp)
                }
            }

            #[inline(always)]
            fn into_buffer(&self, buffer: &mut BufferVec) -> Result<TailOffset> {
                let payload = (self.len() * size_of::<$ty>()) as isize;
                let align = self.align() as isize;
                let calc = |c: isize| ((c - payload) & !(align - 1)) - 4;

                let mut header_start = calc(buffer.cursor as isize);
                if header_start < 0 {
                    buffer.grow_by((-header_start) as usize)?;
                    header_start = calc(buffer.cursor as isize);
                    debug_assert!(header_start >= 0);
                }
                let header_start = header_start as usize;
                let payload_start = header_start + 4;
                let payload_end = payload_start + payload as usize;

                self.with_payload_slice(|src| {
                    buffer.data[payload_start..payload_end].copy_from_slice(src);
                });
                buffer.data[header_start..payload_start]
                    .copy_from_slice(&(self.len() as u32).to_le_bytes());
                buffer.cursor = header_start;
                Ok(buffer.handle())
            }

            /// Caller must guarantee capacity for `size()` plus worst-case
            /// alignment padding (`align() - 1`).
            #[inline(always)]
            fn into_buffer_unchecked(&self, buffer: &mut BufferVec) -> Result<TailOffset> {
                let payload = self.len() * size_of::<$ty>();
                let align = self.align();
                let header_start = ((buffer.cursor - payload) & !(align - 1)) - 4;
                let payload_start = header_start + 4;
                let payload_end = payload_start + payload;

                self.with_payload_slice(|src| {
                    buffer.data[payload_start..payload_end].copy_from_slice(src);
                });
                buffer.data[header_start..payload_start]
                    .copy_from_slice(&(self.len() as u32).to_le_bytes());
                buffer.cursor = header_start;
                Ok(buffer.handle())
            }
        }
    )*};
}

#[macro_export]
macro_rules! impl_raw_string_arr {
    ($($ty:ty),*) => {$(
        impl RawBytes for [$ty] {
            /// Packed string records + outer 4-byte count prefix, no external padding.
            /// Empty arrays serialize to nothing; empty entries occupy only
            /// their (zeroed) jump-table slot.
            #[inline(always)]
            fn size(&self) -> usize {
                let len = self.len();
                if len == 0 { return 0; }

                let table = len * 4 + 4;

                // Safe to unwrap because we already verified len != 0
                let (last, rest) = self.split_last().unwrap();

                let payload = rest
                    .iter()
                    .fold(0usize, |acc, s| acc + align_up_to(s.size(), 4))
                    + last.size();

                payload + table
            }

            #[inline(always)]
            fn align(&self) -> usize { 4 }

            #[inline(always)]
            fn with_payload_slice<F, R>(&self, _f: F) -> R
            where F: FnOnce(&[u8]) -> R {
                unimplemented!("string lists have no contiguous source payload; \
                                use into_buffer directly")
            }

            fn into_buffer(&self, buffer: &mut BufferVec) -> Result<TailOffset> {
                if self.len() == 0 { return Ok(TailOffset(0)) }; // Sentinel for None
                let (last, rest) = self.split_last().unwrap();

                let payload_size = rest
                    .iter()
                    .fold(0usize, |acc, s| acc + align_up_to(s.size(), 4))
                    + last.size();

                let table_size = self.len() * 4;
                let size = payload_size + table_size + 4;

                // Reserve total space up front — single growth check
                let calc = |c: isize| (c - size as isize) & !3;
                let mut block_start = calc(buffer.cursor as isize);
                if block_start < 0 {
                    buffer.grow_by((-block_start) as usize)?;
                    block_start = calc(buffer.cursor as isize)
                }
                let block_start = block_start as usize;
                let table_start = block_start + 4;
                let payload_start = table_start + table_size;
                // Fast linear table pointer tracking
                let mut table_cursor = payload_start;

                // Zero-overhead loop: straight memory streaming.
                // Empty strings write nothing and get a 0 jump slot.
                //
                // `into_buffer_unchecked` sets `buffer.cursor` to the record's
                // header_start on the non-sentinel path, and
                // `TailOffset::get_access(total)` is defined as `total - handle.0`
                // where `handle.0 = total - header_start` — the round trip
                // through TailOffset is an identity recovering `header_start`.
                // Reading `buffer.cursor` directly after the call gets the same
                // value without constructing or inverting a TailOffset, and
                // drops the per-loop `buffer_size` capture entirely.
                for s in self.iter().rev() {
                    let is_empty = s.into_buffer_unchecked(buffer)?.offset() == 0;
                    let table_slot = table_cursor - 4;
                    let jump = if is_empty { 0 } else {
                        (buffer.cursor - table_slot) as u32
                    };
                    buffer.data[table_slot..table_cursor]
                        .copy_from_slice(&jump.to_le_bytes());

                    table_cursor = table_slot;
                }

                buffer.cursor = table_cursor;
                (self.len() as u32).into_buffer_unchecked(buffer)?;

                Ok(buffer.handle())
            }

            /// Caller must guarantee `buffer.cursor >= self.size() + 3`
            /// (total block + worst-case entry alignment padding).
            fn into_buffer_unchecked(&self, buffer: &mut BufferVec) -> Result<TailOffset> {
                if self.len() == 0 { return Ok(TailOffset(0)) }; // Sentinel for None
                let (last, rest) = self.split_last().unwrap();

                let payload_size = rest
                    .iter()
                    .fold(0usize, |acc, s| acc + align_up_to(s.size(), 4))
                    + last.size();

                let table_size = self.len() * 4;
                let size = payload_size + table_size + 4;
                debug_assert!(buffer.cursor >= size + 3,
                    "string array into_buffer_unchecked: capacity precondition violated");

                let block_start = ((buffer.cursor as isize - size as isize) & !3) as usize;
                let table_start = block_start + 4;
                let payload_start = table_start + table_size;
                let mut table_cursor = payload_start;
                // Same identity as the checked path: `buffer.cursor` right
                // after a non-sentinel write already equals what the
                // TailOffset round trip would have recomputed.
                for s in self.iter().rev() {
                    let is_empty = s.into_buffer_unchecked(buffer)?.offset() == 0;
                    let table_slot = table_cursor - 4;
                    let jump = if is_empty { 0 } else {
                        (buffer.cursor - table_slot) as u32
                    };
                    buffer.data[table_slot..table_cursor]
                        .copy_from_slice(&jump.to_le_bytes());
                    table_cursor = table_slot;
                }

                buffer.cursor = table_cursor;
                (self.len() as u32).into_buffer_unchecked(buffer)?;
                Ok(buffer.handle())
            }
        }
    )*};
}

impl_raw_string_arr!(&str, String);
impl_raw_bytes_primitive!(i8,u8,u16, i16, u32, i32, f32, u64, i64, f64, i128, u128);
impl_raw_num_arr!(i8,u8,u16, i16, u32, i32, f32, u64, i64, f64, i128, u128);

#[cfg(test)]
mod skip_tests {
    use super::*;

    /// Helper to extract a u32 from an absolute buffer position
    fn read_u32_at(buffer: &BufferVec, pos: usize) -> u32 {
        u32::from_le_bytes(buffer.data[pos..pos + 4].try_into().unwrap())
    }

    #[test]
    fn empty_string_skipped_entirely() {
        let mut buffer = BufferVec::with_size(64, 1 << 20);
        let cursor_before = buffer.cursor;

        let h = "".into_buffer(&mut buffer).unwrap();
        assert_eq!(h.0, 0, "empty string must return the sentinel handle");
        assert_eq!(buffer.cursor, cursor_before, "empty string must write nothing");
        assert_eq!("".size(), 0);
    }

    #[test]
    fn empty_array_skipped_entirely() {
        let mut buffer = BufferVec::with_size(64, 1 << 20);
        let cursor_before = buffer.cursor;

        let empty: &[&str] = &[];
        let h = empty.into_buffer(&mut buffer).unwrap();
        assert_eq!(h.0, 0, "empty array must return the sentinel handle");
        assert_eq!(buffer.cursor, cursor_before, "empty array must write nothing");
        assert_eq!(empty.size(), 0);
    }

    #[test]
    fn nonempty_string_still_intact() {
        // Regression for the size-includes-header refactor: payload length,
        // header value, and consumed space must all use `len`, not `size`.
        let mut buffer = BufferVec::with_size(64, 1 << 20);
        let h = "hello".into_buffer(&mut buffer).unwrap();
        let pos = h.get_access(buffer.size());

        assert_eq!(read_u32_at(&buffer, pos), 5, "header must be payload len");
        assert_eq!(&buffer.data[pos + 4..pos + 9], b"hello");
        assert_eq!((pos + 4) % 4, 0, "payload must be 4-aligned");
    }

    #[test]
    fn array_with_empty_entries_zero_jump_slots() {
        let mut buffer = BufferVec::with_size(4, 1 << 20); // tiny: forces growth
        let corpus = ["", "first", "", "middle", "", "", "tail entry longer than most", ""];

        let h = corpus.as_slice().into_buffer(&mut buffer).unwrap();
        let pos = h.get_access(buffer.size());

        assert_eq!(read_u32_at(&buffer, pos) as usize, corpus.len(), "count prefix");

        let table_start = pos + 4;
        for (i, s) in corpus.iter().enumerate() {
            let slot = table_start + i * 4;
            let jump = read_u32_at(&buffer, slot) as usize;
            if s.is_empty() {
                assert_eq!(jump, 0, "slot {i} should be the skip sentinel");
            } else {
                assert!(jump >= 4, "slot {i} jump must clear the table");
                let target = slot + jump;
                let len = read_u32_at(&buffer, target) as usize;
                assert_eq!(len, s.len(), "slot {i} target length");
                assert_eq!(&buffer.data[target + 4..target + 4 + len], s.as_bytes());
                assert_eq!((target + 4) % 4, 0, "slot {i} payload alignment");
            }
        }
    }

    #[test]
    fn all_empty_array_still_roundtrips_count() {
        // An array OF empties is not an empty array: count must survive.
        let mut buffer = BufferVec::with_size(64, 1 << 20);
        let corpus = ["", "", ""];
        let h = corpus.as_slice().into_buffer(&mut buffer).unwrap();
        assert_ne!(h.0, 0, "non-empty array of empty strings is still present");

        let pos = h.get_access(buffer.size());
        assert_eq!(read_u32_at(&buffer, pos), 3);
        for i in 0..3 {
            assert_eq!(read_u32_at(&buffer, pos + 4 + i * 4), 0);
        }
    }

    #[test]
    fn unchecked_array_path_matches_checked() {
        // The unchecked path must produce identical skip semantics.
        let corpus = ["alpha", "", "gamma", ""];
        let needed = corpus.as_slice().size() + 3;

        let mut checked = BufferVec::with_size(1024, 1 << 20);
        let h_c = corpus.as_slice().into_buffer(&mut checked).unwrap();

        let mut unchecked = BufferVec::with_size(1024, 1 << 20);
        unchecked.ensure_fit(needed).unwrap();
        let h_u = corpus.as_slice().into_buffer_unchecked(&mut unchecked).unwrap();

        // NOTE: we deliberately compare only the DEFINED bytes (count, jump
        // slots, string records) — the alignment padding gaps between records
        // are currently uninitialized memory and differ between allocations.
        // Comparing raw block bytes here was flaky for exactly that reason;
        // padding should eventually be zeroed for deterministic buffers.
        let pc = h_c.get_access(checked.size());
        let pu = h_u.get_access(unchecked.size());

        let count_c = read_u32_at(&checked, pc);
        let count_u = read_u32_at(&unchecked, pu);
        assert_eq!(count_c, count_u, "count prefix diverges");

        for i in 0..corpus.len() {
            let slot_c = pc + 4 + i * 4;
            let slot_u = pu + 4 + i * 4;
            let jump_c = read_u32_at(&checked, slot_c) as usize;
            let jump_u = read_u32_at(&unchecked, slot_u) as usize;
            assert_eq!(jump_c, jump_u, "jump slot {i} diverges");
            if jump_c == 0 {
                assert!(corpus[i].is_empty(), "slot {i}: 0 jump for non-empty entry");
                continue;
            }
            let rec_c = slot_c + jump_c;
            let rec_u = slot_u + jump_u;
            let len = corpus[i].len();
            assert_eq!(
                &checked.data[rec_c..rec_c + 4 + len],
                &unchecked.data[rec_u..rec_u + 4 + len],
                "record {i} diverges"
            );
        }
    }

    #[test]
    fn skipping_after_odd_cursor_alignment() {
        // A u8 write leaves the cursor unaligned; the array block reservation
        // and per-string alignment slack must still agree.
        let mut buffer = BufferVec::with_size(1024, 1 << 20);
        let _ = 7u8.into_buffer(&mut buffer).unwrap();

        let corpus = ["odd", "", "alignment", "paths", ""];
        let h = corpus.as_slice().into_buffer(&mut buffer).unwrap();
        let pos = h.get_access(buffer.size());
        assert_eq!(read_u32_at(&buffer, pos) as usize, corpus.len());

        let table_start = pos + 4;
        for (i, s) in corpus.iter().enumerate() {
            let slot = table_start + i * 4;
            let jump = read_u32_at(&buffer, slot) as usize;
            if s.is_empty() {
                assert_eq!(jump, 0);
            } else {
                let target = slot + jump;
                let len = read_u32_at(&buffer, target) as usize;
                assert_eq!(len, s.len(), "index {i}");
                assert_eq!(&buffer.data[target + 4..target + 4 + len], s.as_bytes());
            }
        }
    }
}

#[cfg(test)]
mod bench {
    use super::*;
    use std::hint::black_box;
    use std::time::Instant;

    const N: usize = 1_000_000;

    /// Generic hot-path bench over any RawBytes type.
    fn bench_hot<T, F>(name: &str, mut make: F)
    where
        T: RawBytes,
        F: FnMut(u64) -> T,
    {
        let mut buffer = BufferVec::with_size(4096, 1 << 20);

        // warmup
        for i in 0..1000u64 {
            black_box(make(i)).into_buffer(black_box(&mut buffer)).unwrap();
        }
        buffer.reset();

        let start = Instant::now();
        for i in 0..N as u64 {
            let v = black_box(make(i));
            let off = v.into_buffer(black_box(&mut buffer)).unwrap();
            black_box(off);
            black_box(buffer.cursor);
            if buffer.cursor < 64 {
                buffer.reset();
            }
        }
        let elapsed = start.elapsed();
        println!(
            "  {:<10} {:>9} iters in {:>12?}  ({:>5.2} ns/iter)",
            name, N, elapsed,
            elapsed.as_nanos() as f64 / N as f64
        );
    }

    /// Hot-path bench for numeric arrays — variations loop through a pre-built 
    /// slice matrix to eliminate vector allocations during the measurement loop.
    fn bench_hot_array<'a, T>(name: &str, arrays: &[&'a [T]], reset_below: usize, iters: usize, elem_count: usize)
    where
        [T]: RawBytes,
    {
        let mut buffer = BufferVec::with_size(8192, 1 << 22);

        // warmup
        for arr in arrays.iter().cycle().take(1000) {
            black_box(*arr).into_buffer(black_box(&mut buffer)).unwrap();
        }
        buffer.reset();

        let start = Instant::now();
        let mut idx = 0usize;
        for _ in 0..iters {
            let arr = black_box(arrays[idx]);
            idx = (idx + 1) % arrays.len();
            let off = arr.into_buffer(black_box(&mut buffer)).unwrap();
            black_box(off);
            black_box(buffer.cursor);
            if buffer.cursor < reset_below {
                buffer.reset();
            }
        }
        let elapsed = start.elapsed();
        let ns_per_iter = elapsed.as_nanos() as f64 / iters as f64;
        
        if elem_count > 0 {
            let ns_per_elem = ns_per_iter / elem_count as f64;
            println!(
                "  {:<13} {:>8} iters in {:>12?}  ({:>5.2} ns/iter, {:>5.2} ns/elem)",
                name, iters, elapsed, ns_per_iter, ns_per_elem
            );
        } else {
            println!(
                "  {:<13} {:>8} iters in {:>12?}  ({:>5.2} ns/iter)",
                name, iters, elapsed, ns_per_iter
            );
        }
    }

    /// Hot-path bench for strings — pre-built corpus so string construction
    /// isn't measured, varied lengths so alignment paths all get exercised.
    fn bench_hot_str(name: &str, strings: &[&str], reset_below: usize) {
        let mut buffer = BufferVec::with_size(8192, 1 << 22);

        for s in strings.iter().cycle().take(1000) {
            black_box(*s).into_buffer(black_box(&mut buffer)).unwrap();
        }
        buffer.reset();

        let start = Instant::now();
        let mut idx = 0usize;
        for _ in 0..N {
            let s = black_box(strings[idx]);
            idx = (idx + 1) % strings.len();
            let off = s.into_buffer(black_box(&mut buffer)).unwrap();
            black_box(off);
            black_box(buffer.cursor);
            if buffer.cursor < reset_below {
                buffer.reset();
            }
        }
        let elapsed = start.elapsed();
        println!(
            "  {:<10} {:>9} iters in {:>12?}  ({:>5.2} ns/iter)",
            name, N, elapsed,
            elapsed.as_nanos() as f64 / N as f64
        );
    }

    #[test]
    fn bench_into_buffer_hot_all_types() {
        println!("\n=== into_buffer hot path (reused buffer) ===");
        bench_hot::<u8, _>("u8", |i| (i % 256) as u8);
        bench_hot::<i8, _>("i8", |i| (i % 128) as i8);
        bench_hot::<u16, _>("u16", |i| i as u16);
        bench_hot::<i16, _>("i16", |i| i as i16);
        bench_hot::<u32, _>("u32", |i| i as u32);
        bench_hot::<i32, _>("i32", |i| i as i32);
        bench_hot::<f32, _>("f32", |i| i as f32);
        bench_hot::<u64, _>("u64", |i| i);
        bench_hot::<i64, _>("i64", |i| i as i64);
        bench_hot::<f64, _>("f64", |i| i as f64);
        bench_hot::<u128, _>("u128", |i| i as u128);
        bench_hot::<i128, _>("i128", |i| i as i128);
    }

    #[test]
    fn bench_into_buffer_hot_arrays() {
        println!("\n=== into_buffer hot path — arrays (reused buffer) ===");
        let array_iters = 100_000;

        // 1. &[u32] x4
        let u32_x4_data: Vec<Vec<u32>> = vec![vec![1, 2, 3, 4]];
        let u32_x4_slices: Vec<&[u32]> = u32_x4_data.iter().map(|v| v.as_slice()).collect();
        bench_hot_array::<u32>("&[u32] x4", &u32_x4_slices, 128, array_iters, 4);

        // 2. &[u32] x128
        let u32_x128_data: Vec<Vec<u32>> = vec![(0..128).collect()];
        let u32_x128_slices: Vec<&[u32]> = u32_x128_data.iter().map(|v| v.as_slice()).collect();
        bench_hot_array::<u32>("&[u32] x128", &u32_x128_slices, 1024, array_iters, 128);

        // 3. &[f64] x8
        let f64_x8_data: Vec<Vec<f64>> = vec![vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]];
        let f64_x8_slices: Vec<&[f64]> = f64_x8_data.iter().map(|v| v.as_slice()).collect();
        bench_hot_array::<f64>("&[f64] x8", &f64_x8_slices, 256, array_iters, 8);

        // 4. &[&str] x2
        println!("\n=== into_buffer hot path — string arrays ===");
        let str_x2_corpus = [["hello", "world!"].as_slice()];
        bench_hot_array::<&str>("&[&str] x2", &str_x2_corpus, 256, array_iters, 2);

        // 5. &[&str] x8
        let str_x8_corpus = [["a", "b", "c", "d", "e", "f", "g", "h"].as_slice()];
        bench_hot_array::<&str>("&[&str] x8", &str_x8_corpus, 512, array_iters, 8);
    }

    #[test]
    fn bench_into_buffer_hot_strings() {
        println!("\n=== into_buffer hot path — strings (reused buffer) ===");

        let short: Vec<&str> = vec!["hello", "world!", "seven77", "eight888"];
        let medium: Vec<&str> = vec![
            "the quick brown fox jumps ov",
            "a somewhat longer string here!!",
            "SKU-000042-PRODUCT-LABEL-EXAMPLE",
        ];
        let long_owned: String = "x".repeat(256);
        let long: Vec<&str> = vec![long_owned.as_str()];

        bench_hot_str("str~7B", &short, 64);
        bench_hot_str("str~32B", &medium, 128);
        bench_hot_str("str~256B", &long, 512);
    }

    /// Correctness: adjacent strings must not clobber each other's headers,
    /// and every handle must resolve to an intact [u32 len][utf8] record.
    #[test]
    fn string_round_trip_adjacent() {
        let cases = ["hello", "world!!", "", "x", "exactly8", "a longer string to cross a growth boundary maybe"];
        let mut buffer = BufferVec::with_size(4, 1 << 20); // tiny: forces growth mid-sequence
        let handles: Vec<TailOffset> = cases
            .iter()
            .map(|s| s.into_buffer(&mut buffer).unwrap())
            .collect();

        for (h, expected) in handles.iter().zip(cases.iter()) {
            if expected.is_empty() {
                assert_eq!(h.0, 0, "empty string must yield the sentinel handle");
                continue;
            }
            let pos = h.get_access(buffer.size());
            let len = u32::from_le_bytes(buffer.data[pos..pos + 4].try_into().unwrap()) as usize;
            assert_eq!(len, expected.len(), "length prefix mismatch for {expected:?}");
            let s = std::str::from_utf8(&buffer.data[pos + 4..pos + 4 + len]).unwrap();
            assert_eq!(s, *expected, "payload mismatch");
            assert_eq!((pos + 4) % 4, 0, "payload start not 4-aligned for {expected:?}");
        }
    }

    /// Correctness: verification that numeric primitive arrays correctly preserve 
    /// length prefixes, satisfy internal alignment rules, and can be read back cleanly.
    #[test]
    fn array_round_trip_adjacent() {
        let mut buffer = BufferVec::with_size(4, 1 << 20);
        
        let arr_u8: &[u8] = &[10, 20, 30, 40, 50];
        let arr_u32: &[u32] = &[1000, 2000, 3000];
        let arr_u64: &[u64] = &[8888, 9999];

        let h_u8 = arr_u8.into_buffer(&mut buffer).unwrap();
        let h_u32 = arr_u32.into_buffer(&mut buffer).unwrap();
        let h_u64 = arr_u64.into_buffer(&mut buffer).unwrap();

        let size = buffer.size();

        // 1. Validate [u64] array
        let pos_u64 = h_u64.get_access(size);
        let len_u64 = u32::from_le_bytes(buffer.data[pos_u64..pos_u64 + 4].try_into().unwrap()) as usize;
        assert_eq!(len_u64, arr_u64.len());
        let payload_pos_u64 = pos_u64 + 4;
        assert_eq!(payload_pos_u64 % std::mem::align_of::<u64>(), 0, "u64 array alignment failure");
        for (i, expected) in arr_u64.iter().enumerate() {
            let p = payload_pos_u64 + i * 8;
            let v = u64::from_le_bytes(buffer.data[p..p + 8].try_into().unwrap());
            assert_eq!(v, *expected);
        }

        // 2. Validate [u32] array
        let pos_u32 = h_u32.get_access(size);
        let len_u32 = u32::from_le_bytes(buffer.data[pos_u32..pos_u32 + 4].try_into().unwrap()) as usize;
        assert_eq!(len_u32, arr_u32.len());
        let payload_pos_u32 = pos_u32 + 4;
        assert_eq!(payload_pos_u32 % std::mem::align_of::<u32>(), 0, "u32 array alignment failure");
        for (i, expected) in arr_u32.iter().enumerate() {
            let p = payload_pos_u32 + i * 4;
            let v = u32::from_le_bytes(buffer.data[p..p + 4].try_into().unwrap());
            assert_eq!(v, *expected);
        }

        // 3. Validate [u8] array
        let pos_u8 = h_u8.get_access(size);
        let len_u8 = u32::from_le_bytes(buffer.data[pos_u8..pos_u8 + 4].try_into().unwrap()) as usize;
        assert_eq!(len_u8, arr_u8.len());
        let payload_pos_u8 = pos_u8 + 4;
        assert_eq!(&buffer.data[payload_pos_u8..payload_pos_u8 + len_u8], arr_u8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to extract a u32 length prefix from an absolute buffer position
    fn read_len_at(buffer: &BufferVec, pos: usize) -> usize {
        u32::from_le_bytes(buffer.data[pos..pos + 4].try_into().unwrap()) as usize
    }

    #[test]
    fn test_primitives_round_trip() {
        let mut buffer = BufferVec::with_size(8192, 1 << 22);

        // Core numeric primitives
        let val_u8: u8 = 255;
        let val_u32: u32 = 0xDEADBEEF;
        let val_u64: u64 = 0xCAFEBABE12345678;
        let val_f64: f64 = 3.141592653589793;
        let val_i128: i128 = -123456789012345678901234567890;

        // Serialize primitives sequentially
        let h_i128 = val_i128.into_buffer(&mut buffer).unwrap();
        let h_f64 = val_f64.into_buffer(&mut buffer).unwrap();
        let h_u64 = val_u64.into_buffer(&mut buffer).unwrap();
        let h_u32 = val_u32.into_buffer(&mut buffer).unwrap();
        let h_u8 = val_u8.into_buffer(&mut buffer).unwrap();

        let size = buffer.size();

        // 1. Verify i128
        let pos = h_i128.get_access(size);
        assert_eq!(pos % 4, 0, "i128 structural alignment broken");
        let read_i128 = i128::from_le_bytes(buffer.data[pos..pos + 16].try_into().unwrap());
        assert_eq!(read_i128, val_i128);

        // 2. Verify f64
        let pos = h_f64.get_access(size);
        assert_eq!(pos % 4, 0, "f64 structural alignment broken");
        let read_f64 = f64::from_le_bytes(buffer.data[pos..pos + 8].try_into().unwrap());
        assert_eq!(read_f64, val_f64);

        // 3. Verify u64
        let pos = h_u64.get_access(size);
        assert_eq!(pos % 4, 0, "u64 structural alignment broken");
        let read_u64 = u64::from_le_bytes(buffer.data[pos..pos + 8].try_into().unwrap());
        assert_eq!(read_u64, val_u64);

        // 4. Verify u32
        let pos = h_u32.get_access(size);
        assert_eq!(pos % 4, 0, "u32 structural alignment broken");
        let read_u32 = u32::from_le_bytes(buffer.data[pos..pos + 4].try_into().unwrap());
        assert_eq!(read_u32, val_u32);

        // 5. Verify u8
        let pos = h_u8.get_access(size);
        let read_u8 = buffer.data[pos];
        assert_eq!(read_u8, val_u8);
    }

    #[test]
    fn test_string_edge_cases_and_alignment() {
        let mut buffer = BufferVec::with_size(1024, 1 << 20);

        // Testing unaligned string lengths to force internal 4-byte structural padding
        let inputs = vec!["", "A", "AB", "ABC", "ABCD", "An unaligned sentence structural test string!"];
        
        let handles: Vec<_> = inputs.iter()
            .map(|s| s.into_buffer(&mut buffer).unwrap())
            .collect();

        let size = buffer.size();

        for (h, expected) in handles.iter().zip(inputs.iter()) {
            if expected.is_empty() {
                assert_eq!(h.0, 0, "empty string must yield the sentinel handle");
                continue;
            }
            let pos = h.get_access(size);
            
            // Outer handle entry must always sit at a 4-byte legal boundary
            assert_eq!(pos % 4, 0, "String record start unaligned for input {:?}", expected);
            
            let len = read_len_at(&buffer, pos);
            assert_eq!(len, expected.len(), "String length header corrupted");

            let payload_start = pos + 4;
            let payload_bytes = &buffer.data[payload_start..payload_start + len];
            let parsed_str = std::str::from_utf8(payload_bytes).unwrap();
            assert_eq!(parsed_str, *expected, "String data payload mismatch");
        }
    }

    #[test]
    fn test_string_array_layout_and_jump_table() {
        let mut buffer = BufferVec::with_size(4096, 1 << 22);

        // Mix string shapes completely: empty string, unaligned length, long string
        let corpus = vec![
            "first_item", 
            "", 
            "unaligned_3", 
            "aligned_abcd", 
            "last_item_in_the_test_matrix"
        ];
        
        let h_arr = corpus.as_slice().into_buffer(&mut buffer).unwrap();
        let size = buffer.size();
        let base_pos = h_arr.get_access(size);

        // 1. Verify the array metadata prefix count
        let array_len = read_len_at(&buffer, base_pos);
        assert_eq!(array_len, corpus.len(), "Array outer element counter mismatch");

        // 2. Step through the jump table left-to-right (0 to N)
        let table_start = base_pos + 4;
        for i in 0..array_len {
            let slot_pos = table_start + (i * 4);
            let jump_offset = read_len_at(&buffer, slot_pos);

            // Skipped (empty) entries carry a 0 jump and no record
            if corpus[i].is_empty() {
                assert_eq!(jump_offset, 0, "Empty entry at index {} must have a 0 slot", i);
                continue;
            }
            
            // Resolve payload position using your forward positive offset rule
            let target_payload_pos = slot_pos + jump_offset;
            
            // Check target record structure
            let string_len = read_len_at(&buffer, target_payload_pos);
            assert_eq!(string_len, corpus[i].len(), "Jump table slot index {} points to wrong string length", i);

            let string_data = &buffer.data[target_payload_pos + 4..target_payload_pos + 4 + string_len];
            assert_eq!(std::str::from_utf8(string_data).unwrap(), corpus[i], "Jump table slot index {} payload broken", i);
        }
    }

    #[test]
    fn test_numeric_array_structural_integrity() {
        let mut buffer = BufferVec::with_size(2048, 1 << 20);

        let data_u32: &[u32] = &[100, 200, 300, 400, 500];
        let h_u32 = data_u32.into_buffer(&mut buffer).unwrap();

        let size = buffer.size();
        let pos_u32 = h_u32.get_access(size);

        // Validate outer raw slice descriptor
        let len_elements = read_len_at(&buffer, pos_u32);
        assert_eq!(len_elements, data_u32.len());

        let payload_start = pos_u32 + 4;
        assert_eq!(payload_start % std::mem::align_of::<u32>(), 0, "Primitive array alignment validation failed");

        for (i, expected) in data_u32.iter().enumerate() {
            let p = payload_start + i * 4;
            let v = u32::from_le_bytes(buffer.data[p..p + 4].try_into().unwrap());
            assert_eq!(v, *expected);
        }
    }

    #[test]
    fn test_buffer_auto_growth_mid_sequence() {
        // Start with a microscopic initial size to guarantee growth triggers 
        // in the middle of serializing types
        let mut buffer = BufferVec::with_size(16, 1 << 20);

        let long_str = "This string is significantly longer than the initial capacity of the tiny testing buffer vec buffer block.";
        let trailing_val: u64 = 0xAAFFFFFFFFFFAA;

        let h_str = long_str.into_buffer(&mut buffer).unwrap();
        let h_val = trailing_val.into_buffer(&mut buffer).unwrap();

        let size = buffer.size();

        // Verify data was correctly re-anchored and readable after allocations shifted
        let pos_val = h_val.get_access(size);
        let read_val = u64::from_le_bytes(buffer.data[pos_val..pos_val + 8].try_into().unwrap());
        assert_eq!(read_val, trailing_val);

        let pos_str = h_str.get_access(size);
        let read_len = read_len_at(&buffer, pos_str);
        assert_eq!(read_len, long_str.len());
        let read_str = std::str::from_utf8(&buffer.data[pos_str + 4..pos_str + 4 + read_len]).unwrap();
        assert_eq!(read_str, long_str);
    }
}