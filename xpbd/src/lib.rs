use ahash::{AHashMap, AHashSet};
use bevy::prelude::*;
pub mod observers;
pub mod debug;
pub use debug::*;
pub mod instance_manager;
pub use instance_manager::*;
use crate::{instance_manager::instances::InstanceManager, observers::validate_new_constraints};


#[derive(Resource, Default)]
pub struct MouseDragState {
    pub is_dragging: bool,
    pub drag_constraint: Option<Entity>,
    pub kinematic_anchor: Option<Entity>,
}

#[inline(always)]
pub fn handle_mouse_drag(
    mut commands: Commands,
    mut drag_state: ResMut<MouseDragState>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    bodies_query: Query<(Entity, &Transform, &SolverConfig, &InverseMass), With<RigidBodyMarker>>,
    constraints: Query<&ConstraintConfig>,
) {
    let Ok(window) = windows.single() else { return };
    let Ok((camera, camera_transform)) = cameras.single() else { return };
    let Some(cursor_pos) = window.cursor_position() else { return };
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_pos) else { return };

    // ===== MOUSE PRESSED - START DRAG =====
    if mouse_button.just_pressed(MouseButton::Left) && !drag_state.is_dragging {
        let mut closest_hit: Option<(Entity, Vec3, f32, SolverConfig)> = None;

        for (entity, transform, config, inv_mass) in bodies_query.iter() {
            if inv_mass.0 == 0.0 { continue; }

            let to_sphere = transform.translation - ray.origin;
            let t = to_sphere.dot(*ray.direction);

            if t > 0.0 {
                let closest_point = ray.origin + *ray.direction * t;
                let distance = closest_point.distance(transform.translation);

                if distance < 1.5 {
                    if closest_hit.is_none() || t < closest_hit.as_ref().unwrap().2 {
                        closest_hit = Some((entity, closest_point, t, *config));
                    }
                }
            }
        }

        if let Some((body, hit_pos, _, body_config)) = closest_hit {
            let kinematic_anchor = commands.spawn((
                Transform::from_translation(hit_pos),
                Velocity::default(),
                AngularVelocity::default(),
                InverseMass(0.0),
                InverseInertia::default(),
                body_config,
                RigidBodyMarker,
            )).id();

            let anchor_kinematic = AnchorPoint::spawn(&mut commands, Transform::IDENTITY, kinematic_anchor);

            let body_transform = bodies_query.get(body).unwrap().1;
            let local_grab_pos = calc_local_pos(
                &body_transform.translation,
                &body_transform.rotation,
                &hit_pos,
            );
            let anchor_body = AnchorPoint::spawn(
                &mut commands,
                Transform::from_translation(local_grab_pos),
                body,
            );

            let grab_config = ConstraintConfig {
                anchor_a: anchor_kinematic,
                body_a: kinematic_anchor,
                anchor_b: anchor_body,
                body_b: body,
                linear_damping: 0.0,
                angular_damping: 0.0,
                params: ConstraintParams::Grab { compliance: 0.01 },
            };

            let constraint = Constraint::spawn_constraint(&mut commands, grab_config, body_config);

            drag_state.is_dragging = true;
            drag_state.drag_constraint = Some(constraint);
            drag_state.kinematic_anchor = Some(kinematic_anchor);
        }
    }

    // ===== MOUSE MOVED - UPDATE DRAG POSITION =====
    if drag_state.is_dragging {
        if let (Some(constraint_entity), Some(kinematic_entity)) =
            (drag_state.drag_constraint, drag_state.kinematic_anchor)
        {
            if let Ok(constraint_config) = constraints.get(constraint_entity) {
                let body_entity = constraint_config.body_b;
                if let Ok((_, body_transform, _, _)) = bodies_query.get(body_entity) {
                    let depth = (body_transform.translation - ray.origin)
                        .dot(*ray.direction)
                        .max(1.0);
                    let world_pos = ray.origin + *ray.direction * depth;
                    commands.entity(kinematic_entity).insert(Transform::from_translation(world_pos));
                }
            }
        }
    }

    // ===== MOUSE RELEASED - END DRAG =====
    if mouse_button.just_released(MouseButton::Left) && drag_state.is_dragging {
        if let Some(constraint) = drag_state.drag_constraint {
            commands.entity(constraint).despawn();
        }
        if let Some(kinematic) = drag_state.kinematic_anchor {
            commands.entity(kinematic).despawn();
        }
        drag_state.is_dragging = false;
        drag_state.drag_constraint = None;
        drag_state.kinematic_anchor = None;
    }
}

#[derive(Default)]
pub struct XPBDPlugin{
    pub debug: bool,
}

impl Plugin for XPBDPlugin {
    fn build(&self, app: &mut App) {
        if self.debug {
            app.add_plugins(XPBDDebugPlugin);
        };
        app
            .insert_resource(Gravity::default())
            .insert_resource(MouseDragState::default())
            .insert_resource(Time::<Fixed>::from_hz(64.0))
            .insert_resource(ConstraintSolverPool::default())
            .add_observer(on_constraint_remove)
            .add_systems(FixedUpdate, (
                validate_new_constraints,
                capture_rbs,
                solve_rbs,
                writeback_rb,
            ).chain())
            .add_systems(Update, handle_mouse_drag)
            ;
    }
}

#[derive(Component)]
pub enum ColliderShape {
    Cuboid { half_size: Vec3 },
    Sphere { radius: f32 },
    Capsule { radius: f32, half_length: f32 },
    Cylinder { radius: f32, half_height: f32 }
}

