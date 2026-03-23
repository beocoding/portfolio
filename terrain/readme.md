# Terrain Generator — README

A tile-based procedural terrain system for [Bevy](https://bevyengine.org/), built around a multi-draw indirect render pipeline, a custom slot-map instance manager, and a double-buffered GPU upload system. CPU draw call overhead is **O(1)** regardless of tile count.

***

## Table of Contents

- [Overview](#overview)
- [Architecture](#architecture)
- [Noise Generation](#noise-generation)
- [Mesh Generation](#mesh-generation)
- [GPU Buffer Management](#gpu-buffer-management)
- [Render Pipeline](#render-pipeline)
- [Configuration](#configuration)
- [Project Structure](#project-structure)

***

## Overview

Most terrain systems draw one mesh per tile. This system draws **all tiles in one GPU call** using `multi_draw_indirect`. Data flows in one direction:

```
Main World                  Render World                    GPU
──────────                  ────────────                    ───
NoiseSettings          →    ExtractResource           →    Push Constants
ChunkSettings          →    ExtractResource           →    Push Constants
TerrainMeshlet         →    TerrainMeshData           →    Vertex Buffer (64 MB pool)
TerrainTileId          →    ExtractComponent          →    DrawIndirectArgs buffer
DirtyTile (marker)     →    prepare → flush → swap    →    multi_draw_indirect
```

Nothing flows back. The render world is a strict consumer.

***

## Architecture

The system is divided into three layers:

### 1. Generation
Fractal noise → normalized heightmap → triangle strip meshlets per tile.

### 2. Management
A generic `InstanceManager<K, T>` slot-map tracks live, dirty, and freed tiles with O(1) insert, lookup, and removal. Freed slots are recycled via an intrusive free list rather than reallocating.

### 3. Rendering
One `Transparent3d` phase item per camera view drives the full render command chain. A `TerrainMultiDrawBuffer` holds `DrawIndirectArgs` for every tile; the GPU dispatches them all in a single `multi_draw_indirect` call.

***

## Noise Generation

Terrain heights are generated with **two-pass fractional Brownian motion (fBm)**.

### Pass 1 — Raw Noise
Each sample accumulates `N` Perlin octaves:

$$h = \sum_{i=0}^{N} \text{perlin}(x \cdot \lambda^i + o_x,\ z \cdot \lambda^i + o_z) \cdot p^i$$

where $\lambda$ is lacunarity (frequency growth per octave) and $p$ is persistence (amplitude decay per octave). Per-octave offsets are seeded deterministically from `NoiseSettings::seed` via `StdRng`, breaking Perlin's grid symmetry. The actual global min and max are tracked across all samples.

### Pass 2 — Normalize & Shape
Raw values are normalized using the **actual** min/max from Pass 1 (not a theoretical bound), then two shaping effects are applied:

- **Radial falloff** — each point's distance from map center $r = \sqrt{n_x^2 + n_z^2}$ is computed in normalized `[-1, 1]` space. A power curve $r^{\text{falloff\_strength}}$ blends the height toward `sea_level` at the edges, producing a natural island silhouette.
- **Height curve** — the final `[0, 1]` value is passed through $t^3$, compressing lowland variation and stretching mountain variation. Configure via `HeightCurve`:

| Variant | Behavior |
|---|---|
| `Identity` | No shaping, linear `[0, 1]` |
| `Cubic` | $t^3$ — flat lowlands, dramatic peaks |
| `MidBoost` | Keyframe curve — flat plains, rising hills, sharp mountains |

### The Skirt
Each tile stores `(chunk_size + 2)²` samples instead of `chunk_size²`. The extra border row/column lets normal calculation sample neighbor heights at tile edges without cross-tile communication, eliminating visible seam lines.

***

## Mesh Generation

Each tile is a **triangle strip** — `2N + 2` vertices per row vs. `6N` for a triangle list, with the GPU reusing every vertex in two triangles automatically.

For each row `k`, the strip emits:
- Two **degenerate triangles** (first vertex duplicated) to join strips without an index buffer
- Alternating `(top, bottom)` vertex pairs across the row
- A closing vertex to end the strip

### Vertex Format

Each `TerrainVertex` is **8 bytes**:

```rust
#[repr(C)]
pub struct TerrainVertex {
    pub altitude: f32,  // normalized height in [0, height_scale]
    pub normal:   u32,  // packed pitch (u16) | yaw (u16)
}
```

Normals are computed from all 8 surrounding triangle cross-products, summed and normalized, then encoded:

$$\text{pitch} = \arcsin(n_y), \quad \text{yaw} = \text{atan2}(n_x, n_z)$$

Both angles are quantized to `u16` and packed into one `u32`. The WGSL shader unpacks them at draw time. Storing angles instead of a full `Vec3` saves 4 bytes per vertex.

***

## GPU Buffer Management

### Global Vertex Buffer
A single **64 MB** `VERTEX | COPY_DST` wgpu buffer is allocated once at startup. A `RangeAllocator` (first-fit free-list over byte offsets) carves it up as tiles come and go. When a tile is removed, its byte range is returned and `merge_adjacent()` coalesces contiguous free blocks to prevent fragmentation. No reallocation ever occurs at runtime.

### Double-Buffered Uploads
`StagedData<T>` manages each tile's GPU slice with two ranges:

```
current_range  →  what the GPU is currently reading
next_range     →  newly written data waiting to go live
```

On `flush()`:
1. Allocate `next_range` from the global pool
2. Write via `queue.write_buffer()`
3. Call `try_swap()` — promotes `next_range → current_range`, pushes displaced old range to a `VecDeque<PendingCleanUpRange>` with release frame `current_frame + 3`

The **3-frame delay** ensures the GPU has finished reading a range before its memory is reclaimed. A `FrameCounter` resource (incremented in both `Update` and `PostCleanup`) provides the clock.

### Indirect Draw Buffer
`TerrainMultiDrawBuffer` mirrors a `Vec<DrawIndirectArgs>` to a GPU `INDIRECT | COPY_DST` buffer. Each entry sits at `tile_id * size_of::<DrawIndirectArgs>()`:

```rust
DrawIndirectArgs {
    vertex_count,    // vertices in this tile
    instance_count,  // 1 = active, 0 = empty/culled
    first_vertex,    // byte_offset / vertex_size
    first_instance,  // tile_id — readable in the vertex shader
}
```

Tiles can be updated independently without touching adjacent entries.

***

## Render Pipeline

The pipeline uses Bevy's `Variants<RenderPipeline, TerrainMeshSpecializer>` to cache variants keyed by `TerrainMeshPipelineKey` (MSAA samples + HDR + primitive topology). Specialization configures:

- **Depth/stencil** — `Depth32Float`, `CompareFunction::Greater` (reverse-Z for far-plane precision)
- **Fragment target** — HDR (`Rgba16Float`) or sRGB (`bevy_default()`) based on view flags
- **MSAA** — sample count from the view's `Msaa` component

### Render Command Chain

One `Transparent3d` phase item per view executes four commands in sequence:

| Step | Command | What it does |
|---|---|---|
| 1 | `SetItemPipeline` | Binds the specialized `RenderPipeline` |
| 2 | `SetMeshViewBindGroup<0>` | Uploads camera matrices and view uniforms |
| 3 | `SetTerrainPushConstants` | Pushes 20 bytes of config to the vertex shader |
| 4 | `DrawTerrainMeshMultiIndirect` | Binds vertex buffer; calls `multi_draw_indirect` |

### Push Constants

Config is passed to the shader as a 20-byte `repr(C)` struct — no bind group needed:

```rust
struct TerrainPushConstants {
    view_distance:     u32,
    chunk_size:        u32,
    map_height:        u32,
    map_width:         u32,
    height_multiplier: f32,
}
```

***

## Configuration

### `ChunkSettings`

| Field | Description |
|---|---|
| `chunk_size` | Vertices per tile edge |
| `map_width` | Total map width in world units |
| `map_height` | Total map height in world units |
| `view_distance` | Tile render radius |

### `NoiseSettings`

| Field | Description |
|---|---|
| `noise_scale` | World-space scale of the noise field |
| `octaves` | Number of fBm layers |
| `persistence` | Amplitude decay per octave |
| `lacunarity` | Frequency growth per octave |
| `height_scale` | Final height multiplier |
| `offset` | World-space pan offset |
| `seed` | Optional fixed seed; random if `None` |
| `noise_type` | Currently `OctaveFractal` |

***

## Project Structure

```
src/
├── noise.rs          # fBm generation, TerrainVertex, mesh building, normal encoding
├── buffer.rs         # RangeAllocator, StagedData<T>, TerrainMeshGpuBuffer
├── pipeline.rs       # TerrainMeshPipeline, specializer, render commands, plugin
├── config.rs         # ChunkSettings, NoiseSettings, HeightCurve
└── instance_manager/ # Generic slot-map (InstanceManager<K, T>)
    ├── mod.rs
    └── drain.rs
```

***

## Design Tradeoffs

| Decision | Benefit | Cost |
|---|---|---|
| `multi_draw_indirect` | O(1) CPU draw overhead | Requires `INDIRECT_FIRST_INSTANCE` wgpu feature |
| Single 64 MB vertex buffer | Zero runtime reallocation | Fixed memory ceiling |
| 3-frame deferred cleanup | Safe GPU pipelining | Up to 3 frames of "dead" memory held |
| Packed `u32` normals | 8-byte vertex, less bandwidth | Decode cost in vertex shader |
| Triangle strips | ~66% fewer vertices vs. triangle list | Degenerate triangles needed between strips |
| Two-pass fBm | Accurate normalization | Full map must fit in memory twice |