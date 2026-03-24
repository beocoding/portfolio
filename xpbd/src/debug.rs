use bevy::prelude::*;
use super::*;

pub struct XPBDDebugPlugin;

impl Plugin for XPBDDebugPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_resource(DebugTimer::default())
            .add_systems(Update, (
                debug_gravity_system,
                debug_draw_constraints,  // ← Add this
            ));
    }
}

// Debug resource to control print frequency
#[derive(Resource)]
struct DebugTimer {
    timer: Timer,
}

impl Default for DebugTimer {
    fn default() -> Self {
        Self {
            timer: Timer::from_seconds(1.0, TimerMode::Repeating),
        }
    }
}

fn debug_gravity_system(
    time: Res<Time>,
    mut debug_timer: ResMut<DebugTimer>,
    query: Query<(&Transform, &Velocity, &InverseMass, Option<&ApplyGravity>), With<RigidBodyMarker>>,
) {
    debug_timer.timer.tick(time.delta());
    
    if debug_timer.timer.just_finished() {
        println!("\n=== Gravity Debug (t={:.2}s) ===", time.elapsed_secs());
        
        for (transform, velocity, inv_mass, gravity) in query.iter() {
            let mass = inv_mass.mass();
            let has_gravity = gravity.is_some();
            let speed = velocity.length();
            
            println!(
                "Body: pos={:.2}, vel={:.2}, speed={:.2}, mass={:.1}, gravity={}",
                transform.translation,
                velocity.0,
                speed,
                mass,
                has_gravity
            );
        }
    }
}

fn debug_draw_constraints(
    mut gizmos: Gizmos,
    constraints: Query<&ConstraintConfig, With<Active>>,
    bodies: Query<&Transform, With<RigidBodyMarker>>,
    anchors: Query<&Transform, With<AnchorMarker>>,
) {
    for constraint in constraints.iter() {
        // Get body transforms
        let Ok(body0_transform) = bodies.get(constraint.body_a) else {
            continue;
        };
        let Ok(body1_transform) = bodies.get(constraint.body_b) else {
            continue;
        };

        // Get anchor local transforms
        let Ok(anchor0_local) = anchors.get(constraint.anchor_a) else {
            continue;
        };
        let Ok(anchor1_local) = anchors.get(constraint.anchor_b) else {
            continue;
        };

        // Calculate world positions of anchors
        let anchor0_world = calc_world_pos(
            &body0_transform.translation,
            &body0_transform.rotation,
            &anchor0_local.translation,
        );
        
        let anchor1_world = calc_world_pos(
            &body1_transform.translation,
            &body1_transform.rotation,
            &anchor1_local.translation,
        );

        // Draw the constraint as a red line
        gizmos.line(anchor0_world, anchor1_world, Color::srgb(1.0, 0.0, 0.0));
        
        // Optional: Draw small spheres at anchor points for better visibility
        gizmos.sphere(anchor0_world, 0.05, Color::srgb(1.0, 0.5, 0.0)); // Orange
        gizmos.sphere(anchor1_world, 0.05, Color::srgb(1.0, 0.5, 0.0)); // Orange

        // Optional: Draw the rest length as a yellow line for comparison
        if let ConstraintParams::Distance { rest_length, .. } = constraint.params {
            let direction = (anchor1_world - anchor0_world).normalize();
            let rest_end = anchor0_world + direction * rest_length;
            gizmos.line(anchor0_world, rest_end, Color::srgb(1.0, 1.0, 0.0)); // Yellow
        }
    }
}