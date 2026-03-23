//voxels/src/sdf.rs
use bevy::prelude::*;

// ============================================================================
// PACKED PRIMITIVE (Single u32 - 4 bytes)
// ============================================================================

/// Layout: [type:8][param_a:5][param_b:5][param_c:5][scale:9]
/// - type: 8 bits (256 shape types)
/// - param_a/b/c: 5 bits each (0-31 range)
/// - scale: 9 bits [polarity:1][multiplier:8]
///   - polarity=0: shrink (1/multiplier), multiplier 1-256 → scales 1.0 to 1/256
///   - polarity=1: grow (multiplier), multiplier 1-256 → scales 1x to 256x
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PackedPrimitive(u32);

impl PackedPrimitive {
    const TYPE_SHIFT: u32 = 24;
    const PARAM_A_SHIFT: u32 = 19;
    const PARAM_B_SHIFT: u32 = 14;
    const PARAM_C_SHIFT: u32 = 9;
    const SCALE_SHIFT: u32 = 0;
    
    const TYPE_MASK: u32 = 0xFF << Self::TYPE_SHIFT;
    const PARAM_A_MASK: u32 = 0x1F << Self::PARAM_A_SHIFT;
    const PARAM_B_MASK: u32 = 0x1F << Self::PARAM_B_SHIFT;
    const PARAM_C_MASK: u32 = 0x1F << Self::PARAM_C_SHIFT;
    const SCALE_MASK: u32 = 0x1FF << Self::SCALE_SHIFT;
    
    #[inline(always)]
    fn new(shape_type: u8, a: u8, b: u8, c: u8, scale: u16) -> Self {
        debug_assert!(a < 32, "param_a must be 0-31");
        debug_assert!(b < 32, "param_b must be 0-31");
        debug_assert!(c < 32, "param_c must be 0-31");
        debug_assert!(scale < 512, "scale must be 0-511");
        
        Self(
            ((shape_type as u32) << Self::TYPE_SHIFT)
            | ((a as u32) << Self::PARAM_A_SHIFT)
            | ((b as u32) << Self::PARAM_B_SHIFT)
            | ((c as u32) << Self::PARAM_C_SHIFT)
            | ((scale as u32) << Self::SCALE_SHIFT)
        )
    }
    
    #[inline(always)]
    pub fn shape_type(&self) -> u8 {
        ((self.0 & Self::TYPE_MASK) >> Self::TYPE_SHIFT) as u8
    }
    
    #[inline(always)]
    pub fn param_a(&self) -> u8 {
        ((self.0 & Self::PARAM_A_MASK) >> Self::PARAM_A_SHIFT) as u8
    }
    
    #[inline(always)]
    pub fn param_b(&self) -> u8 {
        ((self.0 & Self::PARAM_B_MASK) >> Self::PARAM_B_SHIFT) as u8
    }
    
    #[inline(always)]
    pub fn param_c(&self) -> u8 {
        ((self.0 & Self::PARAM_C_MASK) >> Self::PARAM_C_SHIFT) as u8
    }
    
    #[inline(always)]
    pub fn scale(&self) -> f32 {
        let scale_raw = ((self.0 & Self::SCALE_MASK) >> Self::SCALE_SHIFT) as u16;
        let polarity = (scale_raw >> 8) & 1;  // Top bit (0=shrink, 1=grow)
        let multiplier = (scale_raw & 0xFF).max(1) as f32;  // Bottom 8 bits (1-256)
        
        if polarity == 0 {
            1.0 / multiplier  // Shrink: 1/mult
        } else {
            multiplier  // Grow: mult
        }
    }
    
    #[inline(always)]
    fn encode_scale(scale: f32) -> u16 {
        if scale < 1.0 {
            // Shrink: polarity=0, multiplier=1/scale
            let multiplier = (1.0 / scale).round().clamp(1.0, 256.0) as u16;
            multiplier  // Polarity bit is 0
        } else {
            // Grow: polarity=1, multiplier=scale
            let multiplier = scale.round().clamp(1.0, 256.0) as u16;
            (1 << 8) | multiplier
        }
    }
    
    // Constructors
    pub fn sphere(radius: u8, scale: f32) -> Self {
        Self::new(0, radius, 0, 0, Self::encode_scale(scale))
    }
    
