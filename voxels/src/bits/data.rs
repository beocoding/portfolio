//voxels/src/bits/data.rs
use bevy::ecs::component::Component;
use crate::constants::{FaceAxis, FaceDirection, TempChunkMeshData, VoxelInstance};
pub type ChunkData = LinearArray;


#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct OccupancyArray {
    count: u32,         // total number of set bits
    pub bits: [u32; 1024], // represents a 32x32x32 bit array, linearized to i|j<<5|k<<10, 15 bits
}

impl OccupancyArray {
    #[inline(always)]
    pub const fn default() -> Self {
        Self { 
            bits: [0u32; 1024],
            count: 0,
        }
    }    
    #[inline(always)]
    pub const fn new(bits: [u32;1024], count:u32) -> Self {
        Self { 
            bits,
            count
        }
    }
    
    #[inline(always)]
    pub const fn full() -> Self {
        Self { 
            bits: [u32::MAX; 1024],
            count: 32768, // 32 * 1024 = total bits
        }
    }
    
    #[inline(always)]
    pub const fn count(&self) -> u32 {
        self.count
    }
    
    #[inline(always)]
    pub const fn set_from_index(&mut self, idx: usize) {
        let word = idx >> 5;
        let bit = idx & 31;
        let mask = 1 << bit;
        let old_word = self.bits[word];
        self.bits[word] |= mask;
        // Only increment if bit wasn't already set
        if old_word & mask == 0 {
            self.count += 1;
        }
    }
    #[inline(always)] 
    pub const fn clear_from_index(&mut self, idx: usize) {
        let word = idx >> 5; 
        let bit = idx & 31; 
        let mask = 1 << bit; 
        let old_word = self.bits[word]; 
        self.bits[word] &= !mask; // Only decrement if bit was set
        if old_word & mask != 0 { self.count -= 1; }
    }
        
    #[inline(always)]
    pub const fn clear_from_word_bit(&mut self, word: usize, bit: usize) {
        let mask = 1 << bit;
        let old_word = self.bits[word];
        self.bits[word] &= !mask;
        // Only decrement if bit was set
        if old_word & mask != 0 {
            self.count -= 1;
        }
    }

    
    #[inline(always)]
    pub const fn check_index(&self, idx: usize) -> bool {
        (self.bits[idx >> 5] >> (idx & 31)) & 1 == 1
    }
    #[inline(always)]
    pub const fn check_bit_from_coord(&self, word: usize, bit:usize) -> bool {
        (self.bits[word]>>bit)&1 == 1
    }

    #[inline(always)]
    pub const fn read_word(&self, word_idx: usize) -> u32 {
        self.bits[word_idx]
    }

    #[inline(always)]
    pub const fn read_bit_from_index(&self, idx: usize) -> bool {
        (self.bits[idx>>5]>>(idx&31))&1 == 1
    }

    
    #[inline(always)]
    pub const fn is_full(&self) -> bool {
        self.count == 32768
    }
    
    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    #[inline(always)]
    pub fn find_first_set_bit(&self) -> Option<u32> {
        if self.count == 0 {
            return None;
        }
        
        for (word_idx, &word) in self.bits.iter().enumerate() {
            if word != 0 {
                let bit_pos = word.trailing_zeros();
                return Some((word_idx << 5) as u32 + bit_pos);
            }
        }
        None
    }

    #[inline(always)]
    pub fn find_first_non_zero_word(&self) -> u32 {
        if self.count == 0 {
            return 0;
        }
        
        for (word_idx, &word) in self.bits.iter().enumerate() {
            if word != 0 {
                return word_idx as u32;
            }
        }
        0
    }
}
#[inline(always)]
const fn low_bits_mask(n: u32) -> u32 {
    match n {
        0 => 0,
        32..=u32::MAX => u32::MAX,
        n => (1u32 << n) - 1,
    }
}
impl OccupancyArray {

    #[inline(always)]
    pub fn filled_to_height(height: u32) -> Self {
        let pattern = low_bits_mask(height);
        let count = pattern << 10;
        OccupancyArray{bits:[pattern;1024], count}
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Hash, Component)]
pub struct LinearArray {
    /// by default, all indices are linearized as a 32x32x32 space where index = y|x<<5|z<<10
    /// since it is linearized, you should never have any excuse to use raw coordinates as an interface; just iterate from 0-32768, y changes every 1 step, x changes every 32, z changes every 1024.
    pub data: [u8; 32768], //flattened [[u8;32];1024]. each u8 value represents a voxel material. 0 = Air, 1 = dirt, etc
    pub occupancy: OccupancyArray,
}