impl ColliderShape {
    #[inline(always)]
    pub fn mesh(&self) -> Mesh {
        match self {
            ColliderShape::Cuboid { half_size } => Mesh::from(Cuboid { half_size: *half_size }),
            ColliderShape::Sphere { radius } => Mesh::from(Sphere { radius: *radius }),
            ColliderShape::Capsule { radius, half_length } => {
                Mesh::from(Capsule3d { radius: *radius, half_length: *half_length })
            }
            ColliderShape::Cylinder { radius, half_height } => {
                Mesh::from(Cylinder { radius: *radius, half_height: *half_height })
            }
        }
    }
    #[inline(always)]
    pub fn volume(&self) -> f32 {
        const SPHERE_VOLUME_FACTOR: f32 = (4.0 / 3.0) * std::f32::consts::PI;
        const CYLINDER_VOLUME_FACTOR: f32 = std::f32::consts::PI;

        match self {
            ColliderShape::Cuboid { half_size } => {
                let size = *half_size * 2.0;
                size.x * size.y * size.z
            }
            ColliderShape::Sphere { radius } => {
                (4.0 / 3.0) * std::f32::consts::PI * radius.powi(3)
            }
            ColliderShape::Capsule { radius, half_length } => {
                let cylinder_height = half_length * 2.0;
                let cylinder_vol = CYLINDER_VOLUME_FACTOR * cylinder_height * radius.powi(2);
                let sphere_vol = SPHERE_VOLUME_FACTOR * radius.powi(3);
                cylinder_vol + sphere_vol
            }
            ColliderShape::Cylinder { radius, half_height } => {
                let height = half_height * 2.0;
                std::f32::consts::PI * radius.powi(2) * height
            }
        }
    }

    #[inline(always)]
    pub fn calculate_inverse_mass_from_density(&self, density: f32) -> InverseMass {
        InverseMass::from_mass(self.volume() * density)
    }

    #[inline(always)]
    pub fn calculate_inverse_inertia(&self, inverse_mass: InverseMass) -> InverseInertia {
        match self {
            ColliderShape::Cuboid { half_size } => {
                let size = half_size * 2.0;
                let inv_mass = inverse_mass.0;

                let denom_x = size.y * size.y + size.z * size.z;
                let denom_y = size.x * size.x + size.z * size.z;
                let denom_z = size.x * size.x + size.y * size.y;

                InverseInertia(Vec3::new(
                    12.0 * inv_mass * (1.0 / denom_x),
                    12.0 * inv_mass * (1.0 / denom_y),
                    12.0 * inv_mass * (1.0 / denom_z),
                ))
            }

            ColliderShape::Sphere { radius } => {
                let inv_i = 5.0 / 2.0 * inverse_mass.0 / (radius * radius);
                InverseInertia(Vec3::splat(inv_i))
            }

            ColliderShape::Capsule { radius, half_length } => {
                let height = half_length * 2.0;
                let inv_ix = 12.0 * inverse_mass.0 / (3.0 * radius * radius + height * height);
                let inv_iy = 2.0 * inverse_mass.0 / (radius * radius);
                InverseInertia(Vec3::new(inv_ix, inv_iy, inv_ix))
            }

            ColliderShape::Cylinder { radius, half_height } => {
                let height = half_height * 2.0;
                let inv_ix = 12.0 * inverse_mass.0 / (3.0 * radius * radius + height * height);
                let inv_iy = 2.0 * inverse_mass.0 / (radius * radius);
                InverseInertia(Vec3::new(inv_ix, inv_iy, inv_ix))
            }
        }
    }
}

#[derive(Component, Default, Clone, Copy, Deref, DerefMut)]
pub struct Velocity(pub Vec3);

#[derive(Component, Default, Clone, Copy, Deref, DerefMut)]
pub struct AngularVelocity(pub Vec3);

#[derive(Component, Clone, Copy, Deref, DerefMut)]
pub struct InverseMass(pub f32);
impl Default for InverseMass {
    fn default() -> Self {
        Self(0.0)
    }
}
impl InverseMass {
    #[inline(always)]
    pub fn from_mass(mass: f32) -> Self {
        if mass.abs() > f32::EPSILON {
            Self(1.0 / mass)
        } else {
            Self(0.0)
        }
    }
    #[inline(always)]
    pub fn mass(&self) -> f32 {
        if self.0.abs() > f32::EPSILON {
            1.0 / self.0
        } else {
            0.0
        }
    }
}

#[derive(Component, Default, Clone, Copy, Deref, DerefMut)]
pub struct InverseInertia(pub Vec3);
impl InverseInertia {
    #[inline(always)]
    pub fn from_inertia(inertia: Vec3) -> Self {
        if inertia.x.abs() > f32::EPSILON
            && inertia.y.abs() > f32::EPSILON
            && inertia.z.abs() > f32::EPSILON
        {
            Self(Vec3::new(1.0 / inertia.x, 1.0 / inertia.y, 1.0 / inertia.z))
        } else {
            Self::default()
        }
    }

    #[inline(always)]
    pub fn inertia(&self) -> Vec3 {
        Vec3::new(
            if self.x.abs() > f32::EPSILON { 1.0 / self.x } else { 0.0 },
            if self.y.abs() > f32::EPSILON { 1.0 / self.y } else { 0.0 },
            if self.z.abs() > f32::EPSILON { 1.0 / self.z } else { 0.0 },
        )
    }
}

#[derive(Component, Default, Clone, Copy)]
pub struct AttachmentPoint;

#[derive(Component, Default, Clone, Copy)]
pub struct RigidBodyMarker;

#[derive(Bundle, Default)]
pub struct RigidBody {
    pub velocity: Velocity,
    pub angular_velocity: AngularVelocity,
    pub inverse_inertia: InverseInertia,
    pub inverse_mass: InverseMass,
    pub transform: Transform,
    pub mesh: Mesh3d,
    pub solver: SolverConfig,
    pub material: MeshMaterial3d<StandardMaterial>,
    pub visibility: Visibility,
    pub inherited_visibility: InheritedVisibility,
    pub marker: RigidBodyMarker,
}

