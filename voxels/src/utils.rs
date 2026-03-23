//voxels/src/utils.rs
use crate::chunk_config;

#[inline(always)]
pub const fn bits_of(value: usize) -> usize {
    if value == 0 {
        0
    } else {
        usize::BITS as usize - value.leading_zeros() as usize
    }
}
// OPTIMIZATION: Add free functions for bit operations (zero overhead)
#[inline(always)]
pub const fn bit_get(word: u32, index: usize) -> bool {
    (word >> (index & 31)) & 1 != 0
}

#[inline(always)]
pub const fn bit_set(word: &mut u32, index: usize) {
    *word |= 1u32 << (index & 31);
}

#[inline(always)]
pub const fn bit_clear(word: &mut u32, index: usize) {
    *word &= !(1u32 << (index & 31));
}

#[inline(always)]
pub const fn bit_toggle(word: &mut u32, index: usize) {
    *word ^= 1u32 << (index & 31);
}

#[inline(always)]
pub fn u32_range_mask(start: usize, extent: usize) -> u32 {
    let extent = extent.min(32 - start);
    let valid = ((extent != 0) & (start < 32)) as u32;
    (((1u32 << extent) - 1) << start) * valid
}


#[inline(always)]
pub const fn calc_grid_indices(size: usize) -> (usize,usize) {
    (calc_chunk_length(size), calc_voxel_remainder(size))
}

#[inline(always)]
pub const fn calc_chunk_length(size: usize) -> usize {
    (size as usize + chunk_config::CHUNK_SIZE - 1) >> chunk_config::VOXEL_AXIS_BITS
}

#[inline(always)]
pub const fn calc_voxel_remainder(size: usize) -> usize {
    size as usize & (chunk_config::CHUNK_SIZE - 1)
}