impl LinearArray {
    /// Create a LinearArray from an OccupancyArray with a fixed material
    #[inline(always)]
    pub fn from_occupancy(occupancy: &OccupancyArray, material: u8) -> Self {
        let mut data = [0u8; 32768];
        
        // Iterate through each word
        for word_idx in 0..1024 {
            let mut word = occupancy.bits[word_idx];
            if word == 0 { continue; }
            
            // Process each set bit in this word
            while word != 0 {
                let bit = word.trailing_zeros() as usize;
                let idx = (word_idx << 5) | bit;
                data[idx] = material;
                word &= word - 1; // Clear the lowest set bit
            }
        }
        
        Self {
            data,
            occupancy: occupancy.clone(),
        }
    }
    
    /// Create a LinearArray from an OccupancyArray with a closure to determine material
    /// The closure receives (x, y, z) coordinates and returns the material
    #[inline(always)]
    pub fn from_occupancy_with<F>(occupancy: &OccupancyArray, mut material_fn: F) -> Self 
    where
        F: FnMut(usize, usize, usize) -> u8
    {
        let mut data = [0u8; 32768];
        
        // Iterate through each word
        for word_idx in 0..1024 {
            let mut word = occupancy.bits[word_idx];
            if word == 0 { continue; }
            
            // Pre-calculate z and x for this word
            let z = word_idx >> 5;
            let x = word_idx & 31;
            
            // Process each set bit in this word
            while word != 0 {
                let bit = word.trailing_zeros() as usize;
                let y = bit;
                let idx = (word_idx << 5) | bit;
                
                data[idx] = material_fn(x, y, z);
                word &= word - 1; // Clear the lowest set bit
            }
        }
        
        Self {
            data,
            occupancy: occupancy.clone(),
        }
    }
}
impl LinearArray{
    // uniform array creation
    #[inline(always)]
    pub const fn uniform(value: u8) -> Self{
        Self {
            data: [value;32768],
            occupancy: OccupancyArray::full()
        }
    }
    // empty array creation
    #[inline(always)]
    pub const fn default() -> Self{
        Self {
            data: [0;32768],
            occupancy: OccupancyArray::default()
        }
    }
    // alternating between 1 and 0 (3D checkerboard)
    #[inline(always)]
    pub fn test_checkerboard(complement: bool) -> Self {
        let mut data = [0u8; 32768];
        let mut bits = [0u32; 1024];
        let mut count = 0;

        for z in 0..32 {
            for x in 0..32 {
                for y in 0..32 {
                    let idx = y | (x << 5) | (z << 10); // linear index
                    
                    let mut val = ((x ^ y ^ z) & 1) as u8;
                    if complement {
                        val ^= 1; // flip 0 ↔ 1
                    }

                    data[idx] = val;

                    if val != 0 {
                        let word = idx >> 5;
                        let bit = idx & 31;
                        bits[word] |= 1 << bit;
                        count += 1;
                    }
                }
            }
        }

        Self {
            data,
            occupancy: OccupancyArray { bits, count },
        }
    }

    // odd y coordinates = 1, even y coordinates = 0 → horizontal planes
    #[inline(always)]
    pub const fn test_y_banded() -> Self {
        let mut data = [0u8; 32768];
        let mut bits = [0u32; 1024];
        let mut i = 0;
        let mut count = 0;

        while i < 32768 {
            let y = i & 31;
            let val = (y & 1) as u8;

            data[i] = val;

            if val != 0 {
                let word = i >> 5;
                let bit = i & 31;
                bits[word] |= 1 << bit;
                count += 1;
            }

            i += 1;
        }

        Self {
            data,
            occupancy: OccupancyArray { bits, count },
        }
    }

    // odd x coordinates = 1, even x coordinates = 0 → vertical bands in X
    #[inline(always)]
    pub const fn test_x_banded() -> Self {
        let mut data = [0u8; 32768];
        let mut bits = [0u32; 1024];
        let mut i = 0;
        let mut count = 0;

        while i < 32768 {
            let x = (i >> 5) & 31;
            let val = (x & 1) as u8;

            data[i] = val;

            if val != 0 {
                let word = i >> 5;
                let bit = i & 31;
                bits[word] |= 1 << bit;
                count += 1;
            }

            i += 1;
        }

        Self {
            data,
            occupancy: OccupancyArray { bits, count },
        }
    }

    // odd z coordinates = 1, even z coordinates = 0 → vertical bands in Z
    #[inline(always)]
    pub const fn test_z_banded() -> Self {
        let mut data = [0u8; 32768];
        let mut bits = [0u32; 1024];
        let mut i = 0;
        let mut count = 0;

        while i < 32768 {
            let z = (i >> 10) & 31;
            let val = (z & 1) as u8;

            data[i] = val;

            if val != 0 {
                let word = i >> 5;
                let bit = i & 31;
                bits[word] |= 1 << bit;
                count += 1;
            }

            i += 1;
        }

        Self {
            data,
            occupancy: OccupancyArray { bits, count },
        }
    }