impl RigidBody {
    #[inline(always)]
    pub fn spawn_temporary(
        commands: &mut Commands,
        position: Vec3,
    ) -> Entity {
        let entity_commands = commands.spawn((
            Transform::from_translation(position),
            InverseMass(0.0),
            RigidBodyMarker,
        ));
        entity_commands.id()
    }

    #[inline(always)]
    pub fn spawn_with_mass(
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        velocity: Vec3,
        angular_velocity: Vec3,
        mass: f32,
        transform: Transform,
        collider_type: ColliderShape,
        color: Option<Color>,
        solver: &SolverConfig,
    ) -> Entity {
        let velocity = Velocity(velocity);
        let angular_velocity = AngularVelocity(angular_velocity);
        let inverse_mass = InverseMass::from_mass(mass);
        let inverse_inertia = collider_type.calculate_inverse_inertia(inverse_mass);
        let mesh = Mesh3d(meshes.add(collider_type.mesh()));
        let material = MeshMaterial3d(materials.add(color.unwrap_or(Color::WHITE)));

        commands.spawn(RigidBody {
            velocity,
            angular_velocity,
            inverse_inertia,
            inverse_mass,
            transform,
            mesh,
            material,
            solver: *solver,
            ..default()
        }).id()
    }

    #[inline(always)]
    pub fn spawn_with_density(
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        velocity: Vec3,
        angular_velocity: Vec3,
        density: f32,
        transform: Transform,
        collider_type: ColliderShape,
        color: Option<Color>,
        solver: &SolverConfig,
    ) -> Entity {
        let velocity = Velocity(velocity);
        let angular_velocity = AngularVelocity(angular_velocity);
        let inverse_mass = collider_type.calculate_inverse_mass_from_density(density);
        let inverse_inertia = collider_type.calculate_inverse_inertia(inverse_mass);
        let mesh = Mesh3d(meshes.add(collider_type.mesh()));
        let material = MeshMaterial3d(materials.add(color.unwrap_or(Color::WHITE)));

        commands.spawn(RigidBody {
            velocity,
            angular_velocity,
            inverse_inertia,
            inverse_mass,
            transform,
            mesh,
            material,
            solver: *solver,
            ..default()
        }).id()
    }
}

#[derive(Component, Default, Clone, Copy)]
pub struct AnchorMarker;

#[derive(Bundle)]
pub struct AnchorPoint {
    pub transform: Transform,
    pub parent: ChildOf,
    marker: AnchorMarker,
}
impl AnchorPoint {
    #[inline(always)]
    pub fn spawn(
        commands: &mut Commands,
        offset: Transform,
        parent: Entity,
    ) -> Entity {
        commands.spawn(Self {
            transform: offset,
            marker: AnchorMarker,
            parent: ChildOf(parent),
        }).id()
    }
}

#[derive(Resource, Copy, Clone)]
pub struct Gravity(pub Vec3);
impl Default for Gravity {
    fn default() -> Self {
        Self(Vec3::new(0.0, -9.8, 0.0))
    }
}

#[derive(Copy, Clone, Deref, DerefMut, Default)]
pub struct PrevPosition(pub Vec3);

#[derive(Copy, Clone, Deref, DerefMut, Default)]
pub struct CurrentPosition(pub Vec3);

#[derive(Copy, Clone, Deref, DerefMut, Default)]
pub struct PrevRotation(pub Quat);

#[derive(Copy, Clone, Deref, DerefMut, Default)]
pub struct CurrentRotation(pub Quat);

#[derive(Debug, Component, Default, Clone)]
pub struct ApplyGravity;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyFlag {
    Dynamic   = 1 << 0,
    Gravity   = 1 << 1,
}

#[derive(Default, Clone, Copy)]
pub struct BodyFlags(pub u8);

impl BodyFlags {
    #[inline(always)]
    pub const fn has_flag(&self, flag: BodyFlag) -> bool {
        self.0 & (flag as u8) != 0
    }
    #[inline(always)]
    pub const fn gravity(&self) -> bool {
        self.has_flag(BodyFlag::Gravity)
    }
}

#[derive(Debug, Component, Clone, Copy, Eq, PartialEq, Hash)]
pub struct SolverConfig {
    pub steps: u32,
    pub iterations: u32,
}

impl Default for SolverConfig {
    fn default() -> Self {
        Self { steps: 20, iterations: 1 }
    }
}

impl SolverConfig {
    pub const NONE: Self = Self { steps: 0, iterations: 0 };
    #[inline(always)]
    pub const fn key(&self) -> SolverConfigKey {
        SolverConfigKey::from_config(self)
    }
}

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, Hash)]
pub struct SolverConfigKey(pub u32);

impl SolverConfigKey {
    #[inline(always)]
    pub const fn from_config(config: &SolverConfig) -> Self {
        SolverConfigKey((config.steps << 16) | (config.iterations & 0xFFFF))
    }

    #[inline(always)]
    pub const fn steps(&self) -> u32 {
        self.0 >> 16
    }

    #[inline(always)]
    pub const fn iterations(&self) -> u32 {
        self.0 & 0xFFFF
    }
}

#[inline(always)]
fn get_pair_mut<'a, K: std::hash::Hash + Eq, V>(
    map: &'a mut AHashMap<K, V>,
    k1: &K,
    k2: &K,
) -> Result<(&'a mut V, &'a mut V), &'static str> {
    if std::ptr::eq(k1, k2) {
        return Err("cannot get two mutable references to the same key");
    }

    let v1 = map.get_mut(k1).ok_or("key1 not found")? as *mut V;
    let v2 = map.get_mut(k2).ok_or("key2 not found")? as *mut V;

    Ok(unsafe { (&mut *v1, &mut *v2) })
}

