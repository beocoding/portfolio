use std::ops::Range;

use bevy::{prelude::*, render::extract_resource::ExtractResource};
use bytemuck::{Pod, Zeroable};

use crate::noise::HeightCurve;


#[inline(always)]
pub const fn samples_per_tile(chunk_size: usize) -> usize {
    let size = chunk_size+2;
    size*size
}
#[inline(always)]
pub const fn vertices_per_row(chunk_size: usize) -> usize {
    (chunk_size<<1)+4
}
#[inline(always)]
pub const fn vertices_per_tile(chunk_size: usize) -> usize {
    vertices_per_row(chunk_size) * chunk_size + (chunk_size - 1)
}

#[derive(Resource, Clone, Copy, ExtractResource, Pod, Zeroable)]
#[repr(C)]
pub struct ChunkSettings {
    pub view_distance: u32,  // Changed from usize
    pub chunk_size: u32,     // Changed from usize
    pub map_height: u32,     // Changed from usize
    pub map_width: u32,      // Changed from usize
}

impl Default for ChunkSettings {
    fn default() -> Self {
        Self {
            view_distance: 4,
            chunk_size: 32,
            map_height: 512,
            map_width: 512,
        }
    }
}

#[derive(Debug, Resource)]
pub struct CameraSettings {
    pub orthographic_viewport_height: f32,
    pub orthographic_zoom_range: Range<f32>,
    pub orthographic_zoom_speed: f32,
    pub min_distance: f32,
    pub max_distance: f32,
    pub mouse_sensitivity: f32,
}

impl Default for CameraSettings {
    fn default() -> Self {
        Self {
            orthographic_viewport_height: 50.0,
            orthographic_zoom_range: 0.1..1000.0,
            orthographic_zoom_speed: 0.2,
            min_distance: 2.0,
            max_distance: 20.0,
            mouse_sensitivity: 0.1,
        }
    }
}

#[derive(Debug, Copy, Clone, Default)]
pub enum NoisePatterns {
    #[default]OctaveFractal,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NormalizeMode {
    Local,
    Global,
}

#[derive(Resource, Debug, Copy, Clone, ExtractResource)]
pub struct NoiseSettings {
    pub offset: Vec2,
    pub octaves: usize,
    pub seed: Option<u32>,
    pub noise_scale: f32,
    pub lacunarity: f32,
    pub persistence: f32,
    pub height_scale: f32,
    pub noise_type: NoisePatterns,
    pub normalize_mode: NormalizeMode,
    pub height_curve: HeightCurve
}  

impl Default for NoiseSettings{
    #[inline(always)]
    fn default() -> Self {
        Self {
            offset: Vec2::ZERO,        // no offset by default
            octaves: 4,                // default number of noise octaves
            seed: None,                // random seed by default
            noise_scale: 0.01,          // default noise scaling
            lacunarity: 2.0,           // default lacunarity
            persistence: 0.5,          // default persistence
            height_scale: 1.0,
            noise_type: NoisePatterns::default(), // replace with an actual variant
            normalize_mode: NormalizeMode::Global,
            height_curve: HeightCurve::MidBoost
        }
    }
}
impl NoiseSettings {   
    #[inline(always)]
    pub fn new(
        offset: Vec2,
        octaves: usize,
        seed: Option<u32>,
        noise_scale: f32,
        lacunarity: f32,
        persistence: f32,
        height_scale: f32,
        noise_type: NoisePatterns,
        normalize_mode: NormalizeMode,
        height_curve: HeightCurve,
    ) -> Self {
        Self {
            offset,
            octaves,
            seed,
            noise_scale,
            lacunarity,
            persistence,
            height_scale,
            noise_type,
            normalize_mode,
            height_curve
        }
    }

    #[inline(always)]
    pub fn seed(&self) -> u32 {
        self.seed.unwrap_or_else(rand::random)
    }
}