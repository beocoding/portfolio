# XPBD Physics Engine


A real-time rigid body physics engine built from scratch in Rust using [Bevy](https://bevyengine.org/), implementing the **Extended Position-Based Dynamics (XPBD)** algorithm. Runs natively and compiles to **WebAssembly** via Trunk for browser-based interactive demos.


**[Live Demo](demo/)** — drag and interact with the simulated bodies in real time.


---


## Overview


XPBD is a modern constraint-based simulation algorithm that separates position correction from velocity updates, enabling stable, physically accurate behavior at practical timesteps. This implementation is a ground-up Rust realization of the method described in:


> Müller et al., *"Detailed Rigid Body Simulation with Extended Position Based Dynamics"*, SIGGRAPH 2020.


The engine is designed around Bevy's **Entity Component System (ECS)**, using a custom fixed-timestep solver pipeline fully decoupled from the render loop.


---


## Features


### Constraint Types
- **Distance constraint** — rigid or compliant, unilateral (rope) or bilateral (rod)
- **Hinge constraint** — full 3D rigid joint with configurable swing limits (min/max angle)
- **Servo/target angle** — soft spring driving a hinge toward a target orientation
- **Grab constraint** — mouse-driven spring for interactive dragging


### Solver Architecture
- Sub-stepped integration with configurable steps and iterations per frame
- `ConstraintSolverPool` — multiple independent solver instances keyed by `SolverConfig`, enabling heterogeneous simulation groups
- Full **angular correction** via XPBD generalized inverse mass with inertia tensor in local space
- Velocity-level damping (linear and angular) applied post-constraint as a separate pass
- Anchor point system — constraints attach at arbitrary local offsets on bodies, with full world-space transform calculation


### Rigid Body System
- Spawn helpers for mass-based and density-based bodies across four collider shapes: cuboid, sphere, capsule, cylinder
- Automatic inertia tensor calculation per shape
- `ApplyGravity` marker component — gravity is opt-in per body
- Static bodies (infinite mass) supported natively — zero-branch in integration


### ECS Integration
- `capture_rbs` → `solve_rbs` → `writeback_rb` pipeline runs on Bevy's `FixedUpdate` schedule
- Constraint validation observer (`on_constraint_remove`) keeps solver state consistent with ECS despawns
- Anchor points are child entities using `ChildOf`, keeping the hierarchy legible in the ECS world
- `MouseDragState` resource + `handle_mouse_drag` system for real-time mouse picking and dragging via raycasting


### WebAssembly
- Compiles to `wasm32-unknown-unknown` via [Trunk](https://trunkrs.dev/)
- WebGL2 backend (`bevy/webgl2` feature)
- All solver internals use `f32` — no `f64`/`DVec3` overhead on WASM targets


---


## Demo Scenes


### 1. Door Hinge
A static wall with a dynamic door attached at its left edge. The hinge is constrained to ±90° and falls under gravity with angular damping, naturally swinging to rest.


### 2. Servo Arm
A static anchor with a hanging arm driven toward a 45° target angle via a compliant hinge constraint — demonstrating position-based servo control.


### 3. Hinged Chain
Four rigid segments connected in series from a fixed anchor, each limited to ±60° swing. Demonstrates constraint propagation through a multi-body chain and pendulum dynamics.


---


## Architecture


```
FixedUpdate schedule (64 Hz):
  validate_new_constraints   — register new ECS constraints into solver pool
  capture_rbs                — snapshot ECS transforms/velocities into solver
  solve_rbs                  — run XPBD sub-steps (integrate → solve → damp → update vel)
  writeback_rb               — write solver results back to ECS transforms


Update schedule:
  handle_mouse_drag          — raycast picking, kinematic anchor drag
```


The solver operates entirely on **captured data** (`RigidBodyCapture`) — a plain Rust struct containing positions, rotations, velocities, inertia, and anchor offsets. This keeps the hot loop free of ECS overhead and makes the physics logic straightforward to reason about, test, and extend.


---


## Building


### Native
```bash
cargo run
```


### WebAssembly
```bash
rustup target add wasm32-unknown-unknown
cargo install trunk
trunk serve
```


Open `http://localhost:8080` — click and drag any dynamic body.


### Release (WASM)
```bash
trunk build --release
# Output in dist/ — deploy to any static host
```


---


## Project Structure


```
src/
├── lib.rs              # Core types, solver, ECS systems, constraints
├── main.rs             # App entry point and demo scenes
├── observers.rs        # Constraint registration observer
├── debug.rs            # Optional debug plugin (gizmos, gravity logging)
└── instance_manager/   # Generic keyed instance container
```


---


## Technical Notes


**Why XPBD over impulse-based methods?**
XPBD is unconditionally stable at large timesteps, compliance is a first-class parameter (not a tuning hack), and the position-level correction makes constraint behavior predictable and art-directable — valuable properties for real-time simulation where behavior consistency matters as much as physical accuracy.


**Why ECS?**
Bevy's ECS provides natural parallelism boundaries, clean separation between simulation state and render state, and a reactive observer system that keeps solver bookkeeping (constraint registration, removal) tied to entity lifecycle events rather than manual management.


**Solver isolation**
Each unique `SolverConfig` (steps × iterations) gets its own `ConstraintSolver` instance. This allows different parts of a scene to run at different fidelity levels without interfering — a useful property for future work on level-of-detail physics.


---
