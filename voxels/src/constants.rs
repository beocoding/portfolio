
// voxels/src/constants.rs
use core::fmt;

use bytemuck::{Pod, Zeroable};
use bevy::{camera::visibility::NoFrustumCulling, prelude::*};
use crate::{bits::ChunkData, buffers::{ChunkMeshRange, Dirty}, index::{index::ChunkIndex}};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(u8)]
pub enum VoxelType {
    Air,
    Stone,
    Dirt,
    Water,
}


// Conversion
impl From<u8> for VoxelType {
    fn from(value: u8) -> Self {
        match value {
            0 => VoxelType::Air,
            1 => VoxelType::Stone,
            2 => VoxelType::Dirt,
            3 => VoxelType::Water,
            _ => VoxelType::Air,
        }
    }
}

impl From<VoxelType> for u8 {
    fn from(value: VoxelType) -> u8 {
        value as u8
    }
}

impl VoxelType {
    pub const NONE: Option<Self> = None;
    #[inline(always)]
    pub const fn default() -> Self { VoxelType::Air }
    #[inline(always)]
    pub const fn from_index(index: usize) -> Self {
        match index {
            x if x == Self::Stone as usize => Self::Stone,
            x if x == Self::Dirt as usize => Self::Dirt,
            x if x == Self::Water as usize => Self::Water,
            _ => Self::Air,
        }
    }
}


#[derive(Bundle)]
pub struct Chunk {
    mesh: Mesh3d,
    data: ChunkData,
    index: ChunkIndex,
    buffer_slice: ChunkMeshRange,
    dirty: Dirty,
    frustum: NoFrustumCulling
}
impl Chunk {
    #[inline(always)]
    pub const fn new(mesh: Handle<Mesh>, data: ChunkData, index: ChunkIndex, buffer_slice: ChunkMeshRange) -> Self {
        Self {
            mesh: Mesh3d(mesh),
            data,
            index,
            buffer_slice,
            dirty: Dirty,
            frustum: NoFrustumCulling
        }
    }
}

#[inline(always)]
pub const fn rhs_contains_lhs(
    lhs_start: u8,
    lhs_span: u8,
    rhs_start: u8,
    rhs_span: u8,
) -> (bool, bool, bool) {
    let lhs_end = lhs_start + lhs_span;
    let rhs_end = rhs_start + rhs_span;

    let contained = lhs_start >= rhs_start && lhs_end <= rhs_end;
    let has_pre = contained && lhs_start > rhs_start;
    let has_post = contained && lhs_end < rhs_end;

    (contained, has_pre, has_post)
}


#[derive(Clone, Copy, Default, Eq, PartialEq, Hash, Pod, Zeroable)]
#[repr(C)]
pub struct VoxelInstance(pub u32);

impl fmt::Debug for VoxelInstance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VoxelInstance")
            .field("raw", &format_args!("{:032b}", self.0)) // hex with leading 0x
            .field("x", &self.x())
            .field("y", &self.y())
            .field("z", &self.z())
            .field("primary", &self.primary())
            .field("secondary", &self.secondary())
            .field("material", &self.material())
            .finish()
    }
}

impl VoxelInstance{
    #[inline(always)]
    pub const fn try_next(&self, face_axis: FaceAxis) -> bool {
        let step = match face_axis {
            FaceAxis::Y => 0,
            FaceAxis::X => 5,
            FaceAxis::Z => 10
        };
        (self.0 >> step)<31
    }
    #[inline(always)]
    pub const fn try_merge_primary(&mut self, rhs: &Self, face_axis: FaceAxis) -> bool {
        let can_merge = self.can_merge_primary(rhs, face_axis);
        if can_merge{self.inc_primary()}
        can_merge
    }

    #[inline(always)]
    pub const fn can_merge_primary(&self, rhs: &Self, face_axis: FaceAxis) -> bool {
        const SAME_YZM_MASK: u32 = 0xFE007C00;
        const SAME_XZM_MASK: u32 = 0xFE007FE0;

        let can_merge = match face_axis {
            FaceAxis::X => {
                ((self.0^rhs.0)&SAME_XZM_MASK==0) && rhs.y() == self.y() + self.primary() + 1
            },
            FaceAxis::Y => {
                ((self.0^rhs.0)&SAME_YZM_MASK==0) && rhs.x() == self.x() + self.primary() + 1
            },
            FaceAxis::Z => {
                ((self.0^rhs.0)&SAME_XZM_MASK==0) && rhs.y() == self.y() + self.primary() + 1
            },
        };

        if !can_merge {return false}
        true
    }

