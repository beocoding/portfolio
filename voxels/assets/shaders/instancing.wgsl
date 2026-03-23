
// === Auto-generated constants ===
const CHUNK_SIZE: f32 = 32.0;
const CHUNK_AXIS_BITS: u32 = 10u;
const CHUNK_Y_MASK: u32 = (1u << CHUNK_AXIS_BITS) - 1u;
const CHUNK_X_MASK: u32 = CHUNK_Y_MASK << CHUNK_AXIS_BITS;
const CHUNK_Z_MASK: u32 = CHUNK_Y_MASK << (CHUNK_AXIS_BITS * 2u);


// voxel_base.wgsl - Fixed UV mapping for partial chunks

#import bevy_pbr::mesh_view_bindings::view;

// Face directions matching FaceType enum
const YP: u32 = 0u;  // TOP
const YN: u32 = 1u;  // BOTTOM
const XP: u32 = 2u;  // RIGHT
const XN: u32 = 3u;  // LEFT
const ZP: u32 = 4u;  // FRONT
const ZN: u32 = 5u;  // BACK

// Face normals
const FACE_NORMALS: array<vec3<f32>, 6> = array<vec3<f32>, 6>(
    vec3<f32>(0.0, 1.0, 0.0),     // YP (TOP)
    vec3<f32>(0.0, -1.0, 0.0),    // YN (BOTTOM)
    vec3<f32>(1.0, 0.0, 0.0),     // XP (RIGHT)
    vec3<f32>(-1.0, 0.0, 0.0),    // XN (LEFT)
    vec3<f32>(0.0, 0.0, 1.0),     // ZP (FRONT)
    vec3<f32>(0.0, 0.0, -1.0),    // ZN (BACK)
);

const FACE_BRIGHTNESS: array<f32, 6> = array<f32, 6>(
    1.0,   // YP (TOP) - brightest
    0.55,  // YN (BOTTOM) - darkest  
    0.85,  // XP (RIGHT) - side
    0.85,  // XN (LEFT) - side
    0.70,  // ZP (FRONT) - front/back
    0.70   // ZN (BACK) - front/back
);

// Culling configuration - GPU frustum culling handles visibility for orthographic
const ENABLE_BACKFACE_CULLING: bool = false;
const ENABLE_DISTANCE_CULLING: bool = false;

struct Vertex {
    @location(0) position: vec3<f32>,     // From mesh vertices
    @location(1) instance: u32,           // VoxelInstance data
    @location(2) chunk_index: u32,        // First field of InstanceMeta
    @location(3) face: u32,               // Second field of InstanceMeta
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) uv: vec2<f32>,
};

// Extract chunk position from packed data
fn get_chunk_position(data: u32) -> vec3<f32> {
    let y = data & CHUNK_Y_MASK;
    let x = (data & CHUNK_X_MASK) >> CHUNK_AXIS_BITS;
    let z = (data & CHUNK_Z_MASK) >> (CHUNK_AXIS_BITS * 2u);

    return vec3<f32>(f32(x), f32(y), f32(z)) * CHUNK_SIZE;
}

// Extract local voxel position within chunk
fn get_local_position(data: u32) -> vec3<f32> {
    let y = data & 0x1Fu;         // y is LSB (first 5 bits)
    let x = (data >> 5u) & 0x1Fu;  // x is middle (next 5 bits)
    let z = (data >> 10u) & 0x1Fu; // z is MSB (last 5 bits)
    return vec3<f32>(f32(x), f32(y), f32(z));
}

// Extract greedy meshing extension A (primary axis)
fn get_len_a(data: u32) -> f32 {
    let a = (data >> 15u) & 0x1Fu;
    return f32(a);
}

// Extract greedy meshing extension B (secondary axis)
fn get_len_b(data: u32) -> f32 {
    let b = (data >> 20u) & 0x1Fu;
    return f32(b);
}

// Extract texture ID from packed data
fn get_texture_id(data: u32) -> u32 {
    let id = (data >> 25u) & 0x7Fu;
    return id;
}

