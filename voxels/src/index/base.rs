//voxels/src/index/base.rs
use bevy::prelude::*;

use crate::{chunk_config, constants::{FaceAxis, FaceDirection}};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct ArrayIndex3d<const MAX_X: u32, const MAX_Y: u32,const MAX_Z: u32>(u32);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrayCoord3d<const MAX_X: u32, const MAX_Y: u32, const MAX_Z: u32> {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl<const MAX_X: u32, const MAX_Y: u32, const MAX_Z: u32> ArrayCoord3d<MAX_X, MAX_Y, MAX_Z> {
    // Calculate half-size based on the generic parameters
    const HALF_SIZE_X: i32 = MAX_X as i32 / 2;
    const HALF_SIZE_Y: i32 = MAX_Y as i32 / 2;
    const HALF_SIZE_Z: i32 = MAX_Z as i32 / 2;

    #[inline(always)]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        assert!(x >= -Self::HALF_SIZE_X && x < Self::HALF_SIZE_X, "x out of bounds");
        assert!(y >= -Self::HALF_SIZE_Y && y < Self::HALF_SIZE_Y, "y out of bounds");
        assert!(z >= -Self::HALF_SIZE_Z && z < Self::HALF_SIZE_Z, "z out of bounds");
        Self { x, y, z }
    }

    #[inline(always)]
    pub const fn translate(mut self, offset: &Self) -> Self {
        self.x += offset.x;
        self.y += offset.y;
        self.z += offset.z;
        
        assert!(self.x >= -Self::HALF_SIZE_X && self.x < Self::HALF_SIZE_X, "x out of bounds after translate");
        assert!(self.y >= -Self::HALF_SIZE_Y && self.y < Self::HALF_SIZE_Y, "y out of bounds after translate");
        assert!(self.z >= -Self::HALF_SIZE_Z && self.z < Self::HALF_SIZE_Z, "z out of bounds after translate");
        self
    }

    #[inline(always)]
    pub const fn translate_mut(&mut self, offset: &Self) {
        self.x += offset.x;
        self.y += offset.y;
        self.z += offset.z;
        
        assert!(self.x >= -Self::HALF_SIZE_X && self.x < Self::HALF_SIZE_X, "x out of bounds after translate");
        assert!(self.y >= -Self::HALF_SIZE_Y && self.y < Self::HALF_SIZE_Y, "y out of bounds after translate");
        assert!(self.z >= -Self::HALF_SIZE_Z && self.z < Self::HALF_SIZE_Z, "z out of bounds after translate");
    }

    /// Shift negative positions so that all positions fit into [0, MAX_*) range
    #[inline(always)]
    pub const fn pack(&self) -> ArrayIndex3d<MAX_X, MAX_Y, MAX_Z> {
        let x = (self.x + Self::HALF_SIZE_X) as u32;
        let y = (self.y + Self::HALF_SIZE_Y) as u32;
        let z = (self.z + Self::HALF_SIZE_Z) as u32;
        ArrayIndex3d::from_yxz(y, x, z)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deref, DerefMut)]
pub struct ArrayHeightmapIndex3d<const MAX_X: u32, const MAX_Y: u32,const MAX_Z: u32>(u32);

impl<const MAX_X: u32, const MAX_Y: u32,const MAX_Z: u32> ArrayHeightmapIndex3d<MAX_X,MAX_Y,MAX_Z> {
    const MAX_X_BITS: u32 = 32 - MAX_X.saturating_sub(1).leading_zeros();
    const MAX_Y_BITS: u32 = 32 - MAX_Y.saturating_sub(1).leading_zeros();
    const MAX_Z_BITS: u32 = 32 - MAX_Z.saturating_sub(1).leading_zeros();
    
    const Z_SHIFT: u32 = Self::MAX_X_BITS;

    const X_MASK: u32 = ((1 << Self::MAX_X_BITS) - 1);
    const Z_MASK: u32 = ((1 << Self::MAX_Z_BITS) - 1);

    const TOTAL_BITS: u32 = Self::MAX_X_BITS + Self::MAX_Y_BITS + Self::MAX_Z_BITS;
    
    #[inline(always)]
    pub const fn set(&mut self, value: u32) {
        self.0 = value
    }

    #[inline(always)]
    pub const fn new(index: &ArrayIndex3d<MAX_X,MAX_Y,MAX_Z>) -> Self {
        assert!(Self::TOTAL_BITS <= 32,"ERROR: Bits Exceed 32");
        let value = index.value() >> Self::MAX_Y_BITS;
        Self(value)
    }

    #[inline(always)]
    pub const fn x(&self) -> u32 {
        self.0 & Self::X_MASK
    }

    #[inline(always)]
    pub const fn z(&self) -> u32 {
        (self.0 >> Self::Z_SHIFT) & Self::Z_MASK
    }

    #[inline(always)]
    pub const fn value(&self) -> u32 {
        self.0
    }

    #[inline(always)]
    pub const fn index(&self) -> usize {
        self.0 as usize
    }