    #[inline(always)]
    pub const fn try_merge_secondary(&mut self, rhs:&Self, face_axis: FaceAxis) -> (bool, Option<VoxelInstance>, Option<VoxelInstance>) {
        const SAME_YM_MASK: u32 = VoxelInstance::new(0,31,0,0,0,255).0;
        const SAME_XM_MASK: u32 = VoxelInstance::new(31,0,0,0,0,255).0;
        const SAME_ZM_MASK: u32 = VoxelInstance::new(0,0,31,0,0,255).0;

        let (is_same_slice_material_and_adjacent, lhs_start, rhs_start) = match face_axis {
            FaceAxis::X => {(((self.0^rhs.0)&SAME_XM_MASK==0 && rhs.z() == self.z() + self.secondary() + 1),self.y() as u8, rhs.y() as u8)},
            FaceAxis::Y => {(((self.0^rhs.0)&SAME_YM_MASK==0 && rhs.z() == self.z() + self.secondary() + 1),self.x() as u8, rhs.x() as u8)},
            FaceAxis::Z => {(((self.0^rhs.0)&SAME_ZM_MASK==0 && rhs.x() == self.x() + self.secondary() + 1),self.y() as u8, rhs.y() as u8)},
        };
        if !is_same_slice_material_and_adjacent{
            return (false,None,None)
        };
        let lhs_span = self.primary() as u8;
        let rhs_span = rhs.primary() as u8;

        let (contains, has_head, has_tail) = rhs_contains_lhs(lhs_start, lhs_span, rhs_start, rhs_span);
        if contains {self.inc_secondary()};

        let head = if has_head {
            let mut head = VoxelInstance(rhs.0);
            let head_length = lhs_start - rhs_start; // Length of head region
            let head_extent = head_length - 1; // Convert to inclusive extent
            head.set_primary(head_extent as u32);
            Some(head)
        } else { None };

        let tail = if has_tail {
            let mut tail = VoxelInstance(rhs.0);
            let tail_start = lhs_start + lhs_span + 1; // First position after lhs
            let tail_length = (rhs_start + rhs_span + 1) - tail_start; // Length of tail
            let tail_extent = tail_length - 1; // Convert to inclusive extent
            
            match face_axis {
                FaceAxis::X => tail.set_y(tail_start as u32),
                FaceAxis::Y => tail.set_x(tail_start as u32),
                FaceAxis::Z => tail.set_y(tail_start as u32),
            }
            tail.set_primary(tail_extent as u32);
            Some(tail)
        } else { None };


        (contains, head, tail)
    }

}
impl VoxelInstance {
    const BITS: u32 = 5; //default 5 bits
    // Shifts
    pub const Y_SHIFT: u32 = 0;
    pub const X_SHIFT: u32 = Self::BITS * 1;
    pub const Z_SHIFT: u32 = Self::BITS * 2;
    pub const L_SHIFT: u32 = Self::BITS * 3;
    pub const W_SHIFT: u32 = Self::BITS * 4;
    pub const MATERIAL_SHIFT: u32 = Self::BITS * 5;

    // Masks
    pub const AXIS_MASK: u32 = (1 << Self::BITS) - 1;
    pub const MATERIAL_BITS: u32 = 0x7F; // 7 bits 
    const MATERIAL_MASK: u32 = Self::MATERIAL_BITS << Self::MATERIAL_SHIFT;
    pub const INDEX_MASK: u32 = (1 << (Self::BITS * 3)) - 1;

    #[inline(always)]
    pub const fn default() -> Self {
        Self(0)
    }

    #[inline(always)]
    pub const fn clear() -> Self {
        Self(0)
    }

