use bevy::prelude::*;
use super::*;


#[inline(always)]
pub fn validate_new_constraints(
    mut commands: Commands,
    mut new_constraints: Query<(
        Entity,
        &mut ConstraintConfig,
        &SolverConfig,
    ), (
        Without<RigidBodyMarker>,
        Added<Active>  // ← Changed to Added<Active> to catch new constraints
    )>,
    attachment_check: Query<(), With<AnchorMarker>>,
    mut pool: ResMut<ConstraintSolverPool>,
) {
    for (entity, mut constraint, solver) in new_constraints.iter_mut() {
        let body0 = constraint.body_a;
        let body1 = constraint.body_b;

        // === Verify or spawn anchors ===
        if attachment_check.get(constraint.anchor_a).is_err() {
            constraint.anchor_a = AnchorPoint::spawn(&mut commands, Transform::default(), body0);
        }
        if attachment_check.get(constraint.anchor_b).is_err() {
            constraint.anchor_b = AnchorPoint::spawn(&mut commands, Transform::default(), body1);
        }
        
        // === Register the constraint and bodies in solver ===
        pool.insert_constraint(solver, entity, &constraint);
        pool.insert_rb(solver, body0, None).ok();
        pool.insert_rb(solver, body1, None).ok();
    }
}