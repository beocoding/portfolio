use bevy::{
    prelude::*
};
use xpbd::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "XPBD Demo".into(),
                canvas: Some("#bevy".into()),
                fit_canvas_to_parent: true,
                prevent_default_event_handling: false,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(XPBDPlugin::default())
        .add_systems(Startup, (setup_scene, setup_simple_hinge_demo))
        .run();
}


#[inline(always)]
pub fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Light
    commands.spawn((
        DirectionalLight {
            illuminance: 10000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    
    // Camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(10.0, 10.0, 10.0).looking_at(Vec3::new(0.0, 2.5, 0.0), Vec3::Y),
    ));
    
    // Ground plane
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(20.0, 20.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.3, 0.5, 0.3))),
    ));
}


#[inline(always)]
pub fn setup_distance_constraint_test(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let solver = SolverConfig::default();
    
    // Static anchor point (won't move) - Yellow
    let anchor_body = RigidBody::spawn_with_mass(
        &mut commands,
        &mut meshes,
        &mut materials,
        Vec3::ZERO,
        Vec3::ZERO,
        0.0,  // infinite mass - static
        Transform::from_xyz(0.0, 5.0, 0.0),
        ColliderShape::Sphere { radius: 0.3 },
        Some(Color::srgb(1.0, 1.0, 0.2)),
        &solver,
    );
    
    // Dynamic hanging body - Cyan
    let hanging_body = RigidBody::spawn_with_density(
        &mut commands,
        &mut meshes,
        &mut materials,
        Vec3::new(1.0, 0.0, 0.0),  // initial sideways velocity
        Vec3::ZERO,
        1.0,  // density
        Transform::from_xyz(0.0, 3.0, 0.0),  // 2 units below anchor
        ColliderShape::Cuboid { half_size: Vec3::splat(0.4) },
        Some(Color::srgb(0.2, 1.0, 1.0)),
        &solver,
    );
    commands.entity(hanging_body).insert(ApplyGravity);
    
    // Create anchor points (attachment points on the bodies)
    let anchor0 = AnchorPoint::spawn(
        &mut commands,
        Transform::from_xyz(0.0, -0.3, 0.0),  // bottom of static sphere
        anchor_body,
    );
    
    let anchor1 = AnchorPoint::spawn(
        &mut commands,
        Transform::from_xyz(0.0, 0.4, 0.0),  // top of hanging cube
        hanging_body,
    );
    
    // Create distance constraint
    let constraint_config = ConstraintConfig {
        anchor_a: anchor0,
        body_a: anchor_body,
        anchor_b: anchor1,
        body_b: hanging_body,
        linear_damping: 0.0,
        angular_damping: 0.0,
        params: ConstraintParams::Distance {
            rest_length: 2.0,  // 2 units apart
            compliance: 0.0001,  // slightly soft
            unilateral: false,
        },
    };
    
    Constraint::spawn_constraint(
        &mut commands,
        constraint_config,
        solver,
    );
    
    // Add a second pendulum for comparison - Red
    let hanging_body2 = RigidBody::spawn_with_density(
        &mut commands,
        &mut meshes,
        &mut materials,
        Vec3::ZERO,
        Vec3::ZERO,
        1.5,  // heavier
        Transform::from_xyz(2.0, 2.0, 0.0),
        ColliderShape::Sphere { radius: 0.5 },
        Some(Color::srgb(1.0, 0.2, 0.2)),
        &solver,
    );
    commands.entity(hanging_body2).insert(ApplyGravity);
    
    let anchor2 = AnchorPoint::spawn(
        &mut commands,
        Transform::from_xyz(0.0, -0.3, 0.0),
        anchor_body,
    );
    
    let anchor3 = AnchorPoint::spawn(
        &mut commands,
        Transform::from_xyz(0.0, 0.5, 0.0),
        hanging_body2,
    );
    
    let constraint_config2 = ConstraintConfig {
        anchor_a: anchor2,
        body_a: anchor_body,
        anchor_b: anchor3,
        body_b: hanging_body2,
        linear_damping: 0.0,
        angular_damping: 0.0,
        params: ConstraintParams::Distance {
            rest_length: 3.0,  // longer rope
            compliance: 0.001,  // softer
            unilateral: false,
        },
    };
    
    Constraint::spawn_constraint(
        &mut commands,
        constraint_config2,
        solver,
    );
    
    // ============================================
    // NEW: Connect cube and sphere together
    // ============================================
    
    // Anchor on the cube (right side)
    let anchor_cube_right = AnchorPoint::spawn(
        &mut commands,
        Transform::from_xyz(0.4, 0.0, 0.0),  // right side of cube
        hanging_body,
    );
    
    // Anchor on the sphere (left side)
    let anchor_sphere_left = AnchorPoint::spawn(
        &mut commands,
        Transform::from_xyz(-0.5, 0.0, 0.0),  // left side of sphere
        hanging_body2,
    );
    
    // Create rigid distance constraint between cube and sphere
    let constraint_config3 = ConstraintConfig {
        anchor_a: anchor_cube_right,
        body_a: hanging_body,
        anchor_b: anchor_sphere_left,
        body_b: hanging_body2,
        linear_damping: 0.0,
        angular_damping: 0.0,
        params: ConstraintParams::Distance {
            rest_length: 1.5,  // 1.5 units apart
            compliance: 0.0,    // completely rigid
            unilateral: false,
        },
    };
    
    Constraint::spawn_constraint(
        &mut commands,
        constraint_config3,
        solver,
    );
}
#[inline(always)]
pub fn setup_simple_hinge_demo(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let solver = SolverConfig::default();
    
    // ============================================
    // First Example: Door hinge
    // ============================================
    
    let wall = RigidBody::spawn_with_mass(
        &mut commands,
        &mut meshes,
        &mut materials,
        Vec3::ZERO,
        Vec3::ZERO,
        0.0,
        Transform::from_xyz(-2.0, 3.0, 0.0),
        ColliderShape::Cuboid { half_size: Vec3::new(0.2, 1.0, 0.5) },
        Some(Color::srgb(0.3, 0.3, 0.3)),
        &solver,
    );
    
    let door = RigidBody::spawn_with_density(
        &mut commands,
        &mut meshes,
        &mut materials,
        Vec3::ZERO,
        Vec3::ZERO,
        1.0,
        Transform::from_xyz(-0.5, 3.0, 0.0)
            .with_rotation(Quat::from_rotation_y(0.3)),
        ColliderShape::Cuboid { half_size: Vec3::new(1.0, 0.8, 0.1) },
        Some(Color::srgb(0.6, 0.4, 0.2)),
        &solver,
    );
    commands.entity(door).insert(ApplyGravity);
    
    let wall_anchor = AnchorPoint::spawn(
        &mut commands,
        Transform::from_xyz(0.2, 0.0, 0.0)
            .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
        wall,
    );
    
    let door_anchor = AnchorPoint::spawn(
        &mut commands,
        Transform::from_xyz(-1.0, 0.0, 0.0)
            .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
        door,
    );
    
    let hinge_config = ConstraintConfig {
        anchor_a: wall_anchor,
        body_a: wall,
        anchor_b: door_anchor,
        body_b: door,
        linear_damping: 0.0,
        angular_damping: 0.1,
        params: ConstraintParams::Hinge {
            swing_min: -std::f32::consts::FRAC_PI_2,
            swing_max: std::f32::consts::FRAC_PI_2,
            target_angle: None,
        },
    };
    
    Constraint::spawn_constraint(&mut commands, hinge_config, solver);
    
    // ============================================
    // Second Example: Servo-controlled arm
    // ============================================
    
    let arm_anchor = RigidBody::spawn_with_mass(
        &mut commands,
        &mut meshes,
        &mut materials,
        Vec3::ZERO,
        Vec3::ZERO,
        0.0,
        Transform::from_xyz(2.0, 5.0, 0.0),
        ColliderShape::Sphere { radius: 0.3 },
        Some(Color::srgb(1.0, 1.0, 0.2)),
        &solver,
    );
    
    let arm = RigidBody::spawn_with_density(
        &mut commands,
        &mut meshes,
        &mut materials,
        Vec3::ZERO,
        Vec3::ZERO,
        1.0,
        Transform::from_xyz(2.0, 3.5, 0.0),
        ColliderShape::Cuboid { half_size: Vec3::new(0.2, 1.0, 0.2) },
        Some(Color::srgb(0.8, 0.2, 0.8)),
        &solver,
    );
    commands.entity(arm).insert(ApplyGravity);
    
    let anchor_static = AnchorPoint::spawn(
        &mut commands,
        Transform::from_xyz(0.0, -0.3, 0.0),
        arm_anchor,
    );
    
    let anchor_arm = AnchorPoint::spawn(
        &mut commands,
        Transform::from_xyz(0.0, 1.0, 0.0),
        arm,
    );
    
    let servo_config = ConstraintConfig {
        anchor_a: anchor_static,
        body_a: arm_anchor,
        anchor_b: anchor_arm,
        body_b: arm,
        linear_damping: 0.0,
        angular_damping: 0.05,
        params: ConstraintParams::Hinge {
            swing_min: -std::f32::consts::PI,
            swing_max: std::f32::consts::PI,
            target_angle: Some(TargetAngle {
                angle: std::f32::consts::FRAC_PI_4,
                compliance: 0.001,
            }),
        },
    };
    
    Constraint::spawn_constraint(&mut commands, servo_config, solver);
    
    // ============================================
    // Third Example: Chain of hinged segments
    // ============================================
    
    let chain_anchor = RigidBody::spawn_with_mass(
        &mut commands,
        &mut meshes,
        &mut materials,
        Vec3::ZERO,
        Vec3::ZERO,
        0.0,
        Transform::from_xyz(5.0, 5.0, 0.0),
        ColliderShape::Sphere { radius: 0.3 },
        Some(Color::srgb(1.0, 0.6, 0.2)),
        &solver,
    );
    
    let num_segments = 4;
    let segment_length = 0.8;
    let mut prev_body = chain_anchor;
    
    for i in 0..num_segments {
        let segment = RigidBody::spawn_with_density(
            &mut commands,
            &mut meshes,
            &mut materials,
            Vec3::ZERO,
            Vec3::ZERO,
            0.5,
            Transform::from_xyz(5.0, 5.0 - (i as f32 + 1.0) * segment_length, 0.0),
            ColliderShape::Cuboid { half_size: Vec3::new(0.15, segment_length * 0.4, 0.15) },
            Some(Color::srgb(
                0.2 + i as f32 * 0.2,
                0.5,
                0.8 - i as f32 * 0.15,
            )),
            &solver,
        );
        commands.entity(segment).insert(ApplyGravity);
        
        // Quat::IDENTITY keeps hinge axis along world X — correct for pendulum-style swing
        let anchor_prev = AnchorPoint::spawn(
            &mut commands,
            Transform::from_xyz(0.0, if i == 0 { -0.3 } else { -segment_length * 0.4 }, 0.0),
            prev_body,
        );
        
        let anchor_curr = AnchorPoint::spawn(
            &mut commands,
            Transform::from_xyz(0.0, segment_length * 0.4, 0.0),
            segment,
        );
        
        let chain_config = ConstraintConfig {
            anchor_a: anchor_prev,
            body_a: prev_body,
            anchor_b: anchor_curr,
            body_b: segment,
            linear_damping: 0.0,
            angular_damping: 0.02,
            params: ConstraintParams::Hinge {
                swing_min: -std::f32::consts::FRAC_PI_3,
                swing_max: std::f32::consts::FRAC_PI_3,
                target_angle: None,
            },
        };
        
        Constraint::spawn_constraint(&mut commands, chain_config, solver);
        
        prev_body = segment;
    }
}