    #[inline(always)]
    pub const fn new(x: u8, y: u8, z: u8, primary: u8, secondary: u8, material: u8) -> Self {
        let value: u32 = (y as u32 & Self::AXIS_MASK) 
            | ((x as u32 & Self::AXIS_MASK) << Self::X_SHIFT) 
            | ((z as u32 & Self::AXIS_MASK) << Self::Z_SHIFT) 
            | ((primary as u32 & Self::AXIS_MASK) << Self::L_SHIFT) 
            | ((secondary as u32 & Self::AXIS_MASK) << Self::W_SHIFT) 
            | ((material as u32 & Self::MATERIAL_BITS) << Self::MATERIAL_SHIFT);
        Self(value)
    }

    #[inline(always)]
    pub const fn create_index(index:u32, primary: u8, secondary: u8, material: u8) -> Self {
        let value: u32 = index & Self::INDEX_MASK
            | ((primary as u32 & Self::AXIS_MASK) << Self::L_SHIFT) 
            | ((secondary as u32 & Self::AXIS_MASK) << Self::W_SHIFT) 
            | ((material as u32 & Self::MATERIAL_BITS) << Self::MATERIAL_SHIFT);
        Self(value)
    }

    #[inline(always)]
    pub const fn word(&self) -> u32 {
        (self.0>>5)&1023
    }

    #[inline(always)]
    pub const fn max_instance(direction: FaceDirection, material: u8) -> Self {
        const BASE_VAL: u32 = 0x01FF8000;      // (0,0,0) + extents
        const BASE_VAL_Y: u32 = 0x01FF801F;    // (0,31,0) + extents
        const BASE_VAL_X: u32 = 0x01FF83E0;    // (31,0,0) + extents
        const BASE_VAL_Z: u32 = 0x01FFFC00;    // (0,0,31) + extents
        
        let base = match direction {
            FaceDirection::YP => BASE_VAL_Y,
            FaceDirection::XP => BASE_VAL_X,
            FaceDirection::ZP => BASE_VAL_Z,
            _ => BASE_VAL
        };
        let value: u32 = base|((material as u32 & Self::MATERIAL_BITS) << Self::MATERIAL_SHIFT);
        Self(value)
    }

    #[inline(always)]
    pub const fn from_u32_idx(index: u32) -> Self {
        Self(index)
    }

    #[inline(always)]
    pub const fn from_u16_idx(index: u16) -> Self {
        Self(index as u32)
    }
    
    #[inline(always)]
    pub const fn from_word_bit(word:usize, bit: usize) -> Self {
        Self((bit|(word<<5)) as u32)
    }

    #[inline(always)]
    pub const fn from_material(index: u32, material: u8) -> Self {
        Self(index|((material as u32) <<Self::MATERIAL_SHIFT))
    }

    #[inline(always)]
    pub const fn set_index(&mut self, index: u32) {
        // Swap only the lower 15 bits
        pub const MASK: u32 = 0x7FFF;
        let x = (self.0 ^ index) & MASK;
        self.0 ^= x;
    }

    #[inline(always)]
    pub const fn set_xz(&mut self, index: u32) {
        pub const MASK: u32 = 0x7FE0; // bits 5-14
        let x = (self.0 ^ index) & MASK;
        self.0 ^= x;
    }
    
    #[inline(always)]
    pub const fn set_yz(&mut self, index: u32) {
        const MASK: u32 = 0x7C1F; // bits 0-4 (Y) and 10-14 (Z)
        let x = (self.0 ^ index) & MASK;
        self.0 ^= x;
    }

    #[inline(always)]
    pub const fn set_y(&mut self, y: u32) {
        self.0 ^= (self.0 ^ (y as u32)) & 0x1F;
    }

    #[inline(always)]
    pub const fn set_x(&mut self, x_val: u32) {
        let x = ((self.0 >> Self::X_SHIFT) ^ (x_val as u32)) & Self::AXIS_MASK;
        self.0 ^= x << Self::X_SHIFT;
    }

    #[inline(always)]
    pub const fn set_z(&mut self, z_val: u32) {
        let x = ((self.0 >> Self::Z_SHIFT) ^ (z_val as u32)) & Self::AXIS_MASK;
        self.0 ^= x << Self::Z_SHIFT;
    }