    pub fn box_shape(x: u8, y: u8, z: u8, scale: f32) -> Self {
        Self::new(1, x, y, z, Self::encode_scale(scale))
    }
    
    pub fn cylinder(radius: u8, height: u8, scale: f32) -> Self {
        Self::new(2, radius, height, 0, Self::encode_scale(scale))
    }
    
    pub fn torus(major_radius: u8, minor_radius: u8, scale: f32) -> Self {
        Self::new(3, major_radius, minor_radius, 0, Self::encode_scale(scale))
    }
    
    pub fn capsule(radius: u8, height: u8, scale: f32) -> Self {
        Self::new(4, radius, height, 0, Self::encode_scale(scale))
    }
    
    pub fn cone(radius: u8, height: u8, scale: f32) -> Self {
        Self::new(5, radius, height, 0, Self::encode_scale(scale))
    }
    
    pub fn plane(height: u8, scale: f32) -> Self {
        Self::new(6, height, 0, 0, Self::encode_scale(scale))
    }
    
    /// Calculate signed distance to shape surface
    #[inline(always)]
    pub fn distance(&self, point: Vec3) -> f32 {
        let scale = self.scale();
        let scaled_point = if scale != 0.0 { point / scale } else { point };
        
        let dist = match self.shape_type() {
            0 => self.distance_sphere(scaled_point),
            1 => self.distance_box(scaled_point),
            2 => self.distance_cylinder(scaled_point),
            3 => self.distance_torus(scaled_point),
            4 => self.distance_capsule(scaled_point),
            5 => self.distance_cone(scaled_point),
            6 => self.distance_plane(scaled_point),
            _ => 0.0, // Unknown type
        };
        
        if scale != 0.0 { dist * scale } else { dist }
    }
    
    #[inline(always)]
    fn distance_sphere(&self, point: Vec3) -> f32 {
        point.length() - self.param_a() as f32
    }
    
    #[inline(always)]
    fn distance_box(&self, point: Vec3) -> f32 {
        let half_extents = Vec3::new(
            self.param_a() as f32,
            self.param_b() as f32,
            self.param_c() as f32,
        );
        let q = point.abs() - half_extents;
        q.max(Vec3::ZERO).length() + q.max_element().min(0.0)
    }
    
    #[inline(always)]
    fn distance_cylinder(&self, point: Vec3) -> f32 {
        let radius = self.param_a() as f32;
        let height = self.param_b() as f32;
        let d = Vec2::new(
            Vec2::new(point.x, point.z).length() - radius,
            point.y.abs() - height,
        );
        d.x.max(d.y).min(0.0) + d.max(Vec2::ZERO).length()
    }
    
    #[inline(always)]
    fn distance_torus(&self, point: Vec3) -> f32 {
        let major_radius = self.param_a() as f32;
        let minor_radius = self.param_b() as f32;
        let q = Vec2::new(
            Vec2::new(point.x, point.z).length() - major_radius,
            point.y,
        );
        q.length() - minor_radius
    }
    
    #[inline(always)]
    fn distance_capsule(&self, point: Vec3) -> f32 {
        let radius = self.param_a() as f32;
        let height = self.param_b() as f32;
        let clamped_y = point.y.clamp(-height, height);
        let to_segment = point - Vec3::new(0.0, clamped_y, 0.0);
        to_segment.length() - radius
    }
    
    #[inline(always)]
    fn distance_cone(&self, point: Vec3) -> f32 {
        let radius = self.param_a() as f32;
        let height = self.param_b() as f32;
        let c = Vec2::new(radius, height).normalize();
        let q = Vec2::new(Vec2::new(point.x, point.z).length(), point.y);
        let d = Vec2::new(q.dot(c), q.dot(Vec2::new(-c.y, c.x)));
        d.x.max(d.y).min(0.0) + d.max(Vec2::ZERO).length()
    }
    
    #[inline(always)]
    fn distance_plane(&self, point: Vec3) -> f32 {
        point.y - self.param_a() as f32
    }
}

// ============================================================================
// FULL SHAPE ENUM (For complex compositions)
// ============================================================================

#[derive(Clone, Debug)]
pub enum Shape {
    // Compact primitive
    Packed(PackedPrimitive),
    
    // Full precision primitives (for large shapes)
    Sphere { radius: f32 },
    Box { half_extents: Vec3 },
    Cylinder { radius: f32, height: f32 },
    Torus { major_radius: f32, minor_radius: f32 },
    Capsule { radius: f32, height: f32 },
    Cone { radius: f32, height: f32 },
    Plane { height: f32 },
    