#[derive(Default, Clone)]
pub struct ConstraintSolver {
    pub rigid_bodies: AHashMap<Entity, RigidBodyCapture>,
    pub constraints: InstanceManager<Entity, ConstraintConfig>,
}

impl ConstraintSolver {
    pub fn new() -> Self {
        Self {
            rigid_bodies: AHashMap::default(),
            constraints: InstanceManager::new(),
        }
    }

    #[inline(always)]
    pub fn solve(&mut self, steps: u32, iterations: u32, gravity: Gravity, dt: f32) {
        let g_vel = gravity.0;
        let sdt = dt / steps as f32;

        for _step in 0..steps {
            // Integration phase
            for (_entity, body) in self.rigid_bodies.iter_mut() {
                body.integrate(g_vel * sdt, sdt);
            }

            // Constraint solving phase
            for _iter in 0..iterations {
                for (_entity, constraint) in self.constraints.iter() {
                    let (body_a, body_b) = get_pair_mut(&mut self.rigid_bodies, &constraint.body_a, &constraint.body_b)
                        .expect("Bodies not found for constraint solving");
                    constraint.solve(body_a, body_b, sdt);
                }
            }

            // Damping
            for (_entity, constraint) in self.constraints.iter() {
                let (body_a, body_b) = get_pair_mut(&mut self.rigid_bodies, &constraint.body_a, &constraint.body_b)
                    .expect("Bodies not found for damping");
                constraint.apply_damping(body_a, body_b, sdt);
            }

            // Update velocities
            for (_entity, body) in self.rigid_bodies.iter_mut() {
                body.update_velocities(sdt);
            }
        }
    }

    #[inline(always)]
    pub fn insert_rb(&mut self, entity: Entity, data: Option<RigidBodyCapture>) {
        let data = data.unwrap_or_default();
        self.rigid_bodies.insert(entity, data);
    }

    #[inline(always)]
    pub fn insert_constraint(&mut self, entity: Entity, config: &ConstraintConfig) {
        self.constraints.insert(entity, config.clone());
    }

    #[inline(always)]
    pub fn remove_constraint(&mut self, entity: Entity) {
        self.constraints.remove(entity);
    }
}

#[derive(Resource, Default)]
pub struct ConstraintSolverPool {
    pub solvers: InstanceManager<SolverConfigKey, ConstraintSolver>,
    pub rb_members: InstanceManager<Entity, SolverConfigKey>,
}

impl ConstraintSolverPool {
    #[inline(always)]
    pub fn get_or_create_solver(&mut self, config: &SolverConfig) -> &mut ConstraintSolver {
        let key = config.key();
        self.solvers
            .entry(key)
            .or_insert_with(|| ConstraintSolver::new())
    }

    #[inline(always)]
    pub fn validate_rb(&self, rb: Entity, solver_key: SolverConfigKey) -> Result<(), String> {
        if let Some(existing_key) = self.rb_members.get(rb) {
            if *existing_key != solver_key {
                return Err(format!(
                    "RigidBody {:?} already exists in solver group (steps: {}, iterations: {}). \
                    Cannot add to different solver group (steps: {}, iterations: {}).",
                    rb,
                    existing_key.steps(),
                    existing_key.iterations(),
                    solver_key.steps(),
                    solver_key.iterations()
                ));
            }
        }
        Ok(())
    }

    #[inline(always)]
    pub fn solve_all(&mut self, gravity: Gravity, dt: f32) {
        for (key, solver) in self.solvers.iter_mut() {
            solver.solve(key.steps(), key.iterations(), gravity, dt);
        }
    }

    #[inline(always)]
    pub fn clear_data(&mut self) {
        self.rb_members.clear();
        for solver in self.solvers.values_mut() {
            solver.rigid_bodies.clear();
            solver.constraints.clear();
        }
    }

    #[inline(always)]
    pub fn prune_empty_solvers(&mut self) {
        self.solvers.retain(|_, solver| {
            !solver.rigid_bodies.is_empty() || !solver.constraints.is_empty()
        });
    }

    #[inline(always)]
    pub fn insert_rb(
        &mut self,
        config: &SolverConfig,
        rb: Entity,
        data: Option<RigidBodyCapture>,
    ) -> Result<(), String> {
        let key = config.key();
        self.validate_rb(rb, key)?;
        self.rb_members.insert(rb, key);
        let solver = self.get_or_create_solver(config);
        solver.insert_rb(rb, data);
        Ok(())
    }

    #[inline(always)]
    pub fn remove_rb(&mut self, rb: Entity) {
        if let Some(key) = self.rb_members.remove(rb) {
            if let Some(solver) = self.solvers.get_mut(key) {
                solver.rigid_bodies.remove(&rb);
            }
        }
    }

    #[inline(always)]
    pub fn insert_constraint(
        &mut self,
        solver_config: &SolverConfig,
        constraint: Entity,
        constraint_config: &ConstraintConfig,
    ) {
        let solver = self.get_or_create_solver(solver_config);
        solver.insert_constraint(constraint, constraint_config);
    }
}

#[derive(Default, Clone, Deref, DerefMut)]
pub struct RigidBodyAnchors(InstanceManager<Entity, Transform>);

#[derive(Default, Clone)]
pub struct RigidBodyCapture {
    pub anchors: RigidBodyAnchors,
    pub prev_rot: PrevRotation,
    pub curr_rot: CurrentRotation,
    pub prev_pos: PrevPosition,
    pub curr_pos: CurrentPosition,
    pub velocity: Velocity,
    pub omega: AngularVelocity,
    pub inv_mass: InverseMass,
    pub inv_inertia: InverseInertia,
    pub flags: BodyFlags,
}

