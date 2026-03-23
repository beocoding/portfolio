use bevy::prelude::*;

use crate::{bits::{ ChunkData, OccupancyArray}, buffers::{ChunkMeshRange, FrameCounter}, constants::{Chunk, VoxelType}, index::index::ChunkIndex, pipeline::FaceQuad};

pub fn test_frame_chunks(
    mut commands: Commands,
    mut frame: ResMut<FrameCounter>,
    quad: Res<FaceQuad>,
) {
    // Create a single voxel instead of filling entire chunk
    let test_chunk = ChunkData::filled(VoxelType::Dirt as u8);

    let index = ChunkIndex(frame.0);

    commands.spawn(Chunk::new(
        quad.0.clone(),
        test_chunk,
        index,
        ChunkMeshRange::default(),
    ));
    if index.x()!= 0 {println!("New X {:?} spawned!", index)};


    frame.0 += 1;

}


pub fn test_chunk(
    mut commands: Commands,
    quad: Res<FaceQuad>,
) {
    // Create a single voxel instead of filling entire chunk
    let test_chunk = ChunkData::filled(VoxelType::Dirt as u8);

    commands.spawn(Chunk::new(
        quad.0.clone(),
        test_chunk.clone(),
        ChunkIndex(256),
        ChunkMeshRange::default(),
    ));

    commands.spawn(Chunk::new(
        quad.0.clone(),
        test_chunk,
        ChunkIndex(0),
        ChunkMeshRange::default(),
    ));


    println!("Chunk {} spawned!", 0);

}
pub fn test_platform(
    mut commands: Commands,
    quad: Res<FaceQuad>,
) {
    let start = std::time::Instant::now();

    const PLATFORM_RADIUS_CHUNKS: f32 = 16.0;
    const PLATFORM_THICKNESS_CHUNKS: f32 = 1.0;
    const CENTER_X: f32 = 16.0;
    const CENTER_Y: f32 = 0.5;
    const CENTER_Z: f32 = 16.0;
    const CHUNK_SIZE: f32 = 32.0;
    
    let material = VoxelType::Dirt as u8;
    
    let sdf = |x: f32, y: f32, z: f32| -> f32 {
        let dx = x - CENTER_X;
        let dz = z - CENTER_Z;
        let radial_dist = (dx * dx + dz * dz).sqrt();
        let circle_dist = radial_dist - PLATFORM_RADIUS_CHUNKS;
        let y_dist = (y - CENTER_Y).abs() - (PLATFORM_THICKNESS_CHUNKS / 2.0);
        circle_dist.max(y_dist)
    };
    
    let search_radius = (PLATFORM_RADIUS_CHUNKS.ceil() as i32) + 2;
    let y_range = (PLATFORM_THICKNESS_CHUNKS.ceil() as u32) + 1;
    
    let corners = [
        (0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (1.0, 1.0, 0.0),
        (0.0, 0.0, 1.0), (1.0, 0.0, 1.0), (0.0, 1.0, 1.0), (1.0, 1.0, 1.0),
    ];

    for z_offset in -search_radius..=search_radius {
        for x_offset in -search_radius..=search_radius {
            for y in 0..y_range {
                let chunk_x = CENTER_X as i32 + x_offset;
                let chunk_z = CENTER_Z as i32 + z_offset;
                
                if chunk_x < 0 || chunk_z < 0 {
                    continue;
                }

                // FIX 1: fixed array instead of Vec
                let mut corner_values = [0.0f32; 8];
                for (i, (dx, dy, dz)) in corners.iter().enumerate() {
                    corner_values[i] = sdf(
                        chunk_x as f32 + dx,
                        y as f32 + dy,
                        chunk_z as f32 + dz,
                    );
                }

                let all_inside = corner_values.iter().all(|&v| v < 0.0);
                let all_outside = corner_values.iter().all(|&v| v > 0.0);

                if all_outside {
                    continue;
                }

                let chunk_data = if all_inside {
                    ChunkData::filled(material)
                } else {
                    let mut occupancy = OccupancyArray::default();

                    // FIX 2: hoist sqrt out of the inner Y loop
                    for vz in 0..32 {
                        for vx in 0..32 {
                            let voxel_chunk_x = chunk_x as f32 + (vx as f32 + 0.5) / CHUNK_SIZE;
                            let voxel_chunk_z = chunk_z as f32 + (vz as f32 + 0.5) / CHUNK_SIZE;
                            let dx = voxel_chunk_x - CENTER_X;
                            let dz = voxel_chunk_z - CENTER_Z;
                            let circle_dist = (dx * dx + dz * dz).sqrt() - PLATFORM_RADIUS_CHUNKS;

                            for vy in 0..32 {
                                let voxel_chunk_y = y as f32 + (vy as f32 + 0.5) / CHUNK_SIZE;
                                let y_dist = (voxel_chunk_y - CENTER_Y).abs() - (PLATFORM_THICKNESS_CHUNKS / 2.0);
                                if circle_dist.max(y_dist) < 0.0 {
                                    let idx = vy | (vx << 5) | (vz << 10);
                                    occupancy.set_from_index(idx);
                                }
                            }
                        }
                    }

                    ChunkData::from_occupancy(&occupancy, material)
                };
                
                if !chunk_data.is_empty() {
                    commands.spawn(Chunk::new(
                        quad.0.clone(),
                        chunk_data,
                        ChunkIndex::from_chunk_coord(chunk_x as u32, y, chunk_z as u32),
                        ChunkMeshRange::default(),
                    ));
                }
            }
        }
    }
    
    println!("SDF-based circular platform spawned at chunks ({}, {}, {}) with radius {} chunks in {:?}", 
             CENTER_X, CENTER_Y, CENTER_Z, PLATFORM_RADIUS_CHUNKS, start.elapsed());
}