    // Operations
    Union(Box<Shape>, Box<Shape>),
    Intersection(Box<Shape>, Box<Shape>),
    Subtraction(Box<Shape>, Box<Shape>),
    SmoothUnion { a: Box<Shape>, b: Box<Shape>, blend_radius: f32 },
    
    // Transformations
    Translate { shape: Box<Shape>, offset: Vec3 },
    Scale { shape: Box<Shape>, scale: f32 },
    RotateY { shape: Box<Shape>, angle: f32 },
}

impl Shape {
    pub fn distance(&self, point: Vec3) -> f32 {
        match self {
            Shape::Packed(packed) => packed.distance(point),
            
            Shape::Sphere { radius } => point.length() - radius,
            
            Shape::Box { half_extents } => {
                let q = point.abs() - *half_extents;
                q.max(Vec3::ZERO).length() + q.max_element().min(0.0)
            }
            
            Shape::Cylinder { radius, height } => {
                let d = Vec2::new(
                    Vec2::new(point.x, point.z).length() - radius,
                    point.y.abs() - height,
                );
                d.x.max(d.y).min(0.0) + d.max(Vec2::ZERO).length()
            }
            
            Shape::Torus { major_radius, minor_radius } => {
                let q = Vec2::new(
                    Vec2::new(point.x, point.z).length() - major_radius,
                    point.y,
                );
                q.length() - minor_radius
            }
            
            Shape::Capsule { radius, height } => {
                let clamped_y = point.y.clamp(-height, *height);
                let to_segment = point - Vec3::new(0.0, clamped_y, 0.0);
                to_segment.length() - radius
            }
            
            Shape::Cone { radius, height } => {
                let c = Vec2::new(*radius, *height).normalize();
                let q = Vec2::new(Vec2::new(point.x, point.z).length(), point.y);
                let d = Vec2::new(q.dot(c), q.dot(Vec2::new(-c.y, c.x)));
                d.x.max(d.y).min(0.0) + d.max(Vec2::ZERO).length()
            }
            
            Shape::Plane { height } => point.y - height,
            
            Shape::Union(a, b) => a.distance(point).min(b.distance(point)),
            Shape::Intersection(a, b) => a.distance(point).max(b.distance(point)),
            Shape::Subtraction(a, b) => a.distance(point).max(-b.distance(point)),
            
            Shape::SmoothUnion { a, b, blend_radius } => {
                let d1 = a.distance(point);
                let d2 = b.distance(point);
                let h = (0.5 + 0.5 * (d2 - d1) / blend_radius).clamp(0.0, 1.0);
                d2 * (1.0 - h) + d1 * h - blend_radius * h * (1.0 - h)
            }
            
            Shape::Translate { shape, offset } => shape.distance(point - *offset),
            Shape::Scale { shape, scale } => shape.distance(point / *scale) * scale,
            
            Shape::RotateY { shape, angle } => {
                let c = angle.cos();
                let s = angle.sin();
                let rotated = Vec3::new(
                    point.x * c - point.z * s,
                    point.y,
                    point.x * s + point.z * c,
                );
                shape.distance(rotated)
            }
        }
    }
    
    // Convenience constructors
    pub fn packed_cylinder(radius: u8, height: u8, scale: f32) -> Self {
        Shape::Packed(PackedPrimitive::cylinder(radius, height, scale))
    }
    
    pub fn packed_sphere(radius: u8, scale: f32) -> Self {
        Shape::Packed(PackedPrimitive::sphere(radius, scale))
    }
    
    pub fn packed_box(x: u8, y: u8, z: u8, scale: f32) -> Self {
        Shape::Packed(PackedPrimitive::box_shape(x, y, z, scale))
    }
    
    pub fn cylinder(radius: f32, height: f32) -> Self {
        Shape::Cylinder { radius, height }
    }
    
    pub fn sphere(radius: f32) -> Self {
        Shape::Sphere { radius }
    }
    
    pub fn box_shape(half_extents: Vec3) -> Self {
        Shape::Box { half_extents }
    }
    
    pub fn translate(self, offset: Vec3) -> Self {
        Shape::Translate { shape: Box::new(self), offset }
    }
    
