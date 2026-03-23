// src/lib.rs
pub mod constants;
pub mod pipeline;
pub mod buffers;
pub mod debug;
pub mod utils;   
pub mod index;
pub mod camera;
pub mod terrain;
pub mod chunk_config{
    pub const CHUNK_SIZE: usize = 32;
    pub const CHUNK_AREA: usize = CHUNK_SIZE*CHUNK_SIZE;
    pub const CHUNK_VOLUME: usize = CHUNK_SIZE*CHUNK_SIZE*CHUNK_SIZE;
    pub const MAX_CHUNKS_PER_AXIS: usize = 1024;
    pub const MAX_CHUNKS_AREA: usize = MAX_CHUNKS_PER_AXIS*MAX_CHUNKS_PER_AXIS;
    pub const MAX_CHUNKS_VOLUME: usize = MAX_CHUNKS_PER_AXIS*MAX_CHUNKS_PER_AXIS*MAX_CHUNKS_PER_AXIS;

    pub const CHUNK_AXIS_BITS: usize = (32 - ((MAX_CHUNKS_PER_AXIS - 1) as u32).leading_zeros()) as usize;
    pub const VOXEL_AXIS_BITS: usize = 32- ((CHUNK_SIZE-1) as u32).leading_zeros() as usize;
    pub const VOXEL_AXIS_BIT_MASK: usize = CHUNK_SIZE-1;
}
pub mod bits;
pub mod buffer_config{
    pub const DEFAULT_CHUNKMESH_SSBO_SIZE: usize = 1024 * 1024; // 8MB
    pub const DEFAULT_INSTANCE_BUFFER_CAPACITY: usize = 1024 * 1024; // 1 million instances
}