        // Hollow cube - only walls, interior is empty
    #[inline(always)]
    pub fn test_hollow_cube(wall_thickness: usize) -> Self {
        let mut data = [0u8; 32768];
        let mut bits = [0u32; 1024];
        let mut count = 0;

        for z in 0..32 {
            for x in 0..32 {
                for y in 0..32 {
                    let is_edge = x < wall_thickness || x >= 32 - wall_thickness ||
                                 y < wall_thickness || y >= 32 - wall_thickness ||
                                 z < wall_thickness || z >= 32 - wall_thickness;
                    
                    if is_edge {
                        let idx = y | (x << 5) | (z << 10);
                        data[idx] = 1;
                        let word = idx >> 5;
                        let bit = idx & 31;
                        bits[word] |= 1 << bit;
                        count += 1;
                    }
                }
            }
        }

        Self {
            data,
            occupancy: OccupancyArray { bits, count },
        }
    }

    // Sphere centered in chunk
    #[inline(always)]
    pub fn test_sphere(radius: f32, material: u8) -> Self {
        let mut data = [0u8; 32768];
        let mut bits = [0u32; 1024];
        let mut count = 0;
        let center = 15.5f32;

        for z in 0..32 {
            for x in 0..32 {
                for y in 0..32 {
                    let dx = x as f32 - center;
                    let dy = y as f32 - center;
                    let dz = z as f32 - center;
                    let dist_sq = dx * dx + dy * dy + dz * dz;
                    
                    if dist_sq <= radius * radius {
                        let idx = y | (x << 5) | (z << 10);
                        data[idx] = material;
                        let word = idx >> 5;
                        let bit = idx & 31;
                        bits[word] |= 1 << bit;
                        count += 1;
                    }
                }
            }
        }

        Self {
            data,
            occupancy: OccupancyArray { bits, count },
        }
    }

    // Sparse random pattern - low fill ratio
    #[inline(always)]
    pub fn test_sparse_random(seed: u32, fill_ratio: f32) -> Self {
        let mut data = [0u8; 32768];
        let mut bits = [0u32; 1024];
        let mut count = 0;
        let mut state = seed;

        #[inline(always)]
        fn xorshift32(state: &mut u32) -> u32 {
            let mut x = *state;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *state = x;
            x
        }

        let threshold = (fill_ratio * u32::MAX as f32) as u32;

        for idx in 0..32768 {
            if xorshift32(&mut state) < threshold {
                data[idx] = 1;
                let word = idx >> 5;
                let bit = idx & 31;
                bits[word] |= 1 << bit;
                count += 1;
            }
        }

        Self {
            data,
            occupancy: OccupancyArray { bits, count },
        }
    }

    // Dense random with multiple materials
    #[inline(always)]
    pub fn test_dense_random(seed: u32, fill_ratio: f32, material_count: u8) -> Self {
        let mut data = [0u8; 32768];
        let mut bits = [0u32; 1024];
        let mut count = 0;
        let mut state = seed;

        #[inline(always)]
        fn xorshift32(state: &mut u32) -> u32 {
            let mut x = *state;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *state = x;
            x
        }

        let threshold = (fill_ratio * u32::MAX as f32) as u32;

        for idx in 0..32768 {
            if xorshift32(&mut state) < threshold {
                let material = ((xorshift32(&mut state) as u8) % material_count) + 1;
                data[idx] = material;
                let word = idx >> 5;
                let bit = idx & 31;
                bits[word] |= 1 << bit;
                count += 1;
            }
        }

        Self {
            data,
            occupancy: OccupancyArray { bits, count },
        }
    }

    // Vertical pillars at regular spacing
    #[inline(always)]
    pub fn test_pillars(spacing: usize, material: u8) -> Self {
        let mut data = [0u8; 32768];
        let mut bits = [0u32; 1024];
        let mut count = 0;

        for z in (0..32).step_by(spacing) {
            for x in (0..32).step_by(spacing) {
                for y in 0..32 {
                    let idx = y | (x << 5) | (z << 10);
                    data[idx] = material;
                    let word = idx >> 5;
                    let bit = idx & 31;
                    bits[word] |= 1 << bit;
                    count += 1;
                }
            }
        }

        Self {
            data,
            occupancy: OccupancyArray { bits, count },
        }
    }