    pub fn scale(self, scale: f32) -> Self {
        Shape::Scale { shape: Box::new(self), scale }
    }
    
    pub fn rotate_y(self, angle: f32) -> Self {
        Shape::RotateY { shape: Box::new(self), angle }
    }
    
    pub fn union(self, other: Shape) -> Self {
        Shape::Union(Box::new(self), Box::new(other))
    }
    
    pub fn intersect(self, other: Shape) -> Self {
        Shape::Intersection(Box::new(self), Box::new(other))
    }
    
    pub fn subtract(self, other: Shape) -> Self {
        Shape::Subtraction(Box::new(self), Box::new(other))
    }
}

// ============================================================================
// WORLD DEFINITION - SDF as Seed with Deduplication
// ============================================================================

use ahash::AHashMap;
use std::hash::{Hash, Hasher};

use crate::bits::OccupancyArray;

/// Fingerprint for an occupancy pattern - just the bits, no material
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OccupancyFingerprint {
    bits: [u32; 1024],
    hash: u64,  // Cached hash for fast lookups
}

impl OccupancyFingerprint {
    pub fn from_occupancy(occupancy: &OccupancyArray) -> Self {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        occupancy.bits.hash(&mut hasher);
        Self {
            bits: occupancy.bits,
            hash: hasher.finish(),
        }
    }
    
    pub fn to_linear_array(&self, material: u8) -> crate::bits::LinearArray {
        let count = self.count();
        let bits = self.bits;
        let occupancy = OccupancyArray::new(
            bits,
            count,
        );
        crate::bits::LinearArray::from_occupancy(&occupancy, material)
    }

    fn count(&self) -> u32 {
        self.bits.iter().map(|w| w.count_ones()).sum()
    }

}

impl Hash for OccupancyFingerprint {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
    }
}

/// Chunk reference - points to shared occupancy pattern
#[derive(Clone, Copy, Debug)]
pub struct ChunkReference {
    fingerprint_id: u32,  // Index into unique occupancy patterns
    material: u8,         // Material can vary per chunk
}

/// World definition with deduplication
#[derive(Clone, Debug)]
pub struct SdfWorld {
    /// The root SDF equation defining the entire world
    pub shape: Shape,
    /// Default material for solid voxels
    pub material: u8,
    /// Distance threshold for voxelization
    pub threshold: f32,
    
    /// Unique occupancy patterns (deduplicated)
    unique_patterns: Vec<OccupancyFingerprint>,
    /// Map from fingerprint to pattern ID
    pattern_lookup: AHashMap<u64, u32>,
    /// Cache: chunk coords -> reference to pattern
    chunk_cache: AHashMap<IVec3, ChunkReference>,
    
    /// Special IDs for common patterns
    empty_pattern_id: u32,
    full_pattern_id: u32,
}

impl SdfWorld {
    pub fn new(shape: Shape, material: u8, threshold: f32) -> Self {
        let mut world = Self {
            shape,
            material,
            threshold,
            unique_patterns: Vec::new(),
            pattern_lookup: AHashMap::new(),
            chunk_cache: AHashMap::new(),
            empty_pattern_id: 0,
            full_pattern_id: 0,
        };
        
        // Pre-register common patterns
        let empty = OccupancyFingerprint::from_occupancy(&OccupancyArray::default());
        let full = OccupancyFingerprint::from_occupancy(&OccupancyArray::full());
        
        world.empty_pattern_id = world.register_pattern(empty);
        world.full_pattern_id = world.register_pattern(full);
        
        world
    }
    
    /// Register a unique occupancy pattern and get its ID
    fn register_pattern(&mut self, fingerprint: OccupancyFingerprint) -> u32 {
        if let Some(&id) = self.pattern_lookup.get(&fingerprint.hash) {
            return id;
        }
        
        let id = self.unique_patterns.len() as u32;
        self.pattern_lookup.insert(fingerprint.hash, id);
        self.unique_patterns.push(fingerprint);
        id
    }
    
