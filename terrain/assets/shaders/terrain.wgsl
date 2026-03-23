#import bevy_pbr::mesh_view_bindings::view

struct VertexInput {
    @location(0) altitude: f32,
    @location(1) normal: u32,
    @builtin(vertex_index) vertex_idx: u32,
    @builtin(instance_index) tile_id: u32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
    @location(1) height: f32,
    @location(2) barycentric: vec3<f32>,
}

struct PushConstants {
    view_distance: u32,
    chunk_size: u32,
    map_height: u32,
    map_width: u32,
    height_multiplier: f32,
}

var<push_constant> pc: PushConstants;

const VERTEX_SPACING: f32 = 1.0;
const PI: f32 = 3.14159265359;

@vertex
fn vertex(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;

    let MAP_CHUNK_WIDTH = pc.map_width / pc.chunk_size;
    let MAP_CHUNK_HEIGHT = pc.map_height / pc.chunk_size;
    let VERTICES_PER_ROW = (pc.chunk_size << 1) + 4;
    let VERTICES_PER_TILE = VERTICES_PER_ROW * pc.chunk_size;

    let tile_x = in.tile_id % MAP_CHUNK_WIDTH;
    let tile_z = in.tile_id / MAP_CHUNK_HEIGHT;

    let map_world_width = f32(MAP_CHUNK_WIDTH) * f32(pc.chunk_size) * VERTEX_SPACING;
    let map_world_depth = f32(MAP_CHUNK_HEIGHT) * f32(pc.chunk_size) * VERTEX_SPACING;

    let tile_world_x = f32(tile_x * pc.chunk_size) * VERTEX_SPACING - (map_world_width * 0.5);
    let tile_world_z = f32(tile_z * pc.chunk_size) * VERTEX_SPACING - (map_world_depth * 0.5);
    
    let local_idx = in.vertex_idx % VERTICES_PER_TILE;
    let row = local_idx / VERTICES_PER_ROW;
    let col_in_strip = local_idx % VERTICES_PER_ROW;

    var local_x: u32 = 0u;
    var local_z: u32 = 0u;

    if col_in_strip < 2u {
        local_x = 0u;
        local_z = row;
    } else if col_in_strip == (VERTICES_PER_ROW - 1u) {
        local_x = pc.chunk_size;
        local_z = row + 1u;
    } else {
        let adjusted = col_in_strip - 2u;
        if adjusted == 0u {
            local_x = 0u;
            local_z = row + 1u;
        } else if (adjusted % 2u) == 1u {
            local_x = (adjusted + 1u) / 2u;
            local_z = row;
        } else {
            local_x = adjusted / 2u;
            local_z = row + 1u;
        }
    }

    local_x = min(local_x, pc.chunk_size);
    local_z = min(local_z, pc.chunk_size);

    let world_x = tile_world_x + f32(local_x) * VERTEX_SPACING;
    let world_z = tile_world_z + f32(local_z) * VERTEX_SPACING;
    let world_y = in.altitude;
    let world_pos = vec3<f32>(world_x, world_y, world_z);

    // Decode normal
    let pitch_u16 = f32(in.normal & 0xFFFFu);
    let yaw_u16 = f32(in.normal >> 16u);
    let pitch = (pitch_u16 / 65535.0) * (PI * 0.5);
    let yaw = (yaw_u16 / 65535.0) * (2.0 * PI) - PI;

    out.world_normal = vec3<f32>(
        cos(pitch) * sin(yaw),
        sin(pitch),
        cos(pitch) * cos(yaw)
    );

    // Generate barycentric coordinates for wireframe
    let tri_vertex_idx = in.vertex_idx % 3u;
    if tri_vertex_idx == 0u {
        out.barycentric = vec3<f32>(1.0, 0.0, 0.0);
    } else if tri_vertex_idx == 1u {
        out.barycentric = vec3<f32>(0.0, 1.0, 0.0);
    } else {
        out.barycentric = vec3<f32>(0.0, 0.0, 1.0);
    }

    out.clip_position = view.clip_from_world * vec4<f32>(world_pos, 1.0);
    out.height = in.altitude;
    
    return out;
}
@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // Use vertex normal for flat shading (approximated)
    let face_normal = normalize(in.world_normal);

    let light_dir = normalize(vec3<f32>(0.5, 1.0, 0.3));
    let ambient = 0.3;
    let diffuse = max(dot(face_normal, light_dir), 0.0);
    let lighting = ambient + (1.0 - ambient) * diffuse;

    // Height-based terrain coloring
    let h = clamp(in.height / pc.height_multiplier, 0.0, 1.0);
    var base_color: vec3<f32>;
    if h < 0.3 {
        base_color = mix(vec3<f32>(0.1, 0.2, 0.4), vec3<f32>(0.7, 0.7, 0.5), h / 0.3);  // Water → Sand
    } else if h < 0.6 {
        base_color = mix(vec3<f32>(0.2, 0.5, 0.1), vec3<f32>(0.3, 0.6, 0.2), (h - 0.3) / 0.3);  // Grass
    } else {
        base_color = mix(vec3<f32>(0.5, 0.5, 0.5), vec3<f32>(0.9, 0.9, 0.9), (h - 0.6) / 0.4);  // Rock → Snow
    }

    let shaded_color = base_color * lighting;

    return vec4<f32>(shaded_color, 1.0);
}