// Get base color from texture ID
fn get_color(texture_id: u32) -> vec4<f32> {
    if texture_id == 0u {
        return vec4<f32>(0.6706, 0.6078, 0.5882, 1.0); // Stone
    }
    if texture_id == 1u {
        return vec4<f32>(0.6314, 0.4039, 0.2902, 1.0); // Dirt
    }
    if texture_id == 2u {
        return vec4<f32>(0.4235, 0.8314, 1.0, 1.0);    // Water
    }
    if texture_id == 3u {
        return vec4<f32>(0.3451, 0.7020, 0.2941, 1.0); // Grass
    }
    if texture_id == 4u {
        return vec4<f32>(0.5020, 0.2510, 0.0000, 1.0); // Wood
    }
    if texture_id == 5u {
        return vec4<f32>(0.7529, 0.7529, 0.7529, 1.0); // Metal
    }
    return vec4<f32>(1.0, 0.0, 1.0, 1.0); // Debug magenta
}

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;

    let face = vertex.face;
    let instance = vertex.instance;
    
    // Extract data from packed instance
    let local_pos = get_local_position(instance);
    let len_a = get_len_a(instance);
    let len_b = get_len_b(instance);
    let chunk_pos = get_chunk_position(vertex.chunk_index);
    let brightness = FACE_BRIGHTNESS[face];
    let texture = get_texture_id(instance);
    let base_color = get_color(texture);
    let face_normal = FACE_NORMALS[face];

    // Base quad in XZ plane: (0,0,0), (1,0,0), (0,0,1), (1,0,1)
    var net_pos = vertex.position;
    
    // Apply greedy meshing extensions FIRST (before transformations)
    if net_pos.x == 1.0 { net_pos.x += len_a; }
    if net_pos.z == 1.0 { net_pos.z += len_b; }
    
    // Apply transformation logic based on face orientation:
    
    // X orientation: swap X and Y coordinates
    if face == XP || face == XN {
        let temp = net_pos.x;
        net_pos.x = net_pos.y;
        net_pos.y = temp;
    }
    
    // Z orientation: rotate XYZ -> ZXY  
    if face == ZP || face == ZN {
        let temp_x = net_pos.x;
        let temp_y = net_pos.y;
        let temp_z = net_pos.z;
        net_pos.x = temp_z;
        net_pos.y = temp_x;
        net_pos.z = temp_y;
    }
    
    // Set the face position offset
    if face == YP {
        net_pos.y = 1.0;
    } else if face == XP {
        net_pos.x = 1.0;
    } else if face == ZP {
        net_pos.z = 1.0;
    }
    
    // Transform to world space
    net_pos += local_pos;  // Add voxel offset within chunk
    net_pos += chunk_pos;  // Add chunk offset in world

    // Set outputs
    out.world_position = net_pos;  
    out.world_normal = face_normal;
    out.clip_position = view.clip_from_world * vec4<f32>(net_pos, 1.0);
    
    out.color = vec4<f32>(
        base_color.rgb * brightness,
        base_color.a
    );
    
    // Calculate UVs from world position for proper tiling
    // Use the two axes perpendicular to the face normal
    if face == YP || face == YN {
        // XZ plane
        out.uv = vec2<f32>(net_pos.x, net_pos.z);
    } else if face == XP || face == XN {
        // YZ plane
        out.uv = vec2<f32>(net_pos.y, net_pos.z);
    } else {
        // ZP || ZN - XY plane
        out.uv = vec2<f32>(net_pos.x, net_pos.y);
    }

    return out;
}

@fragment  
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // Apply subtle noise texture for visual variety
    // Now using world-space UVs for consistent tiling
    let base_color = in.color;
    let scale = 1.0; // Scale of 1.0 means one checkerboard square per voxel
    let noise_u = floor(in.uv.x * scale);
    let noise_v = floor(in.uv.y * scale);
    let noise = (noise_u + noise_v) % 2.0 * 0.1 + 0.9; // Subtle brightness variation
    
    return vec4<f32>(base_color.rgb * noise, base_color.a);
}