    #[inline(always)]
    pub const fn with_x(&self, new_x: u32) -> Self {
        assert!(new_x < MAX_X, "x value out of bounds");
        let cleared = self.0 & !(Self::X_MASK); // Clear current x bits
        let new_val = cleared | (new_x & Self::X_MASK); // Set new x
        Self(new_val)
    }

    #[inline(always)]
    pub const fn with_z(&self, new_z: u32) -> Self {
        assert!(new_z < MAX_Z, "z value out of bounds");
        let cleared = self.0 & !(Self::Z_MASK << Self::Z_SHIFT); // Clear current z bits
        let new_val = cleared | ((new_z & Self::Z_MASK) << Self::Z_SHIFT); // Set new z
        Self(new_val)
    }
    
    #[inline(always)]
    pub const fn encode_y(&self, y: u32) -> ArrayIndex3d<MAX_X,MAX_Y,MAX_Z> {
        ArrayIndex3d::<MAX_X,MAX_Y,MAX_Z>::from_yxz(y,self.x(), self.z())
    }
}

// Bit layout: [zzzzzzzzzz][xxxxxxxxxx][yyyyyyyyyy]
impl<const MAX_X: u32, const MAX_Y: u32,const MAX_Z: u32> ArrayIndex3d<MAX_X,MAX_Y,MAX_Z> {
    const MAX_X_BITS: u32 = 32 - MAX_X.saturating_sub(1).leading_zeros();
    const MAX_Y_BITS: u32 = 32 - MAX_Y.saturating_sub(1).leading_zeros();
    const MAX_Z_BITS: u32 = 32 - MAX_Z.saturating_sub(1).leading_zeros();

    const X_SHIFT: u32 = Self::MAX_Y_BITS;
    const Z_SHIFT: u32 = Self::MAX_Y_BITS + Self::MAX_X_BITS;

    const Y_MASK: u32 = (1 << Self::MAX_Y_BITS) - 1;
    const X_MASK: u32 = ((1 << Self::MAX_X_BITS) - 1) << Self::X_SHIFT;
    const Z_MASK: u32 = ((1 << Self::MAX_Z_BITS) - 1) << Self::Z_SHIFT;

    const TOTAL_BITS: u32 = Self::MAX_X_BITS + Self::MAX_Y_BITS + Self::MAX_Z_BITS;

    #[inline(always)]
    pub const fn new(index: u32) -> Self {
        assert!(Self::TOTAL_BITS <= 32,"ERROR: Bits Exceed 32");
        Self(index)
    }

    #[inline(always)]
    pub const fn value(&self) -> u32 {
        self.0
    }

    #[inline(always)]
    pub const fn index(&self) -> usize{
        self.0 as usize
    }

    #[inline(always)]
    pub const fn default() -> Self {
        Self(0)
    }

    #[inline(always)]
    pub const fn x(&self) -> u32 {
        (self.0 & Self::X_MASK) >> Self::X_SHIFT
    }

    #[inline(always)]
    pub const fn y(&self) -> u32 {
        self.0 & Self::Y_MASK
    }

    #[inline(always)]
    pub const fn z(&self) -> u32 {
        (self.0 & Self::Z_MASK) >> Self::Z_SHIFT
    }

    #[inline(always)]
    pub const fn with_y(&self, new_y: u32) -> Self {
        let cleared = self.0 & !Self::Y_MASK;
        let updated = cleared | (new_y & Self::Y_MASK);
        Self(updated)
    }

    #[inline(always)]
    pub const fn with_x(&self, new_x: u32) -> Self {
        let cleared = self.0 & !Self::X_MASK;
        let updated = cleared | ((new_x << Self::X_SHIFT) & Self::X_MASK);
        Self(updated)
    }

    #[inline(always)]
    pub const fn with_z(&self, new_z: u32) -> Self {
        let cleared = self.0 & !Self::Z_MASK;
        let updated = cleared | ((new_z << Self::Z_SHIFT) & Self::Z_MASK);
        Self(updated)
    }

    #[inline(always)]
    pub const fn from_yxz(new_y: u32, new_x: u32, new_z: u32) -> Self {
        Self::new(0)
            .with_y(new_y)
            .with_x(new_x)
            .with_z(new_z)
    }

    pub const fn heightmap_index(&self) -> ArrayHeightmapIndex3d<MAX_X,MAX_Y,MAX_Z> {
        ArrayHeightmapIndex3d::new(self)
    }

    #[inline(always)]
    pub const fn unpack(&self) -> ArrayCoord3d<MAX_X,MAX_Y,MAX_Z> {
        ArrayCoord3d {
            x: self.x() as i32,
            y: self.y() as i32,
            z: self.z() as i32,
        }
    }