    #[inline(always)]
    pub const fn set_primary(&mut self, l_val: u32) {
        let x = ((self.0 >> Self::L_SHIFT) ^ (l_val as u32)) & Self::AXIS_MASK;
        self.0 ^= x << Self::L_SHIFT;
    }

    #[inline(always)]
    pub const fn set_secondary(&mut self, w_val: u32) {
        let x = ((self.0 >> Self::W_SHIFT) ^ (w_val as u32)) & Self::AXIS_MASK;
        self.0 ^= x << Self::W_SHIFT;
    }

    #[inline(always)]
    pub const fn set_material(&mut self, material_id: u8) {
        let x = (self.0 >> Self::MATERIAL_SHIFT) ^ (material_id as u32);
        self.0 ^= x << Self::MATERIAL_SHIFT;
    }

    #[inline(always)]
    pub const fn index(&self) -> usize {
        (self.0 & Self::INDEX_MASK) as usize
    }

    #[inline(always)]
    pub const fn y(&self) -> u32 {
        self.0&31
    }
    #[inline(always)]
    pub const fn x(&self) -> u32 {
        (self.0>>5)&31
    }
    #[inline(always)]
    pub const fn z(&self) -> u32 {
        (self.0>>10)&31
    }
    #[inline(always)]
    pub const fn primary(&self) -> u32 {
        (self.0 >> Self::L_SHIFT) & Self::AXIS_MASK
    }

    #[inline(always)]
    pub const fn secondary(&self) -> u32 {
        (self.0 >> Self::W_SHIFT) & Self::AXIS_MASK
    }

    #[inline(always)]
    pub const fn material(&self) -> u32 {
        self.0 >> Self::MATERIAL_SHIFT
    }
}
impl VoxelInstance {
    // Stride constants (based on your earlier shifts)
    const Y_STRIDE: u32 = 1 << Self::Y_SHIFT; // 1
    const X_STRIDE: u32 = 1 << Self::X_SHIFT; // 32
    const Z_STRIDE: u32 = 1 << Self::Z_SHIFT; // 1024
    
    // Increment functions
    #[inline(always)]
    pub const fn inc_y(&mut self) {
        self.set_index((self.index() as u32 + Self::Y_STRIDE) & Self::INDEX_MASK);
    }
    #[inline(always)]
    pub const fn inc_y_unchecked(&mut self) {
        self.0+=1;
    }

    #[inline(always)]
    pub const fn dec_y(&mut self) {
        self.set_index((self.index() as u32 + 32768 - Self::Y_STRIDE) & Self::INDEX_MASK);
    }

    #[inline(always)]
    pub const fn inc_x(&mut self) {
        self.set_index((self.index() as u32 + Self::X_STRIDE) & Self::INDEX_MASK);
    }
    #[inline(always)]
    pub const fn inc_x_unchecked(&mut self) {
        self.0+=32;
    }

    #[inline(always)]
    pub const fn dec_x(&mut self) {
        self.set_index((self.index() as u32 + 32768 - Self::X_STRIDE) & Self::INDEX_MASK);
    }

    #[inline(always)]
    pub const fn inc_z(&mut self) {
        self.set_index((self.index() as u32 + Self::Z_STRIDE) & Self::INDEX_MASK);
    }
    #[inline(always)]
    pub const fn inc_z_unchecked(&mut self) {
        self.0+=1024;
    }

    #[inline(always)]
    pub const fn dec_z(&mut self) {
        self.set_index((self.index() as u32 + 32768 - Self::Z_STRIDE) & Self::INDEX_MASK);
    }

    #[inline(always)]
    pub const fn inc_primary(&mut self) {
        let new_l = (self.primary() + 1) & Self::AXIS_MASK;
        self.set_primary(new_l);
    }

    #[inline(always)]
    pub const fn inc_primary_unchecked(&mut self) {
        self.0+=32768;
    }

    #[inline(always)]
    pub const fn dec_primary(&mut self) {
        let new_l = (self.primary() + 32 - 1) & Self::AXIS_MASK;
        self.set_primary(new_l);
    }

    #[inline(always)]
    pub const fn inc_secondary(&mut self) {
        let new_w = (self.secondary() + 1) & Self::AXIS_MASK;
        self.set_secondary(new_w);
    }