    /// Generate a specific chunk on-demand
    pub fn generate_chunk(&mut self, chunk_coords: IVec3) -> Option<crate::bits::LinearArray> {
        // Check cache first
        if let Some(&chunk_ref) = self.chunk_cache.get(&chunk_coords) {
            if chunk_ref.fingerprint_id == self.empty_pattern_id {
                return None;
            }
            let pattern = &self.unique_patterns[chunk_ref.fingerprint_id as usize];
            return Some(pattern.to_linear_array(chunk_ref.material));
        }
        
        // Generate if not cached
        const CHUNK_SIZE: f32 = 32.0;
        let chunk_world_pos = Vec3::new(
            chunk_coords.x as f32 * CHUNK_SIZE,
            chunk_coords.y as f32 * CHUNK_SIZE,
            chunk_coords.z as f32 * CHUNK_SIZE,
        );
        
        use crate::bits::OccupancyArray;
        let mut occupancy = OccupancyArray::default();
        
        // Sample all voxels to build occupancy pattern
        for z in 0..32 {
            for x in 0..32 {
                for y in 0..32 {
                    let world_pos = Vec3::new(
                        chunk_world_pos.x + x as f32,
                        chunk_world_pos.y + y as f32,
                        chunk_world_pos.z + z as f32,
                    );
                    
                    if self.shape.distance(world_pos) <= self.threshold {
                        let idx = y | (x << 5) | (z << 10);
                        occupancy.set_from_index(idx);
                    }
                }
            }
        }
        
        // Create fingerprint and register/lookup pattern
        let fingerprint = OccupancyFingerprint::from_occupancy(&occupancy);
        let pattern_id = self.register_pattern(fingerprint);
        
        // Cache the reference
        let chunk_ref = ChunkReference {
            fingerprint_id: pattern_id,
            material: self.material,
        };
        self.chunk_cache.insert(chunk_coords, chunk_ref);
        
        if pattern_id == self.empty_pattern_id {
            None
        } else {
            let pattern = &self.unique_patterns[pattern_id as usize];
            Some(pattern.to_linear_array(self.material))
        }
    }
    
    /// Generate all chunks within a bounding box
    pub fn generate_region(&mut self, min_chunk: IVec3, max_chunk: IVec3) -> Vec<(IVec3, crate::bits::LinearArray)> {
        let mut chunks = Vec::new();
        
        for cz in min_chunk.z..=max_chunk.z {
            for cy in min_chunk.y..=max_chunk.y {
                for cx in min_chunk.x..=max_chunk.x {
                    let coords = IVec3::new(cx, cy, cz);
                    if let Some(data) = self.generate_chunk(coords) {
                        chunks.push((coords, data));
                    }
                }
            }
        }
        
        chunks
    }
    
    /// Generate chunks in a radius around a point
    pub fn generate_around_point(&mut self, center: Vec3, radius_chunks: i32) -> Vec<(IVec3, crate::bits::LinearArray)> {
        const CHUNK_SIZE: f32 = 32.0;
        let center_chunk = IVec3::new(
            (center.x / CHUNK_SIZE).floor() as i32,
            (center.y / CHUNK_SIZE).floor() as i32,
            (center.z / CHUNK_SIZE).floor() as i32,
        );
        
        let min_chunk = center_chunk - IVec3::splat(radius_chunks);
        let max_chunk = center_chunk + IVec3::splat(radius_chunks);
        
        self.generate_region(min_chunk, max_chunk)
    }
    
    /// Clear the cache (but keep unique patterns)
    pub fn clear_cache(&mut self) {
        self.chunk_cache.clear();
    }
    
