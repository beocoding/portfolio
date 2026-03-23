// Add these debug systems to your main.rs

use bevy::{color::palettes::css::{BLUE, GREEN, RED}, prelude::*};

use crate::{buffer::TerrainMeshData, config::ChunkSettings, noise::TerrainTileId};

// Debug: Print tile spawning
pub fn debug_tiles_spawned(
    query: Query<&TerrainTileId, Added<TerrainTileId>>,
) {
    let count = query.iter().count();
    if count > 0 {
        println!("=== TILES SPAWNED: {} ===", count);
        for (idx, tile_id) in query.iter().enumerate().take(5) {
            println!("  Tile {}: id={}", idx, tile_id.0);
        }
        if count > 5 {
            println!("  ... and {} more", count - 5);
        }
    }
}

pub fn debug_draw_tiles(
    mut gizmos: Gizmos,
    query: Query<&TerrainTileId>,
    config: Res<ChunkSettings>,
) {
    let chunk_size = config.chunk_size as f32;
    let tiles_x = (config.map_width / config.chunk_size) as u32;
    let tiles_z = (config.map_height / config.chunk_size) as u32;

    let map_width = tiles_x as f32 * chunk_size;
    let map_depth = tiles_z as f32 * chunk_size;
    let half_chunk = chunk_size * 0.5;

    // iterate every spawned tile
    for tile_id in query.iter() {
        let id = tile_id.0;
        let chunk_x = (id % tiles_x) as f32;
        let chunk_z = (id / tiles_x) as f32;

        // compute tile center such that the whole grid is centered at world origin
        let world_center_x = (chunk_x + 0.5) * chunk_size - map_width * 0.5;
        let world_center_z = (chunk_z + 0.5) * chunk_size - map_depth * 0.5;

        // corners from center (y = 0)
        let corners = [
            Vec3::new(world_center_x - half_chunk, 0.0, world_center_z - half_chunk),
            Vec3::new(world_center_x + half_chunk, 0.0, world_center_z - half_chunk),
            Vec3::new(world_center_x + half_chunk, 0.0, world_center_z + half_chunk),
            Vec3::new(world_center_x - half_chunk, 0.0, world_center_z + half_chunk),
        ];

        // color per tile (example: hue from tile id)
        let hue = ((id as f32) % 360.0) / 360.0;
        let color = Color::hsla(hue, 0.8, 0.5, 1.0);

        for i in 0..4 {
            gizmos.line(corners[i], corners[(i + 1) % 4], color);
        }
    }

    // optional: draw axes at origin for reference
    let axis_len = (map_width.max(map_depth) * 0.5).max(1.0);
    gizmos.line(Vec3::ZERO, Vec3::X * axis_len, RED);   // X = red
    gizmos.line(Vec3::ZERO, Vec3::Y * axis_len, GREEN); // Y = green
    gizmos.line(Vec3::ZERO, Vec3::Z * axis_len, BLUE);  // Z = blue
}