    // 3D noise-like pattern using simple hash function
    #[inline(always)]
    pub fn test_noise_3d(seed: u32, threshold: f32, scale: f32) -> Self {
        let mut data = [0u8; 32768];
        let mut bits = [0u32; 1024];
        let mut count = 0;

        // Simple 3D hash function for noise-like values
        #[inline(always)]
        fn hash3d(x: u32, y: u32, z: u32, seed: u32) -> f32 {
            let mut h = seed;
            h ^= x.wrapping_mul(374761393);
            h ^= y.wrapping_mul(668265263);
            h ^= z.wrapping_mul(1274126177);
            h ^= h >> 13;
            h = h.wrapping_mul(1274126177);
            h ^= h >> 16;
            (h as f32) / (u32::MAX as f32)
        }

        let inv_scale = 1.0 / scale;

        for z in 0..32 {
            for x in 0..32 {
                for y in 0..32 {
                    // Sample noise at scaled coordinates
                    let nx = (x as f32 * inv_scale) as u32;
                    let ny = (y as f32 * inv_scale) as u32;
                    let nz = (z as f32 * inv_scale) as u32;
                    
                    let noise_val = hash3d(nx, ny, nz, seed);
                    
                    if noise_val > threshold {
                        let idx = y | (x << 5) | (z << 10);
                        data[idx] = 1;
                        let word = idx >> 5;
                        let bit = idx & 31;
                        bits[word] |= 1 << bit;
                        count += 1;
                    }
                }
            }
        }

        Self {
            data,
            occupancy: OccupancyArray { bits, count },
        }
    }
}


impl LinearArray {
    #[inline(always)]
    pub const fn new() -> Self {
        Self { 
            data: [0; 32768], 
            occupancy: OccupancyArray::default() 
        }
    }
    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.occupancy.count == 0
    }
    #[inline(always)]
    pub const fn count(&self) -> u32 {
        self.occupancy.count
    }
    #[inline(always)]
    pub const fn set(&mut self, index: usize, value: u8) {
        let old = self.data[index];
        if old != value {
            self.data[index] = value;
            
            // Update occupancy array
            match (old != 0, value != 0) {
                (false, true) => {
                    // Air → Solid: set bit
                    self.occupancy.set_from_index(index);
                }
                (true, false) => {
                    // Solid → Air: clear bit
                    self.occupancy.clear_from_index(index);
                }
                // (false, false) and (true, true) don't change occupancy
                _ => {}
            }
        }
    }
    #[inline(always)]
    pub const fn filled(value:u8) -> Self {
        Self { data: [value;32768], occupancy: OccupancyArray::full() }
    }
}
impl LinearArray {
    /// Check if array is uniform (all same non-zero material)
    #[inline(always)]
    pub fn is_uniform(&self) -> Option<u8> {
        if self.occupancy.is_empty() {
            return Some(0);
        }
        
        let first = self.data[0];
        let expected_u64 = u64::from_ne_bytes([first; 8]);
        
        // Process 8 bytes at a time
        let chunks = self.data.chunks_exact(8);
        let remainder = chunks.remainder();
        
        // Early termination on first mismatch
        for chunk in chunks {
            let chunk_bytes: [u8; 8] = chunk.try_into().unwrap();
            let chunk_u64 = u64::from_ne_bytes(chunk_bytes);
            if chunk_u64 != expected_u64 {
                return None;
            }
        }
        
        // Handle remaining bytes (0-7 bytes)
        for &byte in remainder {
            if byte != first {
                return None;
            }
        }
        
        Some(first)
    }
}
#[inline(always)]
pub fn build_mesh(data: &LinearArray) -> [Vec<VoxelInstance>; 6] {
    // Early exit for completely uniform arrays
    if let Some(material) = data.is_uniform() {
        if material == 0 {
            return [
                Vec::new(), Vec::new(), Vec::new(),
                Vec::new(), Vec::new(), Vec::new(),
            ];
        }
        let maxi = VoxelInstance::max_instance(FaceDirection::YP, material);
        // VoxelInstance is Copy so we can reuse the same value
        return [
            vec![maxi], vec![maxi], vec![maxi],
            vec![maxi], vec![maxi], vec![maxi],
        ];
    }

    // produce Option<Vec<...>>;6 from existing pipeline
    let [a, b, c, d, e, f] = {
        let faces = get_all_faces_simple(&data.occupancy.bits, &[]);
        let first_pass = primary_pass(faces, &data);
        secondary_pass(first_pass)
    };
    [
        a.unwrap_or_default(),
        b.unwrap_or_default(),
        c.unwrap_or_default(),
        d.unwrap_or_default(),
        e.unwrap_or_default(),
        f.unwrap_or_default(),
    ]
}