impl RigidBodyCapture {
    #[inline(always)]
    fn integrate(&mut self, g_vel: Vec3, sdt: f32) {
        if self.inv_mass.0 == 0.0 || !self.flags.gravity() { return; }

        *self.prev_pos = *self.curr_pos;
        *self.prev_rot = *self.curr_rot;

        self.velocity.0 += g_vel;
        *self.curr_pos += self.velocity.0 * sdt;

        let d_rot = Quat::from_xyzw(self.omega.x, self.omega.y, self.omega.z, 0.0) * *self.curr_rot;
        self.curr_rot.x += 0.5 * sdt * d_rot.x;
        self.curr_rot.y += 0.5 * sdt * d_rot.y;
        self.curr_rot.z += 0.5 * sdt * d_rot.z;
        self.curr_rot.w += 0.5 * sdt * d_rot.w;
        *self.curr_rot = self.curr_rot.normalize();
    }

    #[inline(always)]
    fn update_velocities(&mut self, sdt: f32) {
        if self.inv_mass.0 == 0.0 { return; }

        let pos_diff = *self.curr_pos - *self.prev_pos;
        *self.velocity = pos_diff / sdt;

        let d_rot = *self.curr_rot * self.prev_rot.inverse();
        self.omega.x = d_rot.x * 2.0 / sdt;
        self.omega.y = d_rot.y * 2.0 / sdt;
        self.omega.z = d_rot.z * 2.0 / sdt;

        if d_rot.w < 0.0 {
            *self.omega = -*self.omega;
        }
    }

    #[inline(always)]
    fn apply_correction(&mut self, correction: Vec3, contact_point: Vec3) {
        if self.inv_mass.0 == 0.0 { return; }

        *self.curr_pos += correction * self.inv_mass.0;

        let r = contact_point - *self.curr_pos;
        let mut d_omega = r.cross(correction);

        let inv_rot = self.curr_rot.inverse();
        d_omega = inv_rot * d_omega;

        d_omega = Vec3::new(
            d_omega.x * self.inv_inertia.x,
            d_omega.y * self.inv_inertia.y,
            d_omega.z * self.inv_inertia.z,
        );

        d_omega = *self.curr_rot * d_omega;

        let d_rot = Quat::from_xyzw(d_omega.x, d_omega.y, d_omega.z, 0.0) * *self.curr_rot;
        self.curr_rot.x += 0.5 * d_rot.x;
        self.curr_rot.y += 0.5 * d_rot.y;
        self.curr_rot.z += 0.5 * d_rot.z;
        self.curr_rot.w += 0.5 * d_rot.w;
        *self.curr_rot = self.curr_rot.normalize();
    }

    #[inline(always)]
    fn apply_angular_correction(&mut self, correction: Vec3) {
        if self.inv_mass.0 == 0.0 { return; }

        let inv_rot = self.curr_rot.inverse();
        let mut d_omega = inv_rot * correction;

        d_omega = Vec3::new(
            d_omega.x * self.inv_inertia.x,
            d_omega.y * self.inv_inertia.y,
            d_omega.z * self.inv_inertia.z,
        );

        d_omega = *self.curr_rot * d_omega;

        let d_rot = Quat::from_xyzw(d_omega.x, d_omega.y, d_omega.z, 0.0) * *self.curr_rot;
        self.curr_rot.x += 0.5 * d_rot.x;
        self.curr_rot.y += 0.5 * d_rot.y;
        self.curr_rot.z += 0.5 * d_rot.z;
        self.curr_rot.w += 0.5 * d_rot.w;
        *self.curr_rot = self.curr_rot.normalize();
    }
}

impl RigidBodyCapture {
    #[inline(always)]
    pub fn get_velocity_at(&self, pos: Vec3) -> Vec3 {
        if self.inv_mass.0 == 0.0 { return Vec3::ZERO; }
        let r = pos - *self.curr_pos;
        self.velocity.0 + self.omega.0.cross(r)
    }

    #[inline(always)]
    pub fn apply_velocity_impulse(&mut self, impulse: Vec3, pos: Vec3) {
        if self.inv_mass.0 == 0.0 { return; }

        self.velocity.0 += impulse * self.inv_mass.0;

        let r = pos - *self.curr_pos;
        let mut torque = r.cross(impulse);

        let inv_rot = self.curr_rot.inverse();
        torque = inv_rot * torque;

        torque = Vec3::new(
            torque.x * self.inv_inertia.x,
            torque.y * self.inv_inertia.y,
            torque.z * self.inv_inertia.z,
        );

        torque = *self.curr_rot * torque;
        self.omega.0 += torque;
    }

    #[inline(always)]
    pub fn apply_angular_velocity_correction(&mut self, correction: Vec3) {
        if self.inv_mass.0 == 0.0 { return; }
        self.omega.0 += correction;
    }
}

#[inline(always)]
fn limit_angle(
    body_a: &mut RigidBodyCapture,
    body_b: &mut RigidBodyCapture,
    axis: Vec3,
    a: Vec3,
    b: Vec3,
    min_angle: f32,
    max_angle: f32,
    compliance: f32,
    sdt: f32,
) {
    let phi = get_angle(axis, a, b);

    if phi >= min_angle && phi <= max_angle { return; }

    let target_phi = phi.clamp(min_angle, max_angle);
    let rot = Quat::from_axis_angle(axis, target_phi);
    let ra = rot * a;
    let corr = ra.cross(b);

    if corr.length() < f32::EPSILON { return; }

    let c = corr.length();
    let normal = corr / c;

    let w0 = calc_effective_inv_mass(body_a, normal, None);
    let w1 = calc_effective_inv_mass(body_b, normal, None);
    let w = w0 + w1;

    if w < f32::EPSILON { return; }

    let alpha = compliance / (sdt * sdt);
    let lambda = -c / (w + alpha);
    let correction = normal * (-lambda);

    body_a.apply_angular_correction(correction);
    body_b.apply_angular_correction(-correction);
}