    /// Get deduplication statistics
    pub fn dedup_stats(&self) -> DeduplicationStats {
        let total_chunks = self.chunk_cache.len();
        let unique_patterns = self.unique_patterns.len();
        let empty_chunks = self.chunk_cache.values()
            .filter(|r| r.fingerprint_id == self.empty_pattern_id)
            .count();
        let full_chunks = self.chunk_cache.values()
            .filter(|r| r.fingerprint_id == self.full_pattern_id)
            .count();
        
        // Memory calculation
        let pattern_memory = unique_patterns * 4096; // 4KB per pattern (1024 × u32)
        let reference_memory = total_chunks * 8; // 8 bytes per reference (u32 + u32 + padding)
        let total_memory = pattern_memory + reference_memory;
        
        // Compare to naive storage
        let naive_memory = total_chunks * 32768; // 32KB per full chunk
        let memory_saved = naive_memory.saturating_sub(total_memory);
        
        DeduplicationStats {
            total_chunks,
            unique_patterns,
            empty_chunks,
            full_chunks,
            pattern_memory_kb: pattern_memory / 1024,
            reference_memory_kb: reference_memory / 1024,
            total_memory_kb: total_memory / 1024,
            memory_saved_kb: memory_saved / 1024,
            dedup_ratio: if unique_patterns > 0 {
                total_chunks as f32 / unique_patterns as f32
            } else {
                0.0
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeduplicationStats {
    pub total_chunks: usize,
    pub unique_patterns: usize,
    pub empty_chunks: usize,
    pub full_chunks: usize,
    pub pattern_memory_kb: usize,
    pub reference_memory_kb: usize,
    pub total_memory_kb: usize,
    pub memory_saved_kb: usize,
    pub dedup_ratio: f32,  // How many chunks share each pattern on average
}

/// Result of chunk generation - distinguishes surface vs interior chunks
#[derive(Debug)]
pub struct ChunkGenerationResult {
    /// Surface chunks with unique voxel data (partial fills)
    pub surface_chunks: AHashMap<IVec3, crate::bits::LinearArray>,
    /// Interior chunk positions (all identical - fully solid)
    pub interior_chunks: Vec<IVec3>,
    /// Material ID for interior chunks
    pub interior_material: u8,
}

/// Generate a single chunk of voxel data from a shape
pub fn generate_chunk_from_shape(
    shape: &Shape,
    material: u8,
    chunk_position: Vec3,
    threshold: f32,
) -> (crate::bits::LinearArray, bool) {
    use crate::bits::{LinearArray, OccupancyArray};
    
    let mut occupancy = OccupancyArray::default();
    
    // Sample all 32,768 voxels
    for z in 0..32 {
        for x in 0..32 {
            for y in 0..32 {
                let world_pos = Vec3::new(
                    chunk_position.x + x as f32,
                    chunk_position.y + y as f32,
                    chunk_position.z + z as f32,
                );
                
                if shape.distance(world_pos) <= threshold {
                    let idx = y | (x << 5) | (z << 10);
                    occupancy.set_from_index(idx);
                }
            }
        }
    }
    
    let is_full = occupancy.is_full();
    (LinearArray::from_occupancy(&occupancy, material), is_full)
}

/// Calculate bounding box for a shape (in chunk coordinates)
pub fn calculate_chunk_bounds(shape: &Shape, center: Vec3, max_distance: f32) -> (IVec3, IVec3) {
    const CHUNK_SIZE: f32 = 32.0;
    
    let min_world = center - Vec3::splat(max_distance);
    let max_world = center + Vec3::splat(max_distance);
    
    let min_chunk = IVec3::new(
        (min_world.x / CHUNK_SIZE).floor() as i32,
        (min_world.y / CHUNK_SIZE).floor() as i32,
        (min_world.z / CHUNK_SIZE).floor() as i32,
    );
    
    let max_chunk = IVec3::new(
        (max_world.x / CHUNK_SIZE).ceil() as i32,
        (max_world.y / CHUNK_SIZE).ceil() as i32,
        (max_world.z / CHUNK_SIZE).ceil() as i32,
    );
    
    (min_chunk, max_chunk)
}

/// Generate all chunks that intersect with a shape
/// Separates surface chunks (unique data) from interior chunks (all identical)
pub fn generate_chunks_from_shape(
    shape: &Shape,
    material: u8,
    center: Vec3,
    max_distance: f32,
    threshold: f32,
) -> ChunkGenerationResult {
    const CHUNK_SIZE: f32 = 32.0;
    
    let (min_chunk, max_chunk) = calculate_chunk_bounds(shape, center, max_distance);
    let mut surface_chunks = AHashMap::new();
    let mut interior_chunks = Vec::new();
    
    // Iterate through all chunks in bounding box
    for cz in min_chunk.z..=max_chunk.z {
        for cy in min_chunk.y..=max_chunk.y {
            for cx in min_chunk.x..=max_chunk.x {
                let chunk_world_pos = Vec3::new(
                    cx as f32 * CHUNK_SIZE,
                    cy as f32 * CHUNK_SIZE,
                    cz as f32 * CHUNK_SIZE,
                );
                
                let (chunk_data, is_full) = generate_chunk_from_shape(
                    shape,
                    material,
                    chunk_world_pos,
                    threshold,
                );
                
                if chunk_data.is_empty() {
                    // Skip empty chunks
                    continue;
                }
                
                let chunk_coords = IVec3::new(cx, cy, cz);
                
                if is_full {
                    // Interior chunk - just store position
                    interior_chunks.push(chunk_coords);
                } else {
                    // Surface chunk - store unique data
                    surface_chunks.insert(chunk_coords, chunk_data);
                }
            }
        }
    }
    
    ChunkGenerationResult {
        surface_chunks,
        interior_chunks,
        interior_material: material,
    }
}

// ============================================================================
// USAGE EXAMPLES
// ============================================================================

#[cfg(test)]
mod examples {
    use super::*;
    
    /// Example: Massive sphere with deduplication
    pub fn example_dedup_massive_sphere() {
        let shape = Shape::packed_sphere(16, 256.0)  // 128-chunk radius
            .translate(Vec3::new(16384.0, 0.0, 16384.0));
        
        let mut world = SdfWorld::new(shape, 1, 0.5);
        
        // Generate a large region
        let chunks = world.generate_region(
            IVec3::new(400, -10, 400),
            IVec3::new(600, 10, 600)
        );
        
        let stats = world.dedup_stats();
        println!("=== Deduplication Stats ===");
        println!("Total chunks: {}", stats.total_chunks);
        println!("Unique patterns: {}", stats.unique_patterns);
        println!("Empty: {}, Full: {}", stats.empty_chunks, stats.full_chunks);
        println!("Dedup ratio: {:.2}x (avg {} chunks per pattern)", 
            stats.dedup_ratio, 
            stats.dedup_ratio as usize
        );
        println!("\nMemory Usage:");
        println!("  Patterns: {} KB", stats.pattern_memory_kb);
        println!("  References: {} KB", stats.reference_memory_kb);
        println!("  Total: {} KB", stats.total_memory_kb);
        println!("  Saved: {} KB ({:.1}% reduction)", 
            stats.memory_saved_kb,
            (stats.memory_saved_kb as f32 / (stats.total_memory_kb + stats.memory_saved_kb) as f32) * 100.0
        );
        
        // Expected for solid sphere:
        // - ~2M interior chunks → 1 pattern (full)
        // - ~50k surface chunks → maybe 500-2000 unique patterns
        // - Total: ~2000 patterns for 2M chunks = 1000x dedup ratio!
        // - Memory: ~8MB instead of 64GB = 99.99% savings!
    }
    
    /// Example: Complex world with many similar patterns
    pub fn example_pillars_world() {
        let mut world = SdfWorld::new(
            Shape::packed_cylinder(4, 20, 1.0)
                .translate(Vec3::new(16.0, 16.0, 16.0)),
            1,
            0.5,
        );
        
        // Generate a grid of identical pillars
        for x in 0..10 {
            for z in 0..10 {
                let pillar = Shape::packed_cylinder(4, 20, 1.0)
                    .translate(Vec3::new(
                        (x * 64) as f32 + 16.0,
                        16.0,
                        (z * 64) as f32 + 16.0,
                    ));
                world.shape = world.shape.union(pillar);
            }
        }
        
        // Generate region covering all pillars
        let chunks = world.generate_region(
            IVec3::new(0, 0, 0),
            IVec3::new(20, 2, 20)
        );
        
        let stats = world.dedup_stats();
        println!("100 identical pillars:");
        println!("Chunks: {}, Unique patterns: {}", 
            stats.total_chunks, 
            stats.unique_patterns
        );
        // Each pillar has same occupancy pattern - massive dedup!
    }
    
    /// How to use in Bevy with deduplication stats
    pub fn load_world_with_stats(
        /* 
        mut world: ResMut<SdfWorld>,
        mut commands: Commands,
        quad: Res<FaceQuad>,
        */
    ) {
        // Example (won't compile in test):
        /*
        // Load chunks around origin
        let chunks = world.generate_around_point(Vec3::ZERO, 16);
        
        for (coords, data) in chunks {
            commands.spawn(Chunk::new(
                quad.0.clone(),
                data,
                ChunkIndex::from_chunk_coord(
                    coords.x as u32,
                    coords.y as u32,
                    coords.z as u32
                ),
                ChunkMeshRange::default(),
            ));
        }
        
        // Print memory savings
        let stats = world.dedup_stats();
        info!(
            "Loaded {} chunks using {} unique patterns ({:.0}x dedup, saved {} KB)",
            stats.total_chunks,
            stats.unique_patterns,
            stats.dedup_ratio,
            stats.memory_saved_kb
        );
        */
    }
}