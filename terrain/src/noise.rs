use std::f32;
use bevy::prelude::*;
use bevy::render::extract_component::ExtractComponent;
use bytemuck::{Pod, Zeroable};
use instance_manager::InstanceManager;
use noise::{NoiseFn, Perlin};
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;

use crate::config::{vertices_per_tile, ChunkSettings, NoiseSettings};

#[derive(Copy, Clone, Debug)]
pub enum HeightCurve {
    Identity,
    Cubic,
    MidBoost,
}

impl HeightCurve {
    #[inline(always)]
    pub fn evaluate(&self, t: f32) -> f32 {
        match self {
            Self::Identity => identity_fn(t),
            Self::Cubic => cubic_fn(t),
            Self::MidBoost => flat_mid_boost_fn(t),
        }
    }

}

// curves
fn identity_fn(t: f32) -> f32 {
    t
}
fn cubic_fn(t: f32) -> f32 {
    t.powf(3.0)
}
fn flat_mid_boost_fn(t: f32) -> f32 {    
    // Keyframe-based approximation (like Unity AnimationCurve)
    // Key points: (0.0, 0.0), (0.3, 0.1), (0.6, 0.5), (1.0, 1.0)
    if t < 0.3 {
        // Flat lowlands
        t * 0.333  // 0.0 -> 0.1 over 0.0-0.3
    } else if t < 0.6 {
        // Rising hills
        let local_t = (t - 0.3) / 0.3;
        0.1 + local_t * local_t * 0.4  // 0.1 -> 0.5 (quadratic)
    } else {
        // High mountains
        let local_t = (t - 0.6) / 0.4;
        0.5 + local_t * 0.5  // 0.5 -> 1.0 (linear)
    }
}

#[inline(always)]
const fn calc_chunk_count(value: usize, chunk_size: usize) -> usize {
    (value + chunk_size-1) / chunk_size
}

// Height curve evaluation - approximates Unity's AnimationCurve
#[inline(always)]
fn evaluate_height_curve(t: f32) -> f32 {    
    t.powf(3.0)
}

pub fn generate_octave_fractals(
    noise_config: &NoiseSettings,
    chunk_config: &ChunkSettings,
) -> Vec<f32> {
    let noise_scale = noise_config.noise_scale;
    let octaves = noise_config.octaves;
    let persistence = noise_config.persistence;
    let lacunarity = noise_config.lacunarity;
    let seed = noise_config.seed();

    let apply_falloff = true;
    let falloff_strength = 3.0_f32;
    let sea_level = 0.35_f32;
    let shape_exponent = 1.0_f32;

    let map_width = chunk_config.map_width as usize;
    let map_height = chunk_config.map_height as usize;
    let chunk_size = chunk_config.chunk_size as usize;

    let half_width = (map_width as f32) * 0.5;
    let half_height = (map_height as f32) * 0.5;

    let chunk_width = calc_chunk_count(map_width, chunk_size);
    let chunk_height = calc_chunk_count(map_height, chunk_size);
    let tile_count = chunk_height * chunk_width;

    let mut rng = StdRng::seed_from_u64(seed as u64);
    let samples_per_tile = (chunk_size + 2) * (chunk_size + 2);
    let perlin = Perlin::new(seed);
    
    let mut heightmap: Vec<f32> = vec![0.0; tile_count * samples_per_tile];
    let mut octave_offsets: Vec<Vec2> = Vec::with_capacity(octaves);

    for _ in 0..octaves {
        let x: f32 = rng.random_range(-1000.0..1000.0) + noise_config.offset.x;
        let z: f32 = rng.random_range(-1000.0..1000.0) + noise_config.offset.y;
        octave_offsets.push(Vec2::new(x, z));
    }

    // ============================================================
    // PASS 1: Generate raw noise and track ACTUAL min/max
    // ============================================================
    let mut min_noise_height = f32::MAX;
    let mut max_noise_height = f32::MIN;

    let mut write_offset = 0;
    for cz in 0..chunk_height {
        for cx in 0..chunk_width {
            let chunk_x_base = cx * chunk_size;
            let chunk_z_base = cz * chunk_size;
            let i = chunk_size + 2;

            for z in 0..i {
                for x in 0..i {
                    let world_x = (chunk_x_base + x) as f32;
                    let world_z = (chunk_z_base + z) as f32;

                    let sample_x_base = (world_x - half_width) / noise_scale;
                    let sample_z_base = (world_z - half_height) / noise_scale;

                    let mut amp = 1.0_f32;
                    let mut hz = 1.0_f32;
                    let mut noise_height = 0.0_f32;

                    for oi in 0..octaves {
                        let ox = octave_offsets[oi].x;
                        let oz = octave_offsets[oi].y;
                        let sx = sample_x_base * hz + ox;
                        let sz = sample_z_base * hz + oz;

                        let value = perlin.get([sx as f64, sz as f64]) as f32;
                        noise_height += value * amp;
                        amp *= persistence;
                        hz *= lacunarity;
                    }

                    if noise_height > max_noise_height {
                        max_noise_height = noise_height;
                    }
                    if noise_height < min_noise_height {
                        min_noise_height = noise_height;
                    }

                    heightmap[write_offset] = noise_height;
                    write_offset += 1;
                }
            }
        }
    }

    if (max_noise_height - min_noise_height).abs() < 0.0001 {
        max_noise_height = min_noise_height + 1.0;
    }

    println!("🌍 Global noise range: [{:.3}, {:.3}]", min_noise_height, max_noise_height);

    // ============================================================
    // PASS 2: Normalize using ACTUAL min/max and apply effects
    // ============================================================
    let map_world_width = (half_width * 2.0).max(1.0);
    let map_world_depth = (half_height * 2.0).max(1.0);

    write_offset = 0;
    for cz in 0..chunk_height {
        for cx in 0..chunk_width {
            let chunk_x_base = cx * chunk_size;
            let chunk_z_base = cz * chunk_size;
            let i = chunk_size + 2;

            for z in 0..i {
                for x in 0..i {
                    let world_x = (chunk_x_base + x) as f32;
                    let world_z = (chunk_z_base + z) as f32;

                    let noise_height = heightmap[write_offset];
                    let mut normalized = (noise_height - min_noise_height) / (max_noise_height - min_noise_height);

                    if shape_exponent != 1.0 {
                        normalized = normalized.powf(shape_exponent);
                    }

                    if apply_falloff {
                        let nx = (world_x - half_width) / (map_world_width * 0.5);
                        let nz = (world_z - half_height) / (map_world_depth * 0.5);
                        let radial = (nx * nx + nz * nz).sqrt().clamp(0.0, 1.0);
                        let fall = radial.powf(falloff_strength);
                        normalized = normalized * (1.0 - fall) + sea_level * fall;
                    }

                    heightmap[write_offset] = evaluate_height_curve(normalized.clamp(0.0, 1.0))*noise_config.height_scale;
                    write_offset += 1;
                }
            }
        }
    }

    heightmap
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash, ExtractComponent)]
pub struct TerrainTileId(pub u32);

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash, ExtractComponent)]
pub struct DirtyTile;