    #[inline(always)]
    pub const fn dec_secondary(&mut self) {
        let new_w = (self.secondary() + 32 - 1) & Self::AXIS_MASK;
        self.set_secondary(new_w);
    }

    #[inline(always)]
    pub const fn can_extend_primary(&self, axis: FaceAxis) -> bool {
        match axis {
            FaceAxis::Y => {self.x() + self.primary() <31},
            _ => {self.y() + self.primary() <31},
        }
    }
    #[inline(always)]
    pub const fn can_extend_secondary(&self, axis: FaceAxis) -> bool {
        match axis {
            FaceAxis::Z => {self.x() + self.secondary() <31},
            _ => {self.z() + self.secondary() <31},
        }
    }
}

#[derive(Component,Clone, Debug, Deref, DerefMut)]
pub struct TempChunkMeshData {
    pub data: [Vec<VoxelInstance>;6]
}

impl TempChunkMeshData {
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            data: [
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ]
        }
    }
    #[inline(always)]
    pub const fn default() -> Self {
        Self::new()
    }
    #[inline(always)]
    pub fn clear(&mut self) {
        for vec in &mut self.data {
            vec.clear();
        }
    }

    #[inline(always)]
    pub fn push_face(&mut self, face: FaceDirection, instance: VoxelInstance) {        
        self.data[face as usize].push(instance);
    }

}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceAxis {
    Y,
    X,
    Z
}
impl FaceAxis {
    #[inline(always)]
    pub const fn from_usize(value: usize) -> FaceAxis {
        // Take pairs (>>1), only need 2 LSBs for cycle
        const MAP: [FaceAxis; 3] = [FaceAxis::Y, FaceAxis::X, FaceAxis::Z];
        MAP[(value >> 1) % 3] // modulo can be replaced by a mask+compare
    }
}
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Zeroable)]
pub enum FaceDirection {
    YP,
    YN,
    XP,
    XN,
    ZP,
    ZN,
}

impl FaceDirection {
    #[inline(always)]
    pub const fn default() -> Self {
        Self::YP
    }

    #[inline(always)]
    pub const fn primary_axis(&self) -> Self {
        match self {
            Self::YP|Self::YN => Self::XP,
            Self::XP|Self::XN => Self::YP,
            Self::ZP|Self::ZN => Self::YP,
        }
    }
    #[inline(always)]
    pub const fn secondary_axis(&self) -> Self {
        match self {
            Self::YP|Self::YN => Self::ZP,
            Self::XP|Self::XN => Self::ZP,
            Self::ZP|Self::ZN => Self::XP,
        }
    }

    #[inline(always)]
    pub const fn from_index(index: usize) -> Self {
        match index {
            0 => Self::YP,
            1 => Self::YN,
            2 => Self::XP,
            3 => Self::XN,
            4 => Self::ZP,
            5 => Self::ZN,
            _ => Self::YP, // Or your default
        }
    }
    
    #[inline(always)]
    pub const fn is_positive(&self) -> bool {
        match self {
            FaceDirection::YP | FaceDirection::XP | FaceDirection::ZP => true,
            FaceDirection::YN | FaceDirection::XN | FaceDirection::ZN => false,
        }
    }
    #[inline(always)]
    pub const fn axis(&self) -> FaceAxis {
        match self {
            FaceDirection::YP | FaceDirection::YN => FaceAxis::Y,
            FaceDirection::XP | FaceDirection::XN => FaceAxis::X,
            FaceDirection::ZP | FaceDirection::ZN => FaceAxis::Z,
        }
    }
    #[inline(always)]
    pub const fn reflect(&self) -> Self {
        match self {
            Self::YP => Self::YN,
            Self::YN => Self::YP,
            Self::XP => Self::XN,
            Self::XN => Self::XP,
            Self::ZP => Self::ZN,
            Self::ZN => Self::ZP,
        }
    }

    pub const ALL:[FaceDirection;6] = [
        Self::YP,
        Self::YN,
        Self::XP,
        Self::XN,
        Self::ZP,
        Self::ZN,
    ];

    #[inline(always)]
    pub fn iter()-> std::slice::Iter<'static, FaceDirection>{
        Self::ALL.iter()
    }
}
