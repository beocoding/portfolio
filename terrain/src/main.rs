use bevy::{
    prelude::*,
    render::settings::WgpuFeatures,
    render::{
        RenderPlugin,
        settings::{RenderCreation, WgpuSettings}
    },
};

use terrain::{
    camera::{move_player, rotate_player_camera, spawn_player_cameras, spawn_test_player, update_player_cameras, zoom_player_camera}, config::CameraSettings, debug::{debug_draw_tiles,  debug_tiles_spawned}, pipeline::{init_terrain, TerrainPlugin}, terrain_ui::TerrainUiPlugin
};

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(ImagePlugin::default_nearest())
                .set(RenderPlugin {
                    render_creation: RenderCreation::Automatic(WgpuSettings {
                        features: WgpuFeatures::default() | WgpuFeatures::INDIRECT_FIRST_INSTANCE,
                        ..default()
                    }),
                    ..default()
                })
        )
        .add_plugins(TerrainPlugin)
        .add_plugins(TerrainUiPlugin)  // Add the UI plugin
        .init_resource::<CameraSettings>()
        .add_systems(Startup, (init_terrain, spawn_test_player.after(init_terrain)))
        .add_systems(Update, (
            draw_axes,
            // Debug systems
            debug_tiles_spawned,
            debug_draw_tiles,
            spawn_player_cameras,  // Auto-spawn cameras for new players
            update_player_cameras,  // Follow players
            move_player,            // WASD movement
            zoom_player_camera,     // Mouse wheel
            rotate_player_camera,   // Q/E and R/F for rotation
        ))
        
        .run();
}

// System to draw coordinate axes every frame
pub fn draw_axes(mut gizmos: Gizmos) {
    let axis_length = 50.0;
    let origin = Vec3::ZERO;
    
    // X axis - Red
    gizmos.arrow(
        origin,
        origin + Vec3::X * axis_length,
        Color::srgb(1.0, 0.0, 0.0),
    );
    
    // Y axis - Green
    gizmos.arrow(
        origin,
        origin + Vec3::Y * axis_length,
        Color::srgb(0.0, 1.0, 0.0),
    );
    
    // Z axis - Blue
    gizmos.arrow(
        origin,
        origin + Vec3::Z * axis_length,
        Color::srgb(0.0, 0.0, 1.0),
    );
}