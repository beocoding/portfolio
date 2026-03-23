// voxels/src/camera.rs
use bevy::prelude::*;
use bevy::camera::ScalingMode::FixedVertical;
use bevy::input::mouse::AccumulatedMouseScroll;
use std::ops::Range;

#[derive(Component)]
pub struct CameraFocalPoint {
    pub target: Vec3,
}

#[derive(Debug, Resource)]
pub struct CameraSettings {
    pub orthographic_viewport_height: f32,
    pub orthographic_zoom_range: Range<f32>,
    pub orthographic_zoom_speed: f32,
}

pub fn setup_camera(
    camera_settings: Res<CameraSettings>,
    mut commands: Commands,
) {
    const CHUNK_SIZE: f32 = 32.0;
    
    // Look at the center chunk (0, 0, 0)
    let chunk_index = Vec3::new(0.0, 0.0, 0.0);
    let chunk_world_pos = chunk_index * CHUNK_SIZE;
    let chunk_center = chunk_world_pos + Vec3::splat(CHUNK_SIZE / 2.0);
    let look_at = chunk_center;
    
    // Position camera at 225° yaw (45° + 180°), 30° pitch, 64 units away
    let distance: f32 = 64.0;
    let pitch_deg: f32 = 30.0;
    let yaw_deg: f32 = 45.0 + 180.0; // 🔁 flipped 180° around Y
    
    let pitch = pitch_deg.to_radians();
    let yaw = yaw_deg.to_radians();
    
    let x_offset = distance * yaw.cos() * pitch.cos();
    let z_offset = distance * yaw.sin() * pitch.cos();
    let y_offset = distance * pitch.sin();
    
    let camera_pos = look_at + Vec3::new(x_offset, y_offset, z_offset);
    
    println!("Looking at chunk (0, 0, 0) center: {:?}", look_at);
    println!("Camera at: {:?}", camera_pos);
    
    commands.spawn((
        Name::new("Camera"),
        Camera3d::default(),
        Projection::from(OrthographicProjection {
            scaling_mode: FixedVertical {
                viewport_height: camera_settings.orthographic_viewport_height,
            },
            scale: 1.0,
            near: -5000.0,
            far: 5000.0,
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_translation(camera_pos).looking_at(look_at, Vec3::Y),
        CameraFocalPoint { target: look_at },
    ));
}


pub fn zoom(
    camera: Single<&mut Projection, With<Camera>>,
    camera_settings: Res<CameraSettings>,
    mouse_wheel_input: Res<AccumulatedMouseScroll>,
) {
    if let Projection::Orthographic(ref mut orthographic) = *camera.into_inner() {
        let delta_zoom = -mouse_wheel_input.delta.y * camera_settings.orthographic_zoom_speed;
        let multiplicative_zoom = 1. + delta_zoom;

        orthographic.scale = (orthographic.scale * multiplicative_zoom).clamp(
            camera_settings.orthographic_zoom_range.start,
            camera_settings.orthographic_zoom_range.end*5.,
        );
    }
}

pub fn camera_movement(
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut camera_query: Query<(&mut Transform, &mut CameraFocalPoint), With<Camera>>,
) {
    if let Ok((mut transform, mut focal_point)) = camera_query.single_mut() {
        let move_speed = 100.0;
        let dt = time.delta_secs();
        
        // Get camera's horizontal movement directions (projected on XZ plane)
        let camera_forward = transform.forward();
        let camera_right = transform.right();
        let horizontal_forward = Vec3::new(camera_forward.x, 0.0, camera_forward.z).normalize();
        let horizontal_right = Vec3::new(camera_right.x, 0.0, camera_right.z).normalize();
        
        let mut horizontal_displacement = Vec3::ZERO;
        
        // WASD moves camera AND focal point horizontally (panning, angle preserved)
        if keyboard.pressed(KeyCode::KeyW) {
            horizontal_displacement += horizontal_forward * move_speed * dt;
        }
        if keyboard.pressed(KeyCode::KeyS) {
            horizontal_displacement -= horizontal_forward * move_speed * dt;
        }
        if keyboard.pressed(KeyCode::KeyA) {
            horizontal_displacement -= horizontal_right * move_speed * dt;
        }
        if keyboard.pressed(KeyCode::KeyD) {
            horizontal_displacement += horizontal_right * move_speed * dt;
        }
        
        // Apply horizontal movement to both camera and focal point
        transform.translation += horizontal_displacement;
        focal_point.target += horizontal_displacement;
        
        // Space/Shift moves ONLY camera vertically (tilting, angle changes)
        if keyboard.pressed(KeyCode::Space) {
            transform.translation.y += move_speed * dt;
        }
        if keyboard.pressed(KeyCode::ShiftLeft) {
            transform.translation.y -= move_speed * dt;
        }
        
        // Update camera to look at focal point
        *transform = Transform::from_translation(transform.translation)
            .looking_at(focal_point.target, Vec3::Y);
    }
}