    #[inline(always)]
    pub const fn next_in_direction(&self, direction: FaceDirection) -> Option<Self> {
        let (axis_value, max_value, shift) = match direction.axis() {
            FaceAxis::Y => (self.y(), MAX_Y - 1, 0),
            FaceAxis::X => (self.x(), MAX_X - 1, Self::X_SHIFT),
            FaceAxis::Z => (self.z(), MAX_Z - 1, Self::Z_SHIFT),
        };

        if (direction.is_positive() && axis_value == max_value)
            || (!direction.is_positive() && axis_value == 0)
        {
            return None;
        }

        let step = 1 << shift;
        let new_index = if direction.is_positive() {
            self.0 + step
        } else {
            self.0 - step
        };

        Some(Self(new_index))
    }

    #[inline(always)]
    pub const fn step_in_direction(&self, direction: FaceDirection, step_size: u32) -> Option<Self> {
        // Early return for zero step
        if step_size == 0 {
            return Some(*self);
        }

        let (axis_value, max_value, shift) = match direction.axis() {
            FaceAxis::Y => (self.y(), MAX_Y - 1, 0),
            FaceAxis::X => (self.x(), MAX_X - 1, Self::X_SHIFT),
            FaceAxis::Z => (self.z(), MAX_Z - 1, Self::Z_SHIFT),
        };

        if direction.is_positive() {
            if axis_value + step_size > max_value {
                return None; // would overflow
            }
            Some(Self(self.0 + (step_size << shift)))
        } else {
            if axis_value < step_size {
                return None; // would underflow
            }
            Some(Self(self.0 - (step_size << shift)))
        }
    }
}

#[macro_export]
macro_rules! define_array_index_wrapper {
    (
        $(#[$attr:meta])*
        $vis:vis struct $name:ident($inner_type:ident<$($generic:tt),*>);
    ) => {
        // Generate type aliases first
        paste::paste! {
            pub type [<$name Position>] = ArrayCoord3d<$($generic),*>;
            pub type [<$name HeightMap>] = ArrayHeightmapIndex3d<$($generic),*>;
        }

        $(#[$attr])*
        #[repr(transparent)]
        $vis struct $name($inner_type<$($generic),*>);

        // Only generate wrapper methods that return Self to avoid conflicts with Deref
        impl $name {
            #[inline(always)]
            pub const fn new(index: u32) -> Self {
                Self($inner_type::new(index))
            }
            #[inline(always)]
            pub const fn value(&self) -> u32 {
                self.0.value()
            }
            #[inline(always)]
            pub const fn index(&self) -> usize {
                self.0.value() as usize
            }

            #[inline(always)]
            pub const fn default() -> Self {
                Self($inner_type::default())
            }
            #[inline(always)]
            pub const fn x(&self) -> u32 {
                self.0.x()
            }
            #[inline(always)]
            pub const fn y(&self) -> u32 {
                self.0.y()
            }
            #[inline(always)]
            pub const fn z(&self) -> u32 {
                self.0.z()
            }

            #[inline(always)]
            pub const fn from_yxz(y: u32, x: u32, z: u32) -> Self {
                Self($inner_type::from_yxz(y, x, z))
            }

            #[inline(always)]
            pub const fn with_x(self, new_x: u32) -> Self {
                Self(self.0.with_x(new_x))
            }

            #[inline(always)]
            pub const fn with_y(self, new_y: u32) -> Self {
                Self(self.0.with_y(new_y))
            }

            #[inline(always)]
            pub const fn with_z(self, new_z: u32) -> Self {
                Self(self.0.with_z(new_z))
            }

            #[inline(always)]
            pub const fn unpack(&self) -> paste::paste! { [<$name Position>] } {
                self.0.unpack()
            }

            #[inline(always)]
            pub const fn heightmap_index(&self) -> paste::paste! { [<$name HeightMap>] } {
                self.0.heightmap_index()
            }

            #[inline(always)]
            pub const fn next_in_direction(self, direction: crate::constants::FaceDirection) -> Option<Self> {
                match self.0.next_in_direction(direction) {
                    Some(inner) => Some(Self(inner)),
                    None => None,
                }
            }

            #[inline(always)]
            pub const fn step_in_direction(self, direction: crate::constants::FaceDirection, step_size: u32) -> Option<Self> {
                match self.0.step_in_direction(direction, step_size) {
                    Some(inner) => Some(Self(inner)),
                    None => None,
                }
            }
        }

    };
}

// // Wrapper struct for chunk indexing
// // Usage example:
// define_array_index_wrapper! {
//     #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Component, ExtractComponent, Deref, DerefMut)]
//     pub struct ChunkIndex(ArrayIndex3d<
//         {(chunk_config::MAX_CHUNKS_PER_AXIS-1) as u32},
//         {(chunk_config::MAX_CHUNKS_PER_AXIS-1) as u32},
//         {(chunk_config::MAX_CHUNKS_PER_AXIS-1) as u32}
//     >);
// }

// Replace your VoxelIndex with this:
define_array_index_wrapper! {
    #[derive(Debug , Clone, Copy, Hash, PartialEq,Eq)]
    pub struct VoxelIndex(ArrayIndex3d<
        {(chunk_config::CHUNK_SIZE) as u32},
        {(chunk_config::CHUNK_SIZE) as u32},
        {(chunk_config::CHUNK_SIZE) as u32}
    >);
}