#[derive(Component)]
pub struct TerrainHeightMapTile(pub Vec<f32>);

impl TerrainHeightMapTile {
    #[inline(always)]
    pub fn from_slice(slice: &[f32]) -> Self {
        Self(Vec::from(slice))
    }
}

#[derive(Clone, Debug, Deref, DerefMut, Component,ExtractComponent)]
pub struct TerrainMeshlet(pub Vec<TerrainVertex>);

impl TerrainMeshlet{
    #[inline(always)]
    pub fn new(chunk_size: usize) -> Self {
        Self(vec![TerrainVertex::default(); vertices_per_tile(chunk_size)])
    }
}

#[derive(Bundle)]
pub struct Tile {
    pub meshlet: TerrainMeshlet,
    pub id: TerrainTileId,
}

impl Tile {
    #[inline(always)]
    pub fn new(meshlet: TerrainMeshlet, id: usize) -> Self {
        Self {
            meshlet,
            id: TerrainTileId(id as u32)            
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, Pod, Zeroable,)]
pub struct TerrainVertex {
    pub altitude: f32,
    pub normal: u32,
}


pub fn generate_tile_mesh(tile: &[f32], chunk_size: usize) -> TerrainMeshlet {
    // Remove height_multiplier parameter (or ignore it)
    let mut idx = 0usize;
    let row_stride = chunk_size + 2;
    let mut out: Vec<TerrainVertex> = Vec::with_capacity(vertices_per_tile(chunk_size));
    let out_ptr: *mut TerrainVertex = out.as_mut_ptr();

    for k in 0..chunk_size {
        let k1 = k + 1;

        unsafe {
            // Apply height curve but DON'T scale
            let h0_k = tile[k * row_stride];
            let h0_k1 = tile[k1 * row_stride];
            let h1_k = tile[k * row_stride + 1];
            let h1_k1 = tile[k1 * row_stride + 1];

            // Calculate normals - pass 1.0 as height multiplier for now
            // We'll fix normal calculation separately
            let n0 = calc_vertex_normal_curved(tile, row_stride, k, 0);
            let n0_k1 = calc_vertex_normal_curved(tile, row_stride, k1, 0);
            let n1_k = calc_vertex_normal_curved(tile, row_stride, k, 1);
            let n1_k1 = calc_vertex_normal_curved(tile, row_stride, k1, 1);

            // Store UNSCALED altitudes [0, 1]
            *out_ptr.add(idx) = TerrainVertex { altitude: h0_k, normal: n0 }; 
            idx += 1;
            *out_ptr.add(idx) = TerrainVertex { altitude: h0_k, normal: n0 }; 
            idx += 1;
            *out_ptr.add(idx) = TerrainVertex { altitude: h0_k1, normal: n0_k1 }; 
            idx += 1;
            *out_ptr.add(idx) = TerrainVertex { altitude: h1_k, normal: n1_k }; 
            idx += 1;
            *out_ptr.add(idx) = TerrainVertex { altitude: h1_k1, normal: n1_k1 }; 
            idx += 1;

            for c in 1..chunk_size {
                let x = c + 1;
                let h_top = tile[k * row_stride + x];
                let h_bottom = tile[k1 * row_stride + x];

                let n_top = calc_vertex_normal_curved(tile, row_stride, k, x);
                let n_bottom = calc_vertex_normal_curved(tile, row_stride, k1, x);

                *out_ptr.add(idx) = TerrainVertex { altitude: h_top, normal: n_top }; 
                idx += 1;
                *out_ptr.add(idx) = TerrainVertex { altitude: h_bottom, normal: n_bottom }; 
                idx += 1;
            }

            let last = tile[k1 * row_stride + chunk_size];
            let last_normal = calc_vertex_normal_curved(tile, row_stride, k1, chunk_size);
            
            *out_ptr.add(idx) = TerrainVertex { altitude: last, normal: last_normal }; 
            idx += 1;
        }
    }

    unsafe { out.set_len(idx) };
    TerrainMeshlet(out)
}

#[inline(always)]
fn calc_vertex_normal_curved(
    heights: &[f32], 
    row_stride: usize,
    row: usize, 
    col: usize,
) -> u32 {
    let center_idx = row * row_stride + col;
    
    // Apply curve to ALL heights for consistent normals
    let h_c = heights[center_idx];
    
    let row_above = row.saturating_sub(1);
    let row_below = (row + 1).min(row_stride - 1);
    let col_left = col.saturating_sub(1);
    let col_right = (col + 1).min(row_stride - 1);
    
    let h_n  = heights[row_above * row_stride + col];
    let h_s  = heights[row_below * row_stride + col];
    let h_e  = heights[row * row_stride + col_right];
    let h_w  = heights[row * row_stride + col_left];
    let h_ne = heights[row_above * row_stride + col_right];
    let h_nw = heights[row_above * row_stride + col_left];
    let h_se = heights[row_below * row_stride + col_right];
    let h_sw = heights[row_below * row_stride + col_left];
    
    let mut normal_sum = Vec3::ZERO;
    
    let add_tri = |sum: &mut Vec3, p0: Vec3, p1: Vec3, p2: Vec3| {
        *sum += (p1 - p0).cross(p2 - p0);
    };
    
    let v_c = Vec3::new(0.0, h_c, 0.0);
    
    if col < row_stride - 1 && row > 0 {
        add_tri(&mut normal_sum, v_c, Vec3::new(1.0, h_e, 0.0), Vec3::new(1.0, h_ne, -1.0));
        add_tri(&mut normal_sum, v_c, Vec3::new(1.0, h_ne, -1.0), Vec3::new(0.0, h_n, -1.0));
    }
    if col > 0 && row > 0 {
        add_tri(&mut normal_sum, v_c, Vec3::new(0.0, h_n, -1.0), Vec3::new(-1.0, h_nw, -1.0));
        add_tri(&mut normal_sum, v_c, Vec3::new(-1.0, h_nw, -1.0), Vec3::new(-1.0, h_w, 0.0));
    }
    if col > 0 && row < row_stride - 1 {
        add_tri(&mut normal_sum, v_c, Vec3::new(-1.0, h_w, 0.0), Vec3::new(-1.0, h_sw, 1.0));
        add_tri(&mut normal_sum, v_c, Vec3::new(-1.0, h_sw, 1.0), Vec3::new(0.0, h_s, 1.0));
    }
    if col < row_stride - 1 && row < row_stride - 1 {
        add_tri(&mut normal_sum, v_c, Vec3::new(0.0, h_s, 1.0), Vec3::new(1.0, h_se, 1.0));
        add_tri(&mut normal_sum, v_c, Vec3::new(1.0, h_se, 1.0), Vec3::new(1.0, h_e, 0.0));
    }
    
    let normal = normal_sum.normalize();
    
    let pitch = normal.y.asin();
    let yaw = normal.x.atan2(normal.z);
    let pitch_u16 = ((pitch / std::f32::consts::FRAC_PI_2) * 65535.0).clamp(0.0, 65535.0) as u16;
    let yaw_u16 = (((yaw + std::f32::consts::PI) / (2.0 * std::f32::consts::PI)) * 65535.0).clamp(0.0, 65535.0) as u16;
    
    (pitch_u16 as u32) | ((yaw_u16 as u32) << 16)
}