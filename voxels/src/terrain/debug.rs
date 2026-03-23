
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat
};

pub fn spawn_grid_mesh(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    // Add a light
    commands.spawn((
        DirectionalLight {
            illuminance: 10000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    
    // Create a 32x32 grid mesh
    let grid_mesh = create_grid_mesh(32, 32, 1.0);
    
    // Generate a checkerboard texture - make each square 2x2 cells for visibility
    let texture_handle = images.add(create_checkerboard_texture(64, 64, 2));
    
    commands.spawn((
        Mesh3d(meshes.add(grid_mesh)),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(texture_handle),
            base_color: Color::WHITE,
            unlit: false,
            double_sided: true,
            ..default()
        })),
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));
    
    println!("Grid mesh spawned at (0, 0, 0), extends to (32, 0, 32)");
}

fn create_checkerboard_texture(width: u32, height: u32, square_size: u32) -> Image {
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    
    for y in 0..height {
        for x in 0..width {
            // Create checkerboard with squares of square_size x square_size
            let checker_x = (x / square_size) % 2;
            let checker_y = (y / square_size) % 2;
            let is_white = (checker_x + checker_y) % 2 == 0;
            
            if is_white {
                data.extend_from_slice(&[200, 200, 200, 255]); // Light gray
            } else {
                data.extend_from_slice(&[60, 60, 60, 255]); // Dark gray
            }
        }
    }
    
    let mut image = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    
    // Use nearest neighbor filtering to keep sharp edges
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mag_filter: ImageFilterMode::Nearest,
        min_filter: ImageFilterMode::Nearest,
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..default()
    });
    
    image
}

fn create_grid_mesh(width: u32, height: u32, cell_size: f32) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default()
    );
    
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    
    // Generate vertices
    for z in 0..=height {
        for x in 0..=width {
            let pos_x = x as f32 * cell_size;
            let pos_z = z as f32 * cell_size;
            
            positions.push([pos_x, 0.0, pos_z]);
            normals.push([0.0, 1.0, 0.0]);
            // UV coordinates from 0 to 1
            uvs.push([x as f32 / width as f32, z as f32 / height as f32]);
        }
    }
    
    // Generate indices for triangles
    for z in 0..height {
        for x in 0..width {
            let top_left = z * (width + 1) + x;
            let top_right = top_left + 1;
            let bottom_left = (z + 1) * (width + 1) + x;
            let bottom_right = bottom_left + 1;
            
            indices.push(top_left);
            indices.push(bottom_left);
            indices.push(top_right);
            
            indices.push(top_right);
            indices.push(bottom_left);
            indices.push(bottom_right);
        }
    }
    
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    
    mesh
}

// System to draw coordinate axes every frame
pub fn draw_axes(mut gizmos: Gizmos) {
    let axis_length = 50.0;
    let origin = Vec3::ZERO;
    
    // X axis - Red
    gizmos.arrow(
        origin,
        origin + Vec3::X * axis_length,
        Color::srgb(1.0, 0.0, 0.0),
    );
    
    // Y axis - Green
    gizmos.arrow(
        origin,
        origin + Vec3::Y * axis_length,
        Color::srgb(0.0, 1.0, 0.0),
    );
    
    // Z axis - Blue
    gizmos.arrow(
        origin,
        origin + Vec3::Z * axis_length,
        Color::srgb(0.0, 0.0, 1.0),
    );
}