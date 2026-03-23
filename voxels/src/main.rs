
use bevy::prelude::*;
use voxels::{camera::{camera_movement, setup_camera, zoom, CameraSettings}, debug::test_platform, pipeline::{VoxelEnginePlugin, VoxelInitSet}, terrain::debug::draw_axes};


// Import the stress test module
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MainStartUpSet;
fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            VoxelEnginePlugin,
        ))
        .configure_sets(Startup, MainStartUpSet.after(VoxelInitSet))
        // Uncomment one of these for testing individual features
        .insert_resource(CameraSettings {
            orthographic_viewport_height: 5.,
            orthographic_zoom_range: 0.1..1000.0,
            orthographic_zoom_speed: 0.2,
        })
        .add_systems(Startup, setup_camera)
        .add_systems(Startup, test_platform.in_set(MainStartUpSet))
        .add_systems(Update, (camera_movement, zoom, draw_axes))        
        .run();
}


