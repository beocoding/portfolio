//voxels/src/index/index.rs
use bevy::{prelude::*, render::extract_component::ExtractComponent};

// represents a 1024x1024x1024 bit array, linearized to i|j<<10|k<<20, 30 bits
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Component, ExtractComponent)]

pub struct ChunkIndex(pub u32);

impl ChunkIndex {
    #[inline(always)]
    pub const fn from_transform(transform: &Transform) -> Self {
        // World position → chunk offset → add 512 to center at chunk (512,512,512)
        let chunk_x = (((transform.translation.x as i32) >> 5)) as u32;
        let chunk_y = (((transform.translation.y as i32) >> 5)) as u32;
        let chunk_z = (((transform.translation.z as i32) >> 5)) as u32;
        
        // Pack into u32 with layout: y|x<<10|z<<20 (Y fastest changing)
        Self(chunk_y | (chunk_x << 10) | (chunk_z << 20))
    }

    #[inline(always)]
    pub const fn from_chunk_coord(x: u32, y: u32, z: u32) -> Self {
        const Y_MASK: u32 = (1 << 10) - 1;  // 10 bits
        const X_MASK: u32 = (1 << 10) - 1;  // 10 bits
        const Z_MASK: u32 = (1 << 10) - 1;  // 10 bits

        Self(
            (y & Y_MASK)
            | ((x & X_MASK) << 10)
            | ((z & Z_MASK) << 20)
        )
    }
    #[inline(always)]
    pub const fn x(self) -> u32 {
        (self.0 >> 10) & 0x3FF  // 10 bits
    }

    #[inline(always)]
    pub const fn y(self) -> u32 {
        self.0 & 0x3FF          // 10 bits
    }

    #[inline(always)]
    pub const fn z(self) -> u32 {
        (self.0 >> 20) & 0x3FF  // 12 bits
    }
}