#[inline(always)]
pub fn build_mesh_into(data: &LinearArray, exclude_faces: &[FaceDirection], out: &mut TempChunkMeshData) {
    // Early exit for completely uniform arrays
    out.clear();
    if let Some(material) = data.is_uniform() {
        if material == 0 {
            return 
        }
        // VoxelInstance is Copy so we can reuse the same value
        out.push_face(FaceDirection::YP, VoxelInstance::max_instance(FaceDirection::YP, material));
        out.push_face(FaceDirection::YN, VoxelInstance::max_instance(FaceDirection::YN, material));
        out.push_face(FaceDirection::XP, VoxelInstance::max_instance(FaceDirection::XP, material));
        out.push_face(FaceDirection::XN, VoxelInstance::max_instance(FaceDirection::XN, material));
        out.push_face(FaceDirection::ZP, VoxelInstance::max_instance(FaceDirection::ZP, material));
        out.push_face(FaceDirection::ZN, VoxelInstance::max_instance(FaceDirection::ZN, material));
    }

    // produce Option<Vec<...>>;6 from existing pipeline
    let [a, b, c, d, e, f] = {
        let faces = get_all_faces_simple(&data.occupancy.bits, exclude_faces);
        let first_pass = primary_pass(faces, &data);
        secondary_pass(first_pass)
    };
    out.data=[
        a.unwrap_or_default(),
        b.unwrap_or_default(),
        c.unwrap_or_default(),
        d.unwrap_or_default(),
        e.unwrap_or_default(),
        f.unwrap_or_default(),
    ]
}

/// Get all 6 face directions by calling get_faces individually
/// This should perform close to the individual call timing (~1.5µs)
/// Compute all 6 faces in a single pass - should be much faster than 6 separate calls

#[inline(always)]
pub fn get_all_faces_simple(occupancy_array: &[u32;1024], exclude: &[FaceDirection]) -> [Option<OccupancyArray>; 6] {
    let mut excluded = 0u8;
    for &face in exclude {
        excluded |= 1 << (face as u8);
    }
    
    // Initialize as None if excluded, Some(empty) if included
    let mut faces: [Option<OccupancyArray>; 6] = [
        if excluded & (1 << 0) == 0 { Some(OccupancyArray::default()) } else { None },
        if excluded & (1 << 1) == 0 { Some(OccupancyArray::default()) } else { None },
        if excluded & (1 << 2) == 0 { Some(OccupancyArray::default()) } else { None },
        if excluded & (1 << 3) == 0 { Some(OccupancyArray::default()) } else { None },
        if excluded & (1 << 4) == 0 { Some(OccupancyArray::default()) } else { None },
        if excluded & (1 << 5) == 0 { Some(OccupancyArray::default()) } else { None },
    ];
    
    for i in 0..1024 {
        let word = occupancy_array[i];
        let z = i >> 5;
        let x = i & 31;
        
        if let Some(ref mut face) = faces[FaceDirection::YP as usize] {
            let r = word & !(word >> 1);
            face.bits[i] = r;
            face.count += r.count_ones();
        }
        if let Some(ref mut face) = faces[FaceDirection::YN as usize] {
            let r = word & !(word << 1);
            face.bits[i] = r;
            face.count += r.count_ones();
        }
        if let Some(ref mut face) = faces[FaceDirection::XN as usize] {
            let r = word & !(if x != 0 { occupancy_array[i - 1] } else { 0 });
            face.bits[i] = r;
            face.count += r.count_ones();
        }
        if let Some(ref mut face) = faces[FaceDirection::XP as usize] {
            let r = word & !(if x != 31 { occupancy_array[i + 1] } else { 0 });
            face.bits[i] = r;
            face.count += r.count_ones();
        }
        if let Some(ref mut face) = faces[FaceDirection::ZN as usize] {
            let r = word & !(if z != 0 { occupancy_array[i - 32] } else { 0 });
            face.bits[i] = r;
            face.count += r.count_ones();
        }
        if let Some(ref mut face) = faces[FaceDirection::ZP as usize] {
            let r = word & !(if z != 31 { occupancy_array[i + 32] } else { 0 });
            face.bits[i] = r;
            face.count += r.count_ones();
        }
    }
    
    faces
}
#[test]
fn benchmark_get_all_faces_simple() {
    use std::time::Instant;
    use std::hint::black_box;

    const ITERATIONS: u32 = 1_000;

    println!("\n{:30} {:>10}", "Component", "Avg Time");
    println!("{}", "-".repeat(45));

    // Use a non-trivial pattern (sphere) so face extraction does real work
    let data = LinearArray::test_sphere(12.0, 1);
    let occ = &data.occupancy.bits;

    // Warmup
    black_box(get_all_faces_simple(black_box(occ), &[]));

    // Benchmark get_all_faces_simple alone
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        black_box(get_all_faces_simple(black_box(occ), &[]));
    }
    let faces_time = start.elapsed() / ITERATIONS;
    println!("{:30} {:>10?}", "get_all_faces_simple", faces_time);

    // Compare with full build_mesh_into for context
    let mut temp = TempChunkMeshData::default();
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        build_mesh_into(black_box(&data), &[], black_box(&mut temp));
    }
    let full_time = start.elapsed() / ITERATIONS;
    println!("{:30} {:>10?}", "Full pipeline", full_time);
}

