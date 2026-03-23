use bevy::prelude::*;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseMotion};
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use std::ops::Range;

use crate::config::CameraSettings;

// ============================================================================
// COMPONENTS
// ============================================================================

#[derive(Component)]
pub struct Player {
    pub id: u32,  // For multiplayer identification
}

#[derive(Component)]
pub struct PlayerCamera {
    pub player_entity: Entity,
    pub distance: f32,
    pub pitch: f32,
    pub yaw: f32,
    pub height_offset: f32,
    pub shoulder_offset: f32,  // ✅ Side offset (positive = right shoulder)
    pub look_ahead: f32,       // ✅ How far ahead to look
}

impl Default for PlayerCamera {
    fn default() -> Self {
        Self {
            player_entity: Entity::PLACEHOLDER,
            distance: 5.0,           // ✅ Closer
            pitch: 20.0,             // ✅ More horizontal view
            yaw: 180.0,              // ✅ Behind the player
            height_offset: 1.5,      // ✅ Eye level
            shoulder_offset: 0.8,    // ✅ Over right shoulder
            look_ahead: 2.0,         // ✅ Look ahead of player
        }
    }
}

// ============================================================================
// SPAWN SYSTEMS
// ============================================================================

/// Spawn a test player (in the future, this would be called per-player)
pub fn spawn_test_player(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let player_entity = commands.spawn((
        Name::new("Player_0"),
        Player { id: 0 },
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.8, 0.3),  // Green color
            ..default()
        })),
        Transform::from_translation(Vec3::new(0.0, 0.5, 0.0)),  // Raised 0.5 so bottom sits at y=0
        Visibility::default(),
    )).id();
    
    println!("🎮 Spawned player {:?} at origin with 1x1 cube mesh", player_entity);
}

/// Automatically spawn cameras for any new players
pub fn spawn_player_cameras(
    mut commands: Commands,
    camera_settings: Res<CameraSettings>,
    new_players: Query<(Entity, &Player), Added<Player>>,
    existing_cameras: Query<&PlayerCamera>,
) {
    for (player_entity, player) in new_players.iter() {
        // Check if this player already has a camera
        let has_camera = existing_cameras.iter().any(|cam| cam.player_entity == player_entity);
        
        if has_camera {
            continue;
        }
        
        let player_cam = PlayerCamera {
            player_entity,
            ..Default::default()
        };
        
        // Calculate initial camera position
        let camera_pos = calculate_camera_position(
            Vec3::ZERO,  // Player starts at origin
            player_cam.distance,
            player_cam.pitch,
            player_cam.yaw,
            player_cam.height_offset,
        );
        
        commands.spawn((
            Name::new(format!("Camera_Player_{}", player.id)),
            Camera3d::default(),
            Projection::Perspective(PerspectiveProjection {
                fov: std::f32::consts::PI / 4.0,  // 45 degree FOV
                near: 0.1,
                far: 1000.0,
                ..default()
            }),
            Transform::from_translation(camera_pos).looking_at(Vec3::ZERO, Vec3::Y),
            player_cam,
        ));
        
        println!("📷 Spawned camera for player {} at {:?}", player.id, camera_pos);
    }
}

// ============================================================================
// UPDATE SYSTEMS
// ============================================================================

/// Update all player cameras to follow their respective players
pub fn update_player_cameras(
    players: Query<(&Transform, Entity), With<Player>>,
    mut cameras: Query<(&mut Transform, &PlayerCamera), Without<Player>>,
) {
    for (mut cam_transform, player_cam) in cameras.iter_mut() {
        if let Ok((player_transform, _)) = players.get(player_cam.player_entity) {
            let player_pos = player_transform.translation;
            
            // Calculate camera position based on yaw and pitch
            let yaw_rad = player_cam.yaw.to_radians();
            let pitch_rad = player_cam.pitch.to_radians();
            
            // Camera offset in spherical coordinates
            let offset_x = player_cam.distance * yaw_rad.sin() * pitch_rad.cos();
            let offset_y = player_cam.distance * pitch_rad.sin();
            let offset_z = player_cam.distance * yaw_rad.cos() * pitch_rad.cos();
            
            let camera_pos = player_pos 
                + Vec3::new(offset_x, offset_y + player_cam.height_offset, offset_z);
            
            // Look at player position plus height offset
            let look_target = player_pos + Vec3::Y * player_cam.height_offset;
            
            *cam_transform = Transform::from_translation(camera_pos)
                .looking_at(look_target, Vec3::Y);
        }
    }
}

/// Handle camera zoom with mouse wheel
pub fn zoom_player_camera(
    mut cameras: Query<&mut PlayerCamera>,
    camera_settings: Res<CameraSettings>,
    mouse_wheel_input: Res<AccumulatedMouseScroll>,
) {
    if mouse_wheel_input.delta.y == 0.0 {
        return;
    }
    
    for mut player_cam in cameras.iter_mut() {
        let delta = -mouse_wheel_input.delta.y * 0.5;
        player_cam.distance = (player_cam.distance + delta)
            .clamp(camera_settings.min_distance, camera_settings.max_distance);
    }
}