#[inline(always)]
fn get_anchor_positions(
    body_a: &RigidBodyCapture,
    body_b: &RigidBodyCapture,
    anchor_a: Entity,
    anchor_b: Entity,
) -> (Vec3, Vec3) {
    let anchor_a_trans = body_a.anchors.get(anchor_a).expect("Anchor A not found!");
    let anchor_b_trans = body_b.anchors.get(anchor_b).expect("Anchor B not found!");

    let pos0 = calc_world_pos(&body_a.curr_pos, &body_a.curr_rot, &anchor_a_trans.translation);
    let pos1 = calc_world_pos(&body_b.curr_pos, &body_b.curr_rot, &anchor_b_trans.translation);

    (pos0, pos1)
}

#[inline(always)]
fn get_anchor_rotations(
    body_a: &RigidBodyCapture,
    body_b: &RigidBodyCapture,
    anchor_a: Entity,
    anchor_b: Entity,
) -> (Quat, Quat) {
    let anchor_a_trans = body_a.anchors.get(anchor_a).expect("Anchor A not found!");
    let anchor_b_trans = body_b.anchors.get(anchor_b).expect("Anchor B not found!");

    let rot0 = calc_world_rot(&body_a.curr_rot, &anchor_a_trans.rotation);
    let rot1 = calc_world_rot(&body_b.curr_rot, &anchor_b_trans.rotation);

    (rot0, rot1)
}

#[inline(always)]
pub fn align_axes(
    body_a: &mut RigidBodyCapture,
    body_b: &mut RigidBodyCapture,
    axis0: Vec3,
    axis1: Vec3,
    compliance: f32,
    sdt: f32,
) {
    let corr = axis0.cross(axis1);

    if corr.length() < f32::EPSILON { return; }

    let c = corr.length();
    let normal = corr / c;

    let w0 = calc_effective_inv_mass(body_a, normal, None);
    let w1 = calc_effective_inv_mass(body_b, normal, None);
    let w = w0 + w1;

    if w < f32::EPSILON { return; }

    let alpha = compliance / (sdt * sdt);
    let lambda = -c / (w + alpha);
    let correction = normal * (-lambda);

    body_a.apply_angular_correction(correction);
    body_b.apply_angular_correction(-correction);
}

#[inline(always)]
pub fn calc_effective_inv_mass(body: &RigidBodyCapture, normal: Vec3, contact_point: Option<Vec3>) -> f32 {
    if body.inv_mass.0 == 0.0 { return 0.0; }

    let mut rn = if let Some(pos) = contact_point {
        let r = pos - *body.curr_pos;
        r.cross(normal)
    } else {
        normal
    };

    let inv_rot = body.curr_rot.inverse();
    rn = inv_rot * rn;

    let w_angular = rn.x * rn.x * body.inv_inertia.x
        + rn.y * rn.y * body.inv_inertia.y
        + rn.z * rn.z * body.inv_inertia.z;

    if contact_point.is_some() {
        w_angular + body.inv_mass.0
    } else {
        w_angular
    }
}

#[inline(always)]
pub fn get_angle(axis: Vec3, a: Vec3, b: Vec3) -> f32 {
    let c = a.cross(b);
    let mut phi = (c.dot(axis)).asin();

    if a.dot(b) < 0.0 {
        phi = std::f32::consts::PI - phi;
    }

    while phi > std::f32::consts::PI {
        phi -= 2.0 * std::f32::consts::PI;
    }
    while phi < -std::f32::consts::PI {
        phi += 2.0 * std::f32::consts::PI;
    }

    phi
}

#[inline(always)]
fn solve_distance(
    body_a: &mut RigidBodyCapture,
    body_b: &mut RigidBodyCapture,
    anchor_a: Entity,
    anchor_b: Entity,
    rest_length: f32,
    compliance: f32,
    unilateral: bool,
    sdt: f32,
) {
    let (anchor_world0, anchor_world1) = get_anchor_positions(body_a, body_b, anchor_a, anchor_b);

    let corr = anchor_world1 - anchor_world0;
    let distance = corr.length();

    if distance < f32::EPSILON { return; }

    let normal = corr / distance;
    let c = distance - rest_length;

    if unilateral && c > 0.0 { return; }

    let w0 = calc_effective_inv_mass(body_a, normal, Some(anchor_world0));
    let w1 = calc_effective_inv_mass(body_b, normal, Some(anchor_world1));
    let w = w0 + w1;

    if w < f32::EPSILON { return; }

    let alpha = compliance / (sdt * sdt);
    let lambda = -c / (w + alpha);
    let correction = normal * (-lambda);

    body_a.apply_correction(correction, anchor_world0);
    body_b.apply_correction(-correction, anchor_world1);
}