#[inline(always)]
pub fn dgm_y(mut bits: OccupancyArray, data: &LinearArray) -> [Vec<VoxelInstance>;32] {
    let mut instances: [Vec<VoxelInstance>; 32] = [
        Vec::new(), Vec::new(), Vec::new(), Vec::new(),
        Vec::new(), Vec::new(), Vec::new(), Vec::new(),
        Vec::new(), Vec::new(), Vec::new(), Vec::new(),
        Vec::new(), Vec::new(), Vec::new(), Vec::new(),
        Vec::new(), Vec::new(), Vec::new(), Vec::new(),
        Vec::new(), Vec::new(), Vec::new(), Vec::new(),
        Vec::new(), Vec::new(), Vec::new(), Vec::new(),
        Vec::new(), Vec::new(), Vec::new(), Vec::new(),
    ];
    
    for x in 0..32 {
        let x_shift5 = x << 5;
        for z in 0..32 {
            let z_shift10 = z << 10;
            let z_shift5 = z << 5;
            let xz = x + z_shift5;
            let mut word = bits.bits[xz];
            while word != 0 {
                let bit_y = word.trailing_zeros() as usize;
                let lsb_mask = 1u32 << bit_y;
                let idx = x_shift5 | z_shift10 | bit_y;
                let current_material = data.data[idx];
                let mut current = VoxelInstance::from_material(idx as u32, current_material);
                
                let mut next_xz = xz + 1;
                for i in (x+1)..32 {
                    if bits.bits[next_xz] & lsb_mask == 0 {
                        break;
                    }
                    let next_idx = (i << 5) | z_shift10 | bit_y;
                    if data.data[next_idx] != current_material {
                        break;
                    }
                    current.inc_primary_unchecked();
                    bits.bits[next_xz] &= !(lsb_mask);
                    next_xz += 1;
                }
                instances[bit_y].push(current);
                word &= word - 1;
            }
            bits.bits[xz] = word;
        }
    }
    instances
}

#[inline(always)]
pub fn dgm_x(mut bits: OccupancyArray, data: &LinearArray) -> [Vec<VoxelInstance>;32] {
    let mut instances: [Vec<VoxelInstance>; 32] = std::array::from_fn(|_| Vec::new());
    
    // Swap: iterate Z outer, X inner for sequential word access
    for z in 0..32 {
        let z_shift10 = z << 10;
        let z_shift5 = z << 5;
        
        for x in 0..32 {
            let x_shift5 = x << 5;
            let xz = x + z_shift5;
            let mut word = bits.bits[xz];
            
            while word != 0 {
                let bit_y = word.trailing_zeros() as usize;
                let idx = bit_y | x_shift5 | z_shift10;
                let current_material = data.data[idx];
                let mut current = VoxelInstance::from_material(idx as u32, current_material);
                word &= word - 1;
                
                let mut last_bit = bit_y;
                while word != 0 {
                    let next_bit = word.trailing_zeros() as usize;
                    if next_bit != last_bit + 1 { break; }
                    
                    let next_idx = next_bit | x_shift5 | z_shift10;
                    if data.data[next_idx] != current_material { break; }
                    
                    current.inc_primary_unchecked();
                    word &= word - 1;
                    last_bit = next_bit;
                }
                
                instances[x].push(current);  // Still organize by X
            }
            bits.bits[xz] = word;
        }
    }
    instances
}

#[inline(always)]
pub fn dgm_z(mut bits: OccupancyArray, data: &LinearArray) -> [Vec<VoxelInstance>;32] {
    let mut instances: [Vec<VoxelInstance>; 32] = std::array::from_fn(|_| Vec::new());
    
    for (z, chunk) in bits.bits.chunks_exact_mut(32).enumerate() {
        let z_shift10 = z << 10;
        
        for (x, word) in chunk.iter_mut().enumerate() {
            let x_shift5 = x << 5;
            
            while *word != 0 {
                let bit_y = word.trailing_zeros() as usize;
                let idx = bit_y | x_shift5 | z_shift10;
                let current_material = data.data[idx];
                let mut current = VoxelInstance::from_material(idx as u32, current_material);
                *word &= *word - 1;  // Clear bit_y
                
                let mut last_bit = bit_y;
                while *word != 0 {
                    let next_bit = word.trailing_zeros() as usize;
                    if next_bit != last_bit + 1 { break; }  // Not consecutive
                    
                    let next_idx = next_bit | x_shift5 | z_shift10;
                    if data.data[next_idx] != current_material { break; }
                    
                    current.inc_primary_unchecked();
                    *word &= *word - 1;  // Clear this bit
                    last_bit = next_bit;
                }
                
                instances[z].push(current);
            }
        }
    }
    instances
}

