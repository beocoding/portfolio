// fluffr/flatr_core/src/verify.rs
use crate::read::ReadAt;

// ── Error ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum VerifyError {
    OutOfBounds    { context: &'static str, offset: usize, needed: usize, buf_len: usize },
    BadOffset      { at: usize },
    VTableTooSmall { vtable_size: usize },
    FieldOutOfBounds { field_idx: usize, voff: usize, field_size: usize, object_size: usize },
    InvalidUtf8    { at: usize },
    VectorOverflow { at: usize, len: usize, elem_size: usize },
    DepthLimitExceeded,
}

/// Unit on success — vtable positions are accumulated into the caller-supplied
/// `out: &mut Vec<usize>` so the entire traversal shares one allocation.
pub type VerifyResult = Result<(), VerifyError>;

pub const MAX_DEPTH: usize = 64;

// ── Trait ─────────────────────────────────────────────────────────────────────

pub trait Verify {
    const INLINE_SIZE: usize;

    /// Verify the value at `offset`.  Every vtable position reached during
    /// traversal is pushed onto `out`; duplicates are expected and deduplicated
    /// by the caller once at the end.
    fn verify_at(buf: &[u8], offset: usize, depth: usize, out: &mut Vec<usize>) -> VerifyResult;
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Verify a finished buffer and return the unique vtable positions found.
pub fn verify_root<T: Verify>(buf: &[u8]) -> Result<Vec<usize>, VerifyError> {
    check_bounds(buf, 0, 4, "root offset")?;
    let root_offset = u32::read_at(buf, 0) as usize;
    let mut positions = Vec::new();
    T::verify_at(buf, root_offset, MAX_DEPTH, &mut positions)?;
    Ok(positions)
}

// ── Public helpers ────────────────────────────────────────────────────────────

#[inline]
pub fn verify_table_header(
    buf: &[u8], t_pos: usize, depth: usize,
) -> Result<(usize, usize, usize), VerifyError> {
    if depth == 0 { return Err(VerifyError::DepthLimitExceeded); }

    check_bounds(buf, t_pos, 4, "vtable pointer")?;
    let jump  = i32::read_at(buf, t_pos);
    let v_pos = checked_apply_i32(t_pos, jump)
        .filter(|&v| v < buf.len())
        .ok_or(VerifyError::BadOffset { at: t_pos })?;

    check_bounds(buf, v_pos, 4, "vtable header")?;
    let vtable_size = u16::read_at(buf, v_pos)     as usize;
    let object_size = u16::read_at(buf, v_pos + 2) as usize;

    if vtable_size < 4 { return Err(VerifyError::VTableTooSmall { vtable_size }); }
    check_bounds(buf, v_pos, vtable_size, "vtable body")?;
    check_bounds(buf, t_pos, object_size, "table object")?;

    Ok((v_pos, vtable_size, object_size))
}

#[inline]
pub fn verify_vtable_field(
    buf: &[u8], v_pos: usize, vtable_size: usize,
    t_pos: usize, object_size: usize,
    field_idx: usize, field_size: usize,
) -> Result<Option<usize>, VerifyError> {
    let entry_off = 4 + field_idx * 2;
    if entry_off + 2 > vtable_size { return Ok(None); }
    let voff = u16::read_at(buf, v_pos + entry_off) as usize;
    if voff == 0 { return Ok(None); }
    if voff.saturating_add(field_size) > object_size {
        return Err(VerifyError::FieldOutOfBounds { field_idx, voff, field_size, object_size });
    }
    Ok(Some(t_pos + voff))
}

#[inline]
pub fn verify_string_field(buf: &[u8], field_pos: usize) -> VerifyResult {
    check_bounds(buf, field_pos, 4, "string forward-offset")?;
    let hdr_pos = field_pos.saturating_add(u32::read_at(buf, field_pos) as usize);
    check_bounds(buf, hdr_pos, 4, "string length prefix")?;
    let len = u32::read_at(buf, hdr_pos) as usize;
    check_bounds(buf, hdr_pos + 4, len, "string bytes")?;
    std::str::from_utf8(&buf[hdr_pos + 4..hdr_pos + 4 + len])
        .map_err(|_| VerifyError::InvalidUtf8 { at: hdr_pos + 4 })?;
    Ok(())
}
#[inline]
pub fn verify_file_field(buf: &[u8], field_pos: usize) -> VerifyResult {
    check_bounds(buf, field_pos, 4, "File forward-offset")?;
    let hdr_pos = field_pos.saturating_add(u32::read_at(buf, field_pos) as usize);
    check_bounds(buf, hdr_pos, 4, "File length prefix")?;
    let len = u32::read_at(buf, hdr_pos) as usize;
    check_bounds(buf, hdr_pos + 4, len, "File bytes")?;
    std::str::from_utf8(&buf[hdr_pos + 4..hdr_pos + 4 + len])
        .map_err(|_| VerifyError::InvalidUtf8 { at: hdr_pos + 4 })?;
    Ok(())
}
#[inline]
pub fn verify_scalar_array(buf: &[u8], field_pos: usize, elem_size: usize) -> VerifyResult {
    check_bounds(buf, field_pos, 4, "array forward-offset")?;
    let hdr_pos = field_pos.saturating_add(u32::read_at(buf, field_pos) as usize);
    check_bounds(buf, hdr_pos, 4, "array length prefix")?;
    let len = u32::read_at(buf, hdr_pos) as usize;
    let total = len.checked_mul(elem_size)
        .ok_or(VerifyError::VectorOverflow { at: hdr_pos, len, elem_size })?;
    check_bounds(buf, hdr_pos + 4, total, "array data")
}

#[inline]
pub fn verify_string_array(buf: &[u8], field_pos: usize) -> VerifyResult {
    check_bounds(buf, field_pos, 4, "string-array forward-offset")?;
    let hdr_pos = field_pos.saturating_add(u32::read_at(buf, field_pos) as usize);
    check_bounds(buf, hdr_pos, 4, "string-array length prefix")?;
    let len = u32::read_at(buf, hdr_pos) as usize;
    let total = len.checked_mul(4)
        .ok_or(VerifyError::VectorOverflow { at: hdr_pos, len, elem_size: 4 })?;
    check_bounds(buf, hdr_pos + 4, total, "string-array offset table")?;
    for i in 0..len {
        verify_string_field(buf, hdr_pos + 4 + i * 4)?;
    }
    Ok(())
}
#[inline]
pub fn verify_file_array(buf: &[u8], field_pos: usize) -> VerifyResult {
    check_bounds(buf, field_pos, 4, "file-array forward-offset")?;
    let hdr_pos = field_pos.saturating_add(u32::read_at(buf, field_pos) as usize);
    check_bounds(buf, hdr_pos, 4, "file-array length prefix")?;
    let len = u32::read_at(buf, hdr_pos) as usize;
    let total = len.checked_mul(4)
        .ok_or(VerifyError::VectorOverflow { at: hdr_pos, len, elem_size: 4 })?;
    check_bounds(buf, hdr_pos + 4, total, "file-array offset table")?;
    for i in 0..len {
        verify_file_field(buf, hdr_pos + 4 + i * 4)?;
    }
    Ok(())
}
/// `out` receives one position per table element — duplicates are expected
/// when elements share a vtable (the common case) and are deduplicated later.
#[inline]
pub fn verify_table_array<T: Verify>(
    buf: &[u8], field_pos: usize, depth: usize, out: &mut Vec<usize>,
) -> VerifyResult {
    check_bounds(buf, field_pos, 4, "table-array forward-offset")?;
    let hdr_pos = field_pos.saturating_add(u32::read_at(buf, field_pos) as usize);
    check_bounds(buf, hdr_pos, 4, "table-array length prefix")?;
    let len = u32::read_at(buf, hdr_pos) as usize;
    let total = len.checked_mul(4)
        .ok_or(VerifyError::VectorOverflow { at: hdr_pos, len, elem_size: 4 })?;
    check_bounds(buf, hdr_pos + 4, total, "table-array offset table")?;
    for i in 0..len {
        let ep        = hdr_pos + 4 + i * 4;
        let table_pos = ep.saturating_add(u32::read_at(buf, ep) as usize);
        T::verify_at(buf, table_pos, depth, out)?;
    }
    Ok(())
}

#[inline]
pub fn verify_table_field<T: Verify>(
    buf: &[u8], field_pos: usize, depth: usize, out: &mut Vec<usize>,
) -> VerifyResult {
    check_bounds(buf, field_pos, 4, "table forward-offset")?;
    let table_pos = field_pos.saturating_add(u32::read_at(buf, field_pos) as usize);
    T::verify_at(buf, table_pos, depth, out)
}

// ── Scalar Verify impls ───────────────────────────────────────────────────────

macro_rules! impl_verify_scalar {
    ($($t:ty),*) => {$(
        impl Verify for $t {
            const INLINE_SIZE: usize = ::std::mem::size_of::<Self>();
            #[inline(always)]
            fn verify_at(buf: &[u8], offset: usize, _depth: usize, _out: &mut Vec<usize>) -> VerifyResult {
                check_bounds(buf, offset, ::std::mem::size_of::<Self>(), "scalar")
            }
        }
    )*};
}
impl_verify_scalar!(u8, u16, u32, u64, u128, i8, i16, i32, i64, i128, f32, f64, bool);

// verify.rs — replace both impls

impl Verify for &str {
    const INLINE_SIZE: usize = 4;
    #[inline]
    fn verify_at(buf: &[u8], offset: usize, _depth: usize, _out: &mut Vec<usize>) -> VerifyResult {
        // `offset` is the direct string header (length prefix), matching ReadAt::read_at.
        // Callers in the table path use verify_string_field() directly and never reach here.
        check_bounds(buf, offset, 4, "string length prefix")?;
        let len = u32::read_at(buf, offset) as usize;
        check_bounds(buf, offset + 4, len, "string bytes")?;
        std::str::from_utf8(&buf[offset + 4..offset + 4 + len])
            .map_err(|_| VerifyError::InvalidUtf8 { at: offset + 4 })?;
        Ok(())
    }
}

impl Verify for String {
    const INLINE_SIZE: usize = 4;
    #[inline]
    fn verify_at(buf: &[u8], offset: usize, _depth: usize, _out: &mut Vec<usize>) -> VerifyResult {
        check_bounds(buf, offset, 4, "string length prefix")?;
        let len = u32::read_at(buf, offset) as usize;
        check_bounds(buf, offset + 4, len, "string bytes")?;
        std::str::from_utf8(&buf[offset + 4..offset + 4 + len])
            .map_err(|_| VerifyError::InvalidUtf8 { at: offset + 4 })?;
        Ok(())
    }
}

// ── Internal primitives ───────────────────────────────────────────────────────

#[inline(always)]
pub fn check_bounds(buf: &[u8], offset: usize, needed: usize, ctx: &'static str) -> VerifyResult {
    if offset.saturating_add(needed) > buf.len() {
        Err(VerifyError::OutOfBounds { context: ctx, offset, needed, buf_len: buf.len() })
    } else {
        Ok(())
    }
}

#[inline(always)]
fn checked_apply_i32(base: usize, delta: i32) -> Option<usize> {
    if delta >= 0 { base.checked_sub(delta as usize) }
    else          { base.checked_add(delta.unsigned_abs() as usize) }
}