#[inline(always)]
fn solve_hinge(
    body_a: &mut RigidBodyCapture,
    body_b: &mut RigidBodyCapture,
    anchor_a: Entity,
    anchor_b: Entity,
    swing_min: f32,
    swing_max: f32,
    target_angle: Option<TargetAngle>,
    sdt: f32,
) {
    let hard_compliance = 0.0;

    solve_distance(body_a, body_b, anchor_a, anchor_b, 0.0, hard_compliance, false, sdt);

    let (rot0, rot1) = get_anchor_rotations(body_a, body_b, anchor_a, anchor_b);
    align_axes(body_a, body_b, rot0 * Vec3::X, rot1 * Vec3::X, hard_compliance, sdt);

    if let Some(target) = target_angle {
        let (rot0, rot1) = get_anchor_rotations(body_a, body_b, anchor_a, anchor_b);
        let hinge_axis = rot0 * Vec3::X;
        let perp0 = rot0 * Vec3::Y;
        let perp1 = rot1 * Vec3::Y;

        limit_angle(body_a, body_b, hinge_axis, perp0, perp1,
            target.angle, target.angle, target.compliance, sdt);
    }

    if swing_min > -f32::MAX || swing_max < f32::MAX {
        let (rot0, rot1) = get_anchor_rotations(body_a, body_b, anchor_a, anchor_b);
        let hinge_axis = rot0 * Vec3::X;
        let perp0 = rot0 * Vec3::Y;
        let perp1 = rot1 * Vec3::Y;

        limit_angle(body_a, body_b, hinge_axis, perp0, perp1,
            swing_min, swing_max, hard_compliance, sdt);
    }
}

#[inline(always)]
pub fn calc_local_pos(world_pos_a: &Vec3, world_rot_a: &Quat, world_pos_b: &Vec3) -> Vec3 {
    world_rot_a.inverse() * (*world_pos_b - *world_pos_a)
}

#[inline(always)]
pub fn calc_local_rot(world_rot_a: &Quat, world_rot_b: &Quat) -> Quat {
    world_rot_a.inverse() * *world_rot_b
}

#[inline(always)]
pub fn calc_world_pos(world_pos: &Vec3, world_rot: &Quat, local_pos: &Vec3) -> Vec3 {
    world_pos + world_rot * local_pos
}

#[inline(always)]
pub fn calc_world_rot(world_rot: &Quat, local_rot: &Quat) -> Quat {
    world_rot * local_rot
}

#[inline(always)]
pub fn capture_rbs(
    bodies: Query<(
        Entity,
        &Velocity,
        &AngularVelocity,
        &Transform,
        &InverseMass,
        &InverseInertia,
        &SolverConfig,
        Option<&ApplyGravity>,
        Option<&Children>,
    ), (With<RigidBodyMarker>, Without<AnchorMarker>)>,
    anchors: Query<&Transform, (Without<RigidBodyMarker>, With<AnchorMarker>)>,
    mut pool: ResMut<ConstraintSolverPool>,
) {
    let mut seen_entities = AHashSet::with_capacity(bodies.iter().len());

    for (entity, vel, omega, trans, inv_mass, inv_inertia, config, gravity, maybe_anchors) in bodies.iter() {
        seen_entities.insert(entity);

        let mut flags = BodyFlags(0);
        if gravity.is_some() {
            flags.0 |= BodyFlag::Gravity as u8;
        }

        let anchor_points = if let Some(anchors_list) = maybe_anchors {
            let mut anchor_points = RigidBodyAnchors(InstanceManager::with_capacity(anchors_list.len()));
            for anchor in anchors_list.iter() {
                if let Ok(offset) = anchors.get(anchor) {
                    anchor_points.insert(anchor, *offset);
                }
            }
            anchor_points
        } else {
            RigidBodyAnchors::default()
        };

        let capture = RigidBodyCapture {
            anchors: anchor_points,
            prev_rot: PrevRotation(trans.rotation),
            curr_rot: CurrentRotation(trans.rotation),
            prev_pos: PrevPosition(trans.translation),
            curr_pos: CurrentPosition(trans.translation),
            velocity: *vel,
            omega: *omega,
            inv_mass: *inv_mass,
            inv_inertia: *inv_inertia,
            flags,
        };

        if let Err(e) = pool.insert_rb(config, entity, Some(capture)) {
            panic!("{}", e);
        }
    }

    let to_remove: Vec<Entity> = pool.rb_members
        .keys()
        .filter(|entity| !seen_entities.contains(entity))
        .collect();

    for entity in to_remove {
        if let Some(key) = pool.rb_members.remove(entity) {
            if let Some(solver) = pool.solvers.get_mut(key) {
                solver.rigid_bodies.remove(&entity);
            }
        }
    }
}

#[inline(always)]
pub fn solve_rbs(
    mut pool: ResMut<ConstraintSolverPool>,
    gravity: Res<Gravity>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    pool.solve_all(*gravity, dt);
}

#[inline(always)]
pub fn writeback_rb(
    pool: Res<ConstraintSolverPool>,
    mut bodies: Query<(&mut Velocity, &mut AngularVelocity, &mut Transform)>,
) {
    for (_key, solver) in pool.solvers.iter() {
        for (entity, capture) in solver.rigid_bodies.iter() {
            if let Ok((mut ecs_vel, mut ecs_omega, mut ecs_trans)) = bodies.get_mut(*entity) {
                ecs_trans.translation = *capture.curr_pos;
                ecs_trans.rotation = *capture.curr_rot;
                *ecs_vel = capture.velocity;
                *ecs_omega = capture.omega;
            }
        }
    }
}

#[derive(Component, Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct ConstraintMarker;

#[derive(Component, Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Active;

#[derive(Component, Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Inactive;

#[derive(Component, Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Temporary;

#[derive(Clone, Copy, Debug)]
pub struct TargetAngle {
    pub angle: f32,
    pub compliance: f32,
}

#[derive(Component, Debug, Clone)]
pub struct ConstraintConfig {
    pub anchor_a: Entity,
    pub body_a: Entity,
    pub anchor_b: Entity,
    pub body_b: Entity,
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub params: ConstraintParams,
}