#[inline(always)]
fn secondary_pass_logic(
    instances: Option<[Vec<VoxelInstance>;32]>,
    axis: FaceAxis
) -> Option<Vec<VoxelInstance>> {
    let mut instances = instances?;

    for slice in instances.iter_mut() {
        if slice.len() <= 1 { continue; }
        
        let mut result = Vec::with_capacity(slice.len());
        let mut i = 0;
        
        while i < slice.len() {
            let mut current = slice[i];
            i += 1;
            
            while i < slice.len() {
                let candidate = slice[i];
                let (merged, head, tail) = current.try_merge_secondary(&candidate, axis);
                
                if !merged {
                    break;
                }
                
                // Handle splits
                match (head, tail) {
                    (None, None) => {
                        // Fully merged into current
                        i += 1;
                    }
                    (Some(h), None) | (None, Some(h)) => {
                        // One remainder - push it for later consideration
                        result.push(h);
                        i += 1;
                    }
                    (Some(h), Some(t)) => {
                        // Two pieces - push both
                        result.push(h);
                        result.push(t);
                        i += 1;
                    }
                }
            }
            
            result.push(current);
        }
        
        *slice = result;
    }

    let mut result = std::mem::take(&mut instances[0]);
    for slice in &mut instances[1..] {
        result.append(slice);
    }

    Some(result)
}

#[inline(always)]
pub fn primary_pass(mut faces: [Option<OccupancyArray>;6], arr: &LinearArray) -> [Option<[Vec<VoxelInstance>;32]>;6] {
    [ 
    faces[0].take().map(|f| dgm_y(f, &arr)),
    faces[1].take().map(|f| dgm_y(f, &arr)),
    faces[2].take().map(|f| dgm_x(f, &arr)),
    faces[3].take().map(|f| dgm_x(f, &arr)),
    faces[4].take().map(|f| dgm_z(f, &arr)),
    faces[5].take().map(|f| dgm_z(f, &arr)),
    ]
}

#[inline(always)]
pub fn secondary_pass(
    instances: [Option<[Vec<VoxelInstance>;32]>;6]
) -> [Option<Vec<VoxelInstance>>;6] {
    let mut out: [Option<Vec<VoxelInstance>>; 6] = Default::default();

    for (i, inst) in instances.into_iter().enumerate() {
        out[i] = secondary_pass_logic(inst, FaceAxis::from_usize(i));
    }

    out
}

#[cfg(test)]
mod mesh_benchmarks {
    use super::*;
    use std::time::Instant;
    use std::hint::black_box;

    const ITERATIONS: u32 = 1_000;

    fn benchmark_test<F>(name: &str, mut setup: F, iterations: u32) 
    where 
        F: FnMut() -> LinearArray 
    {
        let data = setup();
        let mut temp = TempChunkMeshData::default();
        
        // Warmup
        for _ in 0..10 {
            build_mesh_into(black_box(&data), &[], black_box(&mut temp));
        }
        
        let start = Instant::now();
        for _ in 0..iterations {
            build_mesh_into(black_box(&data), &[], black_box(&mut temp));
        }
        let elapsed = start.elapsed();
        
        let avg_time = elapsed / iterations;
        let total_instances: usize = temp.data.iter().map(|v| v.len()).sum();
        
        println!("{:30} {:>10?}  ({} instances)", 
            name, avg_time, total_instances);
    }

    #[test]
    fn benchmark_all_patterns() {
        println!("\n{:30} {:>10}  {}", "Pattern", "Avg Time", "Output");
        println!("{}", "-".repeat(60));
        
        benchmark_test("Empty (all air)", 
            || LinearArray::default(), 
            ITERATIONS);
        
        benchmark_test("Uniform (solid stone)", 
            || LinearArray::uniform(1), 
            ITERATIONS);
        
        benchmark_test("Checkerboard (worst case)", 
            || LinearArray::test_checkerboard(false), 
            ITERATIONS);
        
        benchmark_test("Checkerboard complement", 
            || LinearArray::test_checkerboard(true), 
            ITERATIONS);
        
        benchmark_test("Y-banded (horizontal planes)", 
            || LinearArray::test_y_banded(), 
            ITERATIONS);
        
        benchmark_test("X-banded (vertical slices)", 
            || LinearArray::test_x_banded(), 
            ITERATIONS);
        
        benchmark_test("Z-banded (vertical slices)", 
            || LinearArray::test_z_banded(), 
            ITERATIONS);
        
        benchmark_test("Hollow cube (1 voxel walls)", 
            || LinearArray::test_hollow_cube(1), 
            ITERATIONS);
        
        benchmark_test("Hollow cube (2 voxel walls)", 
            || LinearArray::test_hollow_cube(2), 
            ITERATIONS);
        
        benchmark_test("Sphere (radius 8)", 
            || LinearArray::test_sphere(8.0, 1), 
            ITERATIONS);
        
        benchmark_test("Sphere (radius 15)", 
            || LinearArray::test_sphere(15.0, 1), 
            ITERATIONS);
        
        benchmark_test("Sparse random (10% fill)", 
            || LinearArray::test_sparse_random(12345, 0.1), 
            ITERATIONS);
        
        benchmark_test("Sparse random (25% fill)", 
            || LinearArray::test_sparse_random(12345, 0.25), 
            ITERATIONS);
        
        benchmark_test("Sparse random (50% fill)", 
            || LinearArray::test_sparse_random(12345, 0.5), 
            ITERATIONS);
        
        benchmark_test("Sparse random (75% fill)", 
            || LinearArray::test_sparse_random(12345, 0.75), 
            ITERATIONS);
        
        benchmark_test("Dense random (90%, 4 materials)", 
            || LinearArray::test_dense_random(54321, 0.9, 4), 
            ITERATIONS);
        
        benchmark_test("Pillars (spacing 4)", 
            || LinearArray::test_pillars(4, 1), 
            ITERATIONS);
        
        benchmark_test("Pillars (spacing 8)", 
            || LinearArray::test_pillars(8, 1), 
            ITERATIONS);
        
        benchmark_test("3D Noise (threshold 0.3, scale 4)", 
            || LinearArray::test_noise_3d(99999, 0.3, 4.0), 
            ITERATIONS);
        
        benchmark_test("3D Noise (threshold 0.5, scale 8)", 
            || LinearArray::test_noise_3d(99999, 0.5, 8.0), 
            ITERATIONS);
    }
    
