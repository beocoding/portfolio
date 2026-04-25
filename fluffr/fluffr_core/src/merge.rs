use crate::buffer::Buffer;
use crate::read::{HasRawView, ListView, ReadAt};
use crate::serialize::{Serialize, SerializeBytes};

/// Merge a `List(Inline)` field — packed scalar or `#[repr(C)]` struct arrays.
///
/// # Algorithm
///
/// Pre-allocates one contiguous aligned region for all elements from all views
/// combined, then fills it view-by-view in **reverse** order so that `self`
/// (the last view) lands at the lowest address and is therefore read first
/// when iterating.
///
/// Correctness of the single-allocation approach: for any valid `Pod` type T,
/// `size_of::<T>()` is a multiple of `align_of::<T>()`.  Therefore
/// `N * size_of::<T>()` is always aligned for T, meaning consecutive blocks
/// of different views remain contiguous with no padding gaps between them.
///
/// # Returns
///
/// The slot of the length-prefix u32, or 0 if all views were empty.
#[inline]
pub fn merge_inline_list<'a, T, V, B, F>(
    views:      &'a [V],
    out:        &mut B,
    elem_size:  usize,  // size_of::<ElemType>() — avoids a monomorphization per element type
    align_mask: usize,  // align_of::<ElemType>().max(4) - 1
    get_list:   F,      // extracts the field's ListView from a view
) -> usize
where
    B: Buffer,
    T: ReadAt<'a>,
    F: Fn(&'a V) -> ListView<'a, T>,
{
    let total_bytes: usize = views.iter()
        .map(|v| get_list(v).len() * elem_size)
        .sum();
    if total_bytes == 0 { return 0; }

    out.ensure_capacity(total_bytes + align_mask);
    *out.head_mut() -= total_bytes;
    *out.head_mut() &= !align_mask;
    let base = out.head();
    let mut pos = base;
    // Iterate views in reverse so the last view (self) lands at base.
    // ListView reads forward from base, so self's elements appear first.
    for v in views.iter().rev() {
        let list = get_list(v);
        if list.is_empty() { continue; }
        let block = list.len() * elem_size;
        out.buffer_mut()[pos..pos + block]
            .copy_from_slice(&list.buf[list.offset..list.offset + block]);
        pos += block;
    }
    let total_len = (total_bytes / elem_size) as u32;
    total_len.write_to_unchecked(out)
}

/// Merge a `List(String)` field using two-pass block copying.
///
/// # Layout of a string list in the source buffer
///
/// ```text
/// [jump_0][jump_1]...[jump_n-1]   ← jump table: n × u32, each a forward offset
/// [len_0][bytes_0][len_1][bytes_1] ← string pool: length-prefixed UTF-8 blobs
/// ```
///
/// The jump table and string pool are contiguous but at different addresses,
/// and the jump values are position-relative — they must be recomputed when
/// moved to the new buffer.
///
/// # Algorithm
///
/// **Pass A** — for each view, block-copy the string pool (everything from
/// `offset + n*4` to `last_offset + 4 + last_string_len`) as a single
/// `copy_from_slice`.  Records the destination base address (`dest_pool_start`)
/// for each view so Pass B can compute the fixup delta.
///
/// **Pass B** — for each view, block-copy the jump table, then add a constant
/// delta to every entry.  The delta collapses to:
///
/// ```text
/// delta = dest_pool_start - out.head()
/// ```
///
/// measured before subtracting the jump-table size.  This works because both
/// the per-entry movement (`dest_h - src_h_entry`) and the per-string movement
/// (`dest_pool - src_pool`) are factored out identically across all entries
/// in the same view.
///
/// # Returns
///
/// The slot of the length-prefix u32, or 0 if all views were empty.
#[inline]
pub fn merge_string_list<'a, V, B, F>(
    views:    &'a [V],
    out:      &mut B,
    get_list: F,
) -> usize
where
    B: Buffer,
    F: Fn(&'a V) -> ListView<'a, String>,
{
    use crate::read::ReadAt as _;
    let mut pool_starts: Vec<usize> = Vec::with_capacity(views.len());
    let mut total_len = 0u32;

    // Pass A: copy each view's contiguous string pool as one memcpy.
    for v in views.iter() {
        let list = get_list(v);
        if list.is_empty() { pool_starts.push(0); continue; }
        let n          = list.len();
        // Jump table occupies the first n*4 bytes starting at list.offset.
        // The string pool starts immediately after.
        let pool_start = list.offset + n * 4;
        let last       = list.last_offset();
        // last_offset gives the absolute position of the last string's length
        // prefix; adding 4 + the length gives the exclusive end of the pool.
        let pool_end   = last + 4 + u32::read_at(list.buf, last) as usize;
        let pool_size  = pool_end - pool_start;
        out.ensure_capacity(pool_size + 3);
        *out.head_mut() -= pool_size;
        *out.head_mut() &= !3;
        let h = out.head();
        out.buffer_mut()[h..h + pool_size]
            .copy_from_slice(&list.buf[pool_start..pool_end]);
        pool_starts.push(h);
        total_len += n as u32;
    }

    // Pass B: copy each view's jump table and fixup every entry with a
    // per-view constant delta.
    for (v, &dest_pool_start) in views.iter().zip(pool_starts.iter()) {
        let list = get_list(v);
        if list.is_empty() { continue; }
        let n         = list.len();
        let jump_size = n * 4;
        // Delta measured before decrementing head so the algebra is consistent.
        let delta     = dest_pool_start as i64 - out.head() as i64;
        out.ensure_capacity(jump_size + 3);
        *out.head_mut() -= jump_size;
        *out.head_mut() &= !3;
        let dst_h = out.head();
        // Bulk-copy the raw jump table bytes, then fixup in place.
        out.buffer_mut()[dst_h..dst_h + jump_size]
            .copy_from_slice(&list.buf[list.offset..list.offset + jump_size]);
        for j in 0..n {
            let p = dst_h + j * 4;
            let v = u32::read_at(out.buffer(), p) as i64;
            out.buffer_mut()[p..p + 4]
                .copy_from_slice(&((v + delta) as u32).to_le_bytes());
        }
    }

    if total_len == 0 { 0 } else { total_len.write_to_unchecked(out) }
}

/// Merge a `List(Table)` field using three-pass block copying.
///
/// # Layout of a table element in the source buffer
///
/// ```text
/// [vtable_jump: i32][field_0][field_1]...  ← table object at t_pos
/// [vtable_size: u16][object_size: u16][voff_0: u16]...  ← vtable at v_pos
/// [nested payload (strings, arrays, nested tables)]
/// ```
///
/// The entire block from `t_pos` to `block_end()` is self-contained: all
/// internal offsets are relative to positions within the block and survive
/// a verbatim `copy_from_slice`.  The **only** external reference is the
/// vtable jump at `t_pos`, which points outside the block to a (potentially
/// shared) vtable.
///
/// # Algorithm
///
/// **Pass A** — scan all elements across all views, write each unique vtable
/// exactly once into the new buffer.  In practice all elements of the same
/// list type share one vtable, so this is typically one write total.  Builds
/// a `vtable_bytes → slot` map for use in Pass B.
///
/// **Pass B** — for each element: `copy_from_slice` the block, then rewrite
/// the vtable jump at byte 0 of the block:
///
/// ```text
/// new_jump = dst as i64 - (out.len() - vt_slot) as i64
/// ```
///
/// Slots are invariant under `grow()`, so `out.len() - vt_slot` always gives
/// the correct absolute position of the vtable in the post-grow buffer.
///
/// Write order is: views forward, elements reversed within each view — this
/// matches the general serialization path so `ListView` read order is identical.
///
/// **Pass C** — write the forward-offset table (n × u32) that `ListView` uses
/// to locate each element.
///
/// # Nested tables
///
/// Any nested table fields whose data lives inside the block (reachable via a
/// forward offset from the table object) are also copied verbatim.  Their
/// internal offsets are self-relative and survive the copy.  If a nested table
/// has its own vtable jump pointing *outside* the block, an additional fixup
/// pass would be needed — the current schema (BrandData, ImageFile, Tags) has
/// no such fields, so one fixup at `t_pos` suffices.
///
/// # Returns
///
/// The slot of the length-prefix u32, or 0 if all views were empty.
#[inline]
pub fn merge_table_list<'a, T, V, B, FA, FB>(
    views:         &'a [V],
    out:           &mut B,
    get_list:      FA,
    get_block_end: FB, // returns block_end() for each element; guards against t_pos when all fields absent
) -> usize
where
    B: Buffer,
    T: ReadAt<'a>,
    T::ReadOutput: HasRawView<'a>,
    FA: Fn(&'a V) -> ListView<'a, T>,
    FB: Fn(&T::ReadOutput) -> usize,
{
    use crate::read::ReadAt as _;

    // Pass A: write each unique vtable once, record its slot.
    let mut vt_map: Vec<(Vec<u8>, usize)> = Vec::new();
    for v in views.iter() {
        let lst = get_list(v);
        for j in 0..lst.len() {
            let el  = lst.get(j);
            let raw = el.raw_view();
            let vp  = raw.v_pos;
            let vs  = u16::read_at(raw.buf, vp) as usize;
            let vb  = &raw.buf[vp..vp + vs];
            if !vt_map.iter().any(|(b, _)| b.as_slice() == vb) {
                let s = SerializeBytes::write_bytes_to(&vb, out);
                vt_map.push((vb.to_vec(), s));
            }
        }
    }

    // Pass B: copy element blocks and patch the vtable jump at byte 0.
    let mut elem_slots: Vec<usize> = Vec::new();
    let mut total_len  = 0u32;
    for v in views.iter() {
        let lst = get_list(v);
        let n   = lst.len();
        for j in (0..n).rev() {
            let el  = lst.get(j);
            let raw = el.raw_view();
            let tp  = raw.t_pos;
            // Guard: block_end() returns t_pos when all fields absent,
            // but the table object itself must still be copied.
            let obj_end = tp + u16::read_at(raw.buf, raw.v_pos + 2) as usize;
            let be  = get_block_end(&el).max(obj_end);
            let blen = be - tp;
            out.ensure_capacity(blen);
            *out.head_mut() -= blen;
            let dst = out.head();
            out.buffer_mut()[dst..dst + blen]
                .copy_from_slice(&raw.buf[tp..be]);
            // Rewrite the vtable jump.
            let vp = raw.v_pos;
            let vs = u16::read_at(raw.buf, vp) as usize;
            let vb = &raw.buf[vp..vp + vs];
            let vt_slot = vt_map.iter()
                .find(|(b, _)| b.as_slice() == vb)
                .map(|(_, s)| *s)
                .unwrap();
            let new_vt_abs = out.len() - vt_slot;
            let new_jump   = (dst as i64 - new_vt_abs as i64) as i32;
            out.buffer_mut()[dst..dst + 4]
                .copy_from_slice(&new_jump.to_le_bytes());
            elem_slots.push(out.slot());
        }
        total_len += n as u32;
    }

    // Pass C: write the forward-offset table for ListView.
    for tgt_slot in &elem_slots {
        out.ensure_capacity(7);
        *out.head_mut() -= 4;
        *out.head_mut() &= !3;
        let h    = out.head();
        let jump = (out.slot() - tgt_slot) as u32;
        out.buffer_mut()[h..h + 4].copy_from_slice(&jump.to_le_bytes());
    }

    if total_len == 0 { 0 } else { total_len.write_to_unchecked(out) }
}

/// Write a 5-byte union slot: a 4-byte forward offset followed by a 1-byte
/// discriminant tag.
///
/// Union fields in the vtable are `field_size = 5` bytes: the first four are
/// a relative offset to the payload, and the fifth is the variant tag that
/// tells the reader which concrete type to deserialize.
///
/// Called from both `owned_pass2` and `view_pass2` in the generated code.
#[inline(always)]
pub fn write_union_slot<B: Buffer>(buffer: &mut B, data_slot: usize, tag: u8) -> usize {
    *buffer.head_mut() -= 5;
    *buffer.head_mut() &= !3;
    let head = buffer.head();
    let jump = (buffer.slot() - data_slot) as u32;
    buffer.buffer_mut()[head..head + 4].copy_from_slice(&jump.to_le_bytes());
    buffer.buffer_mut()[head + 4] = tag;
    buffer.slot()
}
/// Merge a `List(Union)` field across multiple views.
///
/// # Layout
///
/// ```text
/// [len: u32][jump_0..jump_{n-1}: n×u32][tag_0..tag_{n-1}: n×u8][pad][payloads]
/// ```
///
/// Tags are read from each source view's tag section at
/// `list.offset + 4 * list.total_len() + j`, matching the layout written
/// by `write_union_slice`.
///
/// Write order: views forward, elements reversed within each view — identical
/// to `merge_table_list` so the last view's elements appear first in the
/// merged output.
///
/// Ordering proof: slots and tags are accumulated in the same traversal order
/// [V0_en-1, …, V0_e0, V1_en-1, …, V1_e0, …].  The jump table iterates
/// forward through slots (first slot → highest address → J[total-1], last
/// slot → J[0]).  Tags are written in the same forward order, so tag[i]
/// lands at `offset + 4*total_n + i` which corresponds to J[i]'s target. ✓
///
/// # Returns
///
/// The slot of the length-prefix u32, or 0 if all views were empty.
#[inline]
pub fn merge_union_list<'a, T, V, B, F>(
    views:    &'a [V],
    out:      &mut B,
    get_list: F,
) -> usize
where
    B: Buffer,
    T: ReadAt<'a>,
    T::ReadOutput: crate::serialize::Serialize,
    F: Fn(&'a V) -> ListView<'a, T>,
{
    use crate::read::ReadAt as _;
    use crate::serialize::Serialize as _;

    let total_n: usize = views.iter().map(|v| get_list(v).len()).sum();
    if total_n == 0 { return 0; }

    let mut slots: Vec<usize> = Vec::with_capacity(total_n);
    let mut tags:  Vec<u8>    = Vec::with_capacity(total_n);

    // Pass A: write payloads. Views forward, elements reversed within each view.
    // Absent elements (tag == 0) skip the payload write; their slot is 0.
    for v in views.iter() {
        let list = get_list(v);
        let n    = list.len();
        for j in (0..n).rev() {
            let tag = u8::read_at(list.buf, list.offset + 4 * n + j);
            let slot = if tag != 0 {
                list.get(j).write_to(out)   // write_to calls ensure_capacity internally
            } else {
                0
            };
            slots.push(slot);
            tags.push(tag);
        }
    }
    *out.head_mut() &= !3;
    // head is 4-aligned here: every payload type (String, Table, Struct) leaves
    // head 4-aligned on exit, matching the guarantee in write_union_slice.
    let pad = (4usize.wrapping_sub(total_n & 3)) & 3;
    out.ensure_capacity(pad + total_n + total_n * 4 + 7);
    *out.head_mut() -= pad;

    // Tag section: iterate forward through `tags` so the last-pushed tag
    // (= element 0 of last view) lands at the lowest address (tag[0]). ✓
    for &tag in tags.iter() {
        *out.head_mut() -= 1;
        let h = out.head();
        out.buffer_mut()[h] = tag;
    }
    // head is 4-aligned: pad + total_n bytes total consumed, pad chosen so
    // (pad + n) ≡ 0 (mod 4).

    // Jump table: iterate forward through `slots` so the last-pushed slot
    // (= element 0 of last view) lands at J[0] (lowest jump address). ✓
    for &target_slot in slots.iter() {
        *out.head_mut() -= 4;
        let h    = out.head();
        let jump = if target_slot == 0 { 0u32 }
                   else { (out.slot() - target_slot) as u32 };
        out.buffer_mut()[h..h + 4].copy_from_slice(&jump.to_le_bytes());
    }

    (total_n as u32).write_to_unchecked(out)
}

/// Merge a `List(FileBlob)` field — identical wire format to `List(String)`:
/// each element is a forward-offset pointer to `[len: u32][bytes...]`.
/// Reuses the same two-pass block-copy strategy as `merge_string_list`.
///
/// # Returns
///
/// The slot of the length-prefix u32, or 0 if all views were empty.
#[inline]
pub fn merge_file_list<'a, T, V, B, F>(
    views:    &'a [V],
    out:      &mut B,
    get_list: F,
) -> usize
where
    B: Buffer,
    T: ReadAt<'a>,
    F: Fn(&'a V) -> ListView<'a, T>,
{
    use crate::read::ReadAt as _;
    let mut pool_starts: Vec<usize> = Vec::with_capacity(views.len());
    let mut total_len = 0u32;

    // Pass A: copy each view's contiguous blob pool as one memcpy.
    for v in views.iter() {
        let list = get_list(v);
        if list.is_empty() { pool_starts.push(0); continue; }
        let n          = list.len();
        let pool_start = list.offset + n * 4;
        let last       = list.last_offset();
        let pool_end   = last + 4 + u32::read_at(list.buf, last) as usize;
        let pool_size  = pool_end - pool_start;
        out.ensure_capacity(pool_size + 3);
        *out.head_mut() -= pool_size;
        *out.head_mut() &= !3;
        let h = out.head();
        out.buffer_mut()[h..h + pool_size]
            .copy_from_slice(&list.buf[pool_start..pool_end]);
        pool_starts.push(h);
        total_len += n as u32;
    }

    // Pass B: copy each view's jump table and fixup entries with per-view delta.
    for (v, &dest_pool_start) in views.iter().zip(pool_starts.iter()) {
        let list = get_list(v);
        if list.is_empty() { continue; }
        let n         = list.len();
        let jump_size = n * 4;
        let delta     = dest_pool_start as i64 - out.head() as i64;
        out.ensure_capacity(jump_size + 3);
        *out.head_mut() -= jump_size;
        *out.head_mut() &= !3;
        let dst_h = out.head();
        out.buffer_mut()[dst_h..dst_h + jump_size]
            .copy_from_slice(&list.buf[list.offset..list.offset + jump_size]);
        for j in 0..n {
            let p = dst_h + j * 4;
            let v = u32::read_at(out.buffer(), p) as i64;
            out.buffer_mut()[p..p + 4]
                .copy_from_slice(&((v + delta) as u32).to_le_bytes());
        }
    }

    if total_len == 0 { 0 } else { total_len.write_to_unchecked(out) }
}