impl ConstraintConfig {
    #[inline(always)]
    pub fn solve(&self, body_a: &mut RigidBodyCapture, body_b: &mut RigidBodyCapture, sdt: f32) {
        match self.params {
            ConstraintParams::Grab { compliance } =>
                solve_distance(body_a, body_b, self.anchor_a, self.anchor_b, 0.0, compliance, false, sdt),
            ConstraintParams::Distance { rest_length, compliance, unilateral } =>
                solve_distance(body_a, body_b, self.anchor_a, self.anchor_b, rest_length, compliance, unilateral, sdt),
            ConstraintParams::Hinge { swing_min, swing_max, target_angle } =>
                solve_hinge(body_a, body_b, self.anchor_a, self.anchor_b, swing_min, swing_max, target_angle, sdt),
        }
    }

    #[inline(always)]
    pub fn apply_damping(&self, body_a: &mut RigidBodyCapture, body_b: &mut RigidBodyCapture, sdt: f32) {
        if self.linear_damping > 0.0 {
            apply_linear_damping(body_a, body_b, self.anchor_a, self.anchor_b, self.linear_damping, sdt);
        }
        if self.angular_damping > 0.0 {
            apply_angular_damping(body_a, body_b, self.anchor_a, self.anchor_b, self.angular_damping, sdt);
        }
    }
}

#[derive(Debug, Component)]
pub enum ConstraintType {
    Grab,
    Distance,
    Hinge,
}

#[derive(Debug, Clone)]
pub enum ConstraintParams {
    Grab {
        compliance: f32,
    },
    Distance {
        rest_length: f32,
        compliance: f32,
        unilateral: bool,
    },
    Hinge {
        swing_min: f32,
        swing_max: f32,
        target_angle: Option<TargetAngle>,
    },
}

#[derive(Bundle)]
pub struct Constraint {
    pub config: ConstraintConfig,
    pub solver: SolverConfig,
    _marker: ConstraintMarker,
}

impl Constraint {
    #[inline(always)]
    pub fn spawn_constraint(
        commands: &mut Commands,
        constraint_config: ConstraintConfig,
        solver_config: SolverConfig,
    ) -> Entity {
        commands.spawn((
            Self {
                config: constraint_config,
                solver: solver_config,
                _marker: ConstraintMarker,
            },
            Active,
        )).id()
    }

    #[inline(always)]
    pub fn spawn_temporary(
        commands: &mut Commands,
        constraint_config: ConstraintConfig,
    ) -> Entity {
        commands.spawn((
            Self {
                config: constraint_config,
                solver: SolverConfig::NONE,
                _marker: ConstraintMarker,
            },
            Temporary,
        )).id()
    }
}

#[inline(always)]
fn apply_linear_damping(
    body_a: &mut RigidBodyCapture,
    body_b: &mut RigidBodyCapture,
    anchor_a: Entity,
    anchor_b: Entity,
    damping_coeff: f32,
    sdt: f32,
) {
    if damping_coeff == 0.0 { return; }

    let (pos_a, pos_b) = get_anchor_positions(body_a, body_b, anchor_a, anchor_b);

    let vel_a = body_a.get_velocity_at(pos_a);
    let vel_b = body_b.get_velocity_at(pos_b);
    let mut d_vel = vel_a - vel_b;

    let n = (pos_b - pos_a).normalize_or_zero();
    if n.length_squared() < f32::EPSILON { return; }

    d_vel = n * d_vel.dot(n);
    d_vel *= -(damping_coeff * sdt).min(1.0);

    apply_velocity_correction(body_a, body_b, d_vel, pos_a, pos_b);
}

#[inline(always)]
fn apply_angular_damping(
    body_a: &mut RigidBodyCapture,
    body_b: &mut RigidBodyCapture,
    anchor_a: Entity,
    anchor_b: Entity,
    damping_coeff: f32,
    sdt: f32,
) {
    if damping_coeff == 0.0 { return; }

    let mut d_omega = body_a.omega.0 - body_b.omega.0;

    let (rot_a, _) = get_anchor_rotations(body_a, body_b, anchor_a, anchor_b);
    let hinge_axis = rot_a * Vec3::X;
    d_omega = hinge_axis * d_omega.dot(hinge_axis);

    d_omega *= -(damping_coeff * sdt).min(1.0);

    let w_a = calc_effective_inv_mass(body_a, d_omega, None);
    let w_b = calc_effective_inv_mass(body_b, -d_omega, None);
    let w = w_a + w_b;

    if w < f32::EPSILON { return; }

    let lambda = 1.0 / w;
    body_a.apply_angular_velocity_correction(d_omega * lambda);
    body_b.apply_angular_velocity_correction(-d_omega * lambda);
}

#[inline(always)]
fn apply_velocity_correction(
    body_a: &mut RigidBodyCapture,
    body_b: &mut RigidBodyCapture,
    correction: Vec3,
    pos_a: Vec3,
    pos_b: Vec3,
) {
    let normal = correction.normalize_or_zero();
    if normal.length_squared() < f32::EPSILON { return; }

    let w_a = calc_effective_inv_mass(body_a, normal, Some(pos_a));
    let w_b = calc_effective_inv_mass(body_b, normal, Some(pos_b));
    let w = w_a + w_b;

    if w < f32::EPSILON { return; }

    let lambda = correction.length() / w;
    let impulse = normal * lambda;

    body_a.apply_velocity_impulse(impulse, pos_a);
    body_b.apply_velocity_impulse(-impulse, pos_b);
}

fn on_constraint_remove(
    trigger: On<Remove, ConstraintConfig>,
    constraints: Query<&SolverConfig>,
    mut pool: ResMut<ConstraintSolverPool>,
) {
    let entity = trigger.entity;
    if let Ok(solver_config) = constraints.get(entity) {
        if let Some(solver) = pool.solvers.get_mut(solver_config.key()) {
            solver.remove_constraint(entity);
        }
    }
}