    #[test]
    fn benchmark_face_exclusion() {
        println!("\n{:30} {:>10}  {}", "Exclusion Pattern", "Avg Time", "Note");
        println!("{}", "-".repeat(60));
        
        let data = LinearArray::test_checkerboard(false);
        let mut temp = TempChunkMeshData::default();
        
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            build_mesh_into(black_box(&data), &[], black_box(&mut temp));
        }
        println!("{:30} {:>10?}  All 6 faces", 
            "No exclusions", start.elapsed() / ITERATIONS);
        
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            build_mesh_into(black_box(&data), &[FaceDirection::YP], black_box(&mut temp));
        }
        println!("{:30} {:>10?}  5 faces", 
            "Exclude YP", start.elapsed() / ITERATIONS);
        
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            build_mesh_into(black_box(&data), 
                &[FaceDirection::YP, FaceDirection::YN, FaceDirection::XP], 
                black_box(&mut temp));
        }
        println!("{:30} {:>10?}  3 faces", 
            "Exclude YP/YN/XP", start.elapsed() / ITERATIONS);
    }
    
    #[test]
    fn benchmark_component_breakdown() {
        println!("\n{:30} {:>10}", "Component", "Avg Time");
        println!("{}", "-".repeat(45));
        
        let data = LinearArray::test_sphere(12.0, 1);
        let mut temp = TempChunkMeshData::default();
        
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            build_mesh_into(black_box(&data), &[], black_box(&mut temp));
        }
        let full_time = start.elapsed() / ITERATIONS;
        println!("{:30} {:>10?}", "Full pipeline", full_time);
        
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            black_box(get_all_faces_simple(black_box(&data.occupancy.bits), &[]));
        }
        let face_time = start.elapsed() / ITERATIONS;
        println!("{:30} {:>10?}", "Face extraction", face_time);
        
        let faces = get_all_faces_simple(&data.occupancy.bits, &[]);
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            black_box(primary_pass(black_box(faces), black_box(&data)));
        }
        let primary_time = start.elapsed() / ITERATIONS;
        println!("{:30} {:>10?}", "Primary pass (1D greedy)", primary_time);
        
        let primary_result = primary_pass(faces, &data);
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            black_box(secondary_pass(primary_result.clone()));
        }
        let secondary_time = start.elapsed() / ITERATIONS;
        println!("{:30} {:>10?}", "Secondary pass (2D merge)", secondary_time);
    }
    
    #[test]
    fn benchmark_scaling() {
        println!("\n{:30} {:>10}  {:>10}", "Fill Ratio", "Avg Time", "Instances");
        println!("{}", "-".repeat(55));
        
        let mut temp = TempChunkMeshData::default();
        
        for fill_pct in [5, 10, 20, 30, 40, 50, 60, 70, 80, 90, 95, 99] {
            let fill_ratio = fill_pct as f32 / 100.0;
            let data = LinearArray::test_sparse_random(42, fill_ratio);
            
            let start = Instant::now();
            for _ in 0..ITERATIONS {
                build_mesh_into(black_box(&data), &[], black_box(&mut temp));
            }
            let avg_time = start.elapsed() / ITERATIONS;
            let total_instances: usize = temp.data.iter().map(|v| v.len()).sum();
            
            println!("{:>3}% fill {:20} {:>10?}  {:>10}", 
                fill_pct, "", avg_time, total_instances);
        }
    }
}