/// Rotate camera around player with keyboard (relative to player facing)
pub fn rotate_player_camera(
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    camera_settings: Res<CameraSettings>,
    mut cameras: Query<&mut PlayerCamera>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    mut cursor_query: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let rotation_speed = 90.0;
    let dt = time.delta_secs();
    
    // Check if Alt is being held
    let alt_held = keyboard.pressed(KeyCode::AltLeft) || keyboard.pressed(KeyCode::AltRight);
    
    // Lock cursor for infinite scrolling only when window is focused and Alt is not held
    if let Ok(window) = window_query.single() {
        if let Ok(mut cursor_options) = cursor_query.single_mut() {
            if window.focused && !alt_held {
                cursor_options.grab_mode = CursorGrabMode::Locked;
                cursor_options.visible = false;
            } else {
                cursor_options.grab_mode = CursorGrabMode::None;
                cursor_options.visible = true;
            }
        }
    }
    
    for mut player_cam in cameras.iter_mut() {
        // Mouse look - only active when Alt is not held
        if !alt_held {
            player_cam.yaw -= mouse_motion.delta.x * camera_settings.mouse_sensitivity;
            player_cam.pitch = (player_cam.pitch + mouse_motion.delta.y * camera_settings.mouse_sensitivity)
                .clamp(-89.0, 89.0);
        }
        
        // Q/E: Rotate around player (orbit) - keyboard fallback
        if keyboard.pressed(KeyCode::KeyQ) {
            player_cam.yaw += rotation_speed * dt;
        }
        if keyboard.pressed(KeyCode::KeyE) {
            player_cam.yaw -= rotation_speed * dt;
        }
        
        // R/F: Adjust pitch
        if keyboard.pressed(KeyCode::KeyR) {
            player_cam.pitch = (player_cam.pitch + rotation_speed * dt).clamp(-89.0, 89.0);
        }
        if keyboard.pressed(KeyCode::KeyF) {
            player_cam.pitch = (player_cam.pitch - rotation_speed * dt).clamp(-89.0, 89.0);
        }
        
        // Z/X: Adjust shoulder offset (switch shoulders)
        if keyboard.just_pressed(KeyCode::KeyZ) {
            player_cam.shoulder_offset = -player_cam.shoulder_offset;  // Swap shoulders
        }
    }
}

/// Move player with WASD (basic movement for testing)
pub fn move_player(
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    cameras: Query<&PlayerCamera>,
    mut players: Query<(&mut Transform, Entity), With<Player>>,
) {
    let move_speed = 10.0;
    let dt = time.delta_secs();
    
    for (mut transform, player_entity) in players.iter_mut() {
        // Find camera for this player
        let camera_yaw = cameras
            .iter()
            .find(|cam| cam.player_entity == player_entity)
            .map(|cam| cam.yaw)
            .unwrap_or(0.0);
        
        // Calculate forward and right vectors based on camera yaw
        let yaw_rad = camera_yaw.to_radians();
        let forward = Vec3::new(-yaw_rad.sin(), 0.0, -yaw_rad.cos());
        let right = Vec3::new(yaw_rad.cos(), 0.0, -yaw_rad.sin());
        
        let mut movement = Vec3::ZERO;
        
        if keyboard.pressed(KeyCode::KeyW) {
            movement += forward * move_speed * dt;
        }
        if keyboard.pressed(KeyCode::KeyS) {
            movement -= forward * move_speed * dt;
        }
        if keyboard.pressed(KeyCode::KeyA) {
            movement -= right * move_speed * dt;
        }
        if keyboard.pressed(KeyCode::KeyD) {
            movement += right * move_speed * dt;
        }
        
        if keyboard.pressed(KeyCode::Space) {
            movement.y += move_speed * dt;
        }
        if keyboard.pressed(KeyCode::ShiftLeft) {
            movement.y -= move_speed * dt;
        }
        
        transform.translation += movement;
    }
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================
fn calculate_camera_offset(
    distance: f32,
    pitch_deg: f32,
    yaw_deg: f32,
) -> Vec3 {
    let pitch = pitch_deg.to_radians();
    let yaw = yaw_deg.to_radians();
    
    let x_offset = distance * yaw.sin() * pitch.cos();
    let z_offset = distance * yaw.cos() * pitch.cos();
    let y_offset = distance * pitch.sin();
    
    Vec3::new(x_offset, y_offset, z_offset)
}

fn calculate_camera_position(
    target: Vec3,
    distance: f32,
    pitch_deg: f32,
    yaw_deg: f32,
    height_offset: f32,
) -> Vec3 {
    let offset = calculate_camera_offset(distance, pitch_deg, yaw_deg);
    target + Vec3::new(offset.x, offset.y + height_offset, offset.z)
}