use std::fs;

use bevy::{
    asset::RenderAssetUsages, 
    core_pipeline::core_3d::Transparent3d, 
    ecs::{
        error::BevyError,
        system::{lifetimeless::SRes, SystemParamItem}
    }, 
    mesh::{Indices, PrimitiveTopology, VertexBufferLayout, VertexFormat}, 
    pbr::{
        MeshPipeline, MeshPipelineKey, RenderMeshInstances, 
        SetMeshBindGroup, SetMeshViewBindGroup
    }, 
    prelude::*, 
    render::{
        extract_component::ExtractComponentPlugin, 
        extract_resource::ExtractResourcePlugin,
        mesh::{
            allocator::MeshAllocator,
            RenderMesh, 
            RenderMeshBufferInfo,
        }, 
        render_asset::RenderAssets, 
        render_phase::{
            AddRenderCommand, DrawFunctions, PhaseItem, PhaseItemExtraIndex, 
            RenderCommand, RenderCommandResult, SetItemPipeline, 
            TrackedRenderPass, ViewSortedRenderPhases
        }, 
        render_resource::{
            BindGroupLayout, BlendState, Canonical, ColorTargetState, 
            ColorWrites, CompareFunction, DepthBiasState, DepthStencilState, 
            FragmentState, MultisampleState, PipelineCache, PrimitiveState, 
            RenderPipeline, RenderPipelineDescriptor, Specializer, 
            SpecializerKey, StencilState, TextureFormat, Variants, 
            VertexAttribute, VertexState, VertexStepMode
        }, 
        renderer::{RenderDevice, RenderQueue}, 
        sync_world::MainEntity, 
        view::{ExtractedView, ViewTarget}, 
        Render, RenderApp, RenderSystems
    }
};

use crate::{
    bits::{build_mesh_into, ChunkData}, 
    buffers::{
        ChunkMeshRange, Dirty, FrameCounter, InstanceMetaBuffer, InstancedMeta, MultiDrawBuffer, VoxelInstancePool
    }, 
    chunk_config, 
    constants::{FaceDirection, TempChunkMeshData, VoxelInstance}, 
    index::index::ChunkIndex
};

const SHADER_ASSET_PATH: &str = "shaders/instancing.wgsl";

fn write_shader_with_constants() {
    let constants = format!(
        r#"
// === Auto-generated constants ===
const CHUNK_SIZE: f32 = {}.0;
const CHUNK_AXIS_BITS: u32 = {}u;
const CHUNK_Y_MASK: u32 = (1u << CHUNK_AXIS_BITS) - 1u;
const CHUNK_X_MASK: u32 = CHUNK_Y_MASK << CHUNK_AXIS_BITS;
const CHUNK_Z_MASK: u32 = CHUNK_Y_MASK << (CHUNK_AXIS_BITS * 2u);
"#,
        chunk_config::CHUNK_SIZE,
        chunk_config::CHUNK_AXIS_BITS,
    );
    let base_shader = fs::read_to_string("assets/shaders/voxel_base.wgsl")
        .expect("Failed to read base shader");
    let final_shader = format!("{constants}\n\n{base_shader}");

    fs::write("assets/shaders/instancing.wgsl", final_shader)
        .expect("Failed to write final shader file");
}

// ============================================================================
// PIPELINE RESOURCE
// ============================================================================

#[derive(Resource)]
pub struct VoxelPipeline {
    pub variants: Variants<RenderPipeline, VoxelSpecializer>,
    // Keep layout exposed for bind group creation if needed
    pub bind_group_layout: BindGroupLayout,
}

// ============================================================================
// SPECIALIZER
// ============================================================================

pub struct VoxelSpecializer {
    pub mesh_pipeline: MeshPipeline,
    pub bind_group_layout: BindGroupLayout,
}
impl Specializer<RenderPipeline> for VoxelSpecializer {
    type Key = VoxelPipelineKey;

    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        // Set bind group layouts
        // 0: View data (camera, lights, etc.)
        descriptor.set_layout(
            0, 
            self.mesh_pipeline
                .get_view_layout(key.mesh_key.into())
                .main_layout
                .clone()
        );
        
        // 1: Mesh model data (transforms)
        descriptor.set_layout(
            1, 
            self.mesh_pipeline.mesh_layouts.model_only.clone()
        );

        // Configure multisampling based on key
        descriptor.multisample.count = key.mesh_key.msaa_samples();

        // Configure depth/stencil state for 3D rendering
        descriptor.depth_stencil = Some(DepthStencilState {
            format: TextureFormat::Depth32Float,
            depth_write_enabled: true,
            depth_compare: CompareFunction::Greater,
            stencil: StencilState::default(),
            bias: DepthBiasState::default(),
        });

        // Configure fragment output format based on HDR
        if let Some(fragment) = &mut descriptor.fragment {
            let format = if key.mesh_key.contains(MeshPipelineKey::HDR) {
                ViewTarget::TEXTURE_FORMAT_HDR
            } else {
                TextureFormat::bevy_default()
            };
            
            fragment.targets = vec![Some(ColorTargetState {
                format,
                blend: Some(BlendState::ALPHA_BLENDING),
                write_mask: ColorWrites::ALL,
            })];
        }

        // Return the key as canonical
        Ok(key)
    }
}
// ============================================================================
// PIPELINE KEY
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, Hash, SpecializerKey)]
pub struct VoxelPipelineKey {
    mesh_key: MeshPipelineKey,
}

impl From<MeshPipelineKey> for VoxelPipelineKey {
    fn from(mesh_key: MeshPipelineKey) -> Self {
        Self { mesh_key }
    }
}

// ============================================================================
// PIPELINE INITIALIZATION
// ============================================================================

impl FromWorld for VoxelPipeline {
    fn from_world(world: &mut World) -> Self {
        let render_device = world.resource::<RenderDevice>();
        let asset_server = world.resource::<AssetServer>();
        let mesh_pipeline = world.resource::<MeshPipeline>().clone();
        
        write_shader_with_constants();

        // Create bind group layout (empty for now, but ready for expansion)
        let bind_group_layout = render_device.create_bind_group_layout(
            Some("voxel_bind_group_layout"),
            &[],
        );

        // Define vertex buffer layouts
        let vertex_layouts = vec![
            // Layout 0: Vertex positions
            VertexBufferLayout {
                array_stride: 12,
                step_mode: VertexStepMode::Vertex,
                attributes: vec![VertexAttribute {
                    format: VertexFormat::Float32x3,
                    offset: 0,
                    shader_location: 0,
                }],
            },
            // Layout 1: VoxelInstance data (per-instance)
            VertexBufferLayout {
                array_stride: size_of::<VoxelInstance>() as u64,
                step_mode: VertexStepMode::Instance,
                attributes: vec![VertexAttribute {
                    format: VertexFormat::Uint32,
                    offset: 0,
                    shader_location: 1,
                }],
            },
            // Layout 2: Instance metadata (per-instance)
            VertexBufferLayout {
                array_stride: size_of::<InstanceMetaBuffer>() as u64,
                step_mode: VertexStepMode::Instance,
                attributes: vec![
                    VertexAttribute {
                        format: VertexFormat::Uint32,
                        offset: 0,
                        shader_location: 2,
                    },
                    VertexAttribute {
                        format: VertexFormat::Uint32,
                        offset: 4,
                        shader_location: 3,
                    },
                ],
            },
        ];

        let shader = asset_server.load(SHADER_ASSET_PATH);

        // Create base pipeline descriptor
        let base_descriptor = RenderPipelineDescriptor {
            label: Some("voxel_pipeline".into()),
            layout: vec![],  // Set in specialize
            push_constant_ranges: vec![],
            vertex: VertexState {
                shader: shader.clone(),
                entry_point: Some("vertex".into()),
                buffers: vertex_layouts.clone(),
                shader_defs: vec![],
            },
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleStrip,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,  // Set in specialize
            multisample: MultisampleState::default(),  // Set in specialize
            fragment: Some(FragmentState {
                shader: shader.clone(),
                entry_point: Some("fragment".into()),
                ..default()
            }),
            zero_initialize_workgroup_memory: true,
        };

        // Create variants with the specializer
        let variants = Variants::new(
            VoxelSpecializer {
                mesh_pipeline: mesh_pipeline.clone(),
                bind_group_layout: bind_group_layout.clone(),
            },
            base_descriptor,
        );

        Self { 
            variants,
            bind_group_layout,
        }
    }
}

// ============================================================================
// PLUGIN
// ============================================================================

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct VoxelInitSet;

pub struct VoxelEnginePlugin;

impl Plugin for VoxelEnginePlugin {
    fn build(&self, app: &mut App) {
        app 
            .init_resource::<VoxelInstancePool>()
            .init_resource::<InstancedMeta>()
            .init_resource::<FrameCounter>()
            .add_plugins(ExtractComponentPlugin::<ChunkIndex>::default())
            .add_plugins(ExtractComponentPlugin::<ChunkMeshRange>::default())
            .add_plugins(ExtractResourcePlugin::<FrameCounter>::default())
            .add_plugins(ExtractResourcePlugin::<VoxelInstancePool>::default())
            .add_plugins(ExtractResourcePlugin::<InstancedMeta>::default())

            .add_systems(Startup, FaceQuad::init.in_set(VoxelInitSet))
            .add_systems(Update, remesh_dirty);
    }
    
    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
                
        render_app
            .init_resource::<VoxelPipeline>()
            .init_resource::<MultiDrawBuffer>()      
            .add_render_command::<Transparent3d, DrawCustomIndirect>()
            .add_systems(
                Render,
                (
                    prepare_buffers.in_set(RenderSystems::PrepareResources),
                    queue_custom.in_set(RenderSystems::QueueMeshes),
                )
            );
    }
}

// ============================================================================
// QUEUE SYSTEM
// ============================================================================

fn queue_custom(
    transparent_3d_draw_functions: Res<DrawFunctions<Transparent3d>>,
    mut voxel_pipeline: ResMut<VoxelPipeline>,  // ✅ Needs mut for specialize
    pipeline_cache: Res<PipelineCache>,
    meshes: Res<RenderAssets<RenderMesh>>,
    render_mesh_instances: Res<RenderMeshInstances>,
    material_meshes: Query<(Entity, &MainEntity), With<ChunkMeshRange>>,
    mut transparent_render_phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    views: Query<(&ExtractedView, &Msaa)>,
) {
    let draw_custom = transparent_3d_draw_functions
        .read()
        .id::<DrawCustomIndirect>();

    for (view, msaa) in &views {
        let Some(transparent_phase) = transparent_render_phases
            .get_mut(&view.retained_view_entity) 
        else {
            continue;
        };

        // Queue all chunk meshes
        for (entity, main_entity) in material_meshes.iter() {
            let Some(mesh_instance) = render_mesh_instances
                .render_mesh_queue_data(*main_entity) 
            else {
                continue;
            };
            
            let Some(mesh) = meshes.get(mesh_instance.mesh_asset_id) else {
                continue;
            };
            
            // Build pipeline key from view and mesh properties
            let msaa_key = MeshPipelineKey::from_msaa_samples(msaa.samples());
            let view_key = msaa_key | MeshPipelineKey::from_hdr(view.hdr);
            let key = view_key | MeshPipelineKey::from_primitive_topology(
                mesh.primitive_topology()
            );
            
            // ✅ Specialize the pipeline
            let Ok(pipeline) = voxel_pipeline.variants.specialize(
                &pipeline_cache, 
                key.into()
            ) else {
                continue;
            };
            
            // Add to transparent phase
            transparent_phase.add(Transparent3d {
                entity: (entity, *main_entity),
                pipeline,
                draw_function: draw_custom,
                distance: 0.0,
                batch_range: 0..1,
                extra_index: PhaseItemExtraIndex::None,
                indexed: true,
            });
        }
    }
}

// ============================================================================
// RENDER COMMANDS
// ============================================================================

type DrawCustomIndirect = (
    SetItemPipeline,
    SetMeshViewBindGroup<0>,
    SetMeshBindGroup<1>,
    DrawMeshInstancedIndirect,
);

pub struct DrawMeshInstancedIndirect;

impl<P: PhaseItem> RenderCommand<P> for DrawMeshInstancedIndirect {
    type Param = (
        SRes<RenderAssets<RenderMesh>>,
        SRes<RenderMeshInstances>,
        SRes<MeshAllocator>,
        SRes<VoxelInstancePool>,
        SRes<InstancedMeta>,
        SRes<MultiDrawBuffer>,
    );
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        item: &P,
        _view: (),
        _item_query: Option<()>,
        (
            meshes,
            render_mesh_instances,
            mesh_allocator,
            instances,
            meta,
            multi_draw_buffer,
        ): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        // Convert all SRes to inner references
        let meshes = meshes.into_inner();
        let render_mesh_instances = render_mesh_instances.into_inner();
        let mesh_allocator = mesh_allocator.into_inner();
        let instances = instances.into_inner();
        let meta = meta.into_inner();
        let multi_draw_buffer = multi_draw_buffer.into_inner();

        // Early exit if no draw commands
        if multi_draw_buffer.commands.is_empty() {
            return RenderCommandResult::Skip;
        }

        // Get mesh data or skip
        let Some(mesh_instance) = render_mesh_instances
            .render_mesh_queue_data(item.main_entity()) 
        else {
            return RenderCommandResult::Skip;
        };
        
        let Some(gpu_mesh) = meshes.get(mesh_instance.mesh_asset_id) else {
            return RenderCommandResult::Skip;
        };
        
        let Some(vertex_buffer_slice) = mesh_allocator
            .mesh_vertex_slice(&mesh_instance.mesh_asset_id) 
        else {
            return RenderCommandResult::Skip;
        };

        // Get GPU buffers or skip
        let Some(gpu_buffer) = &multi_draw_buffer.gpu_buffer else {
            return RenderCommandResult::Skip;
        };
        
        let Some(instance_buffer) = &instances.buffer else {
            return RenderCommandResult::Skip;
        };
        
        let Some(meta_buffer) = &meta.buffer else {
            return RenderCommandResult::Skip;
        };

        // Bind vertex buffers
        pass.set_vertex_buffer(0, vertex_buffer_slice.buffer.slice(..));
        pass.set_vertex_buffer(1, instance_buffer.slice(..));
        pass.set_vertex_buffer(2, meta_buffer.slice(..));

        // Execute draw command (indexed or non-indexed)
        match &gpu_mesh.buffer_info {
            RenderMeshBufferInfo::Indexed { index_format, .. } => {
                let Some(index_buffer_slice) = mesh_allocator
                    .mesh_index_slice(&mesh_instance.mesh_asset_id) 
                else {
                    return RenderCommandResult::Skip;
                };
                
                pass.set_index_buffer(
                    index_buffer_slice.buffer.slice(..), 
                    0, 
                    *index_format
                );
                
                pass.multi_draw_indirect(
                    gpu_buffer, 
                    0, 
                    multi_draw_buffer.commands.len() as u32
                );
            }
            RenderMeshBufferInfo::NonIndexed => {
                pass.multi_draw_indirect(
                    gpu_buffer, 
                    0, 
                    multi_draw_buffer.commands.len() as u32
                );
            }
        }

        RenderCommandResult::Success
    }
}

// ============================================================================
// MESH PREPARATION
// ============================================================================

#[inline(always)]
pub fn remesh_dirty(
    mut commands: Commands,
    mut instances: ResMut<VoxelInstancePool>,
    mut query: Query<(Entity, &ChunkData, &mut ChunkMeshRange), With<Dirty>>,
    camera_query: Query<&Transform, With<Camera>>,
    render_device: Res<RenderDevice>,
    mut frame_counter: ResMut<FrameCounter>,
) {
    instances.ensure_created(&render_device);
    let mut results = TempChunkMeshData::new();
    let current_frame = frame_counter.current_frame();
    // Get camera view direction for face culling
    let exclude_faces = if let Ok(camera_transform) = camera_query.single() {
        get_backfacing_faces(camera_transform)
    } else {
        vec![]
    };
    
    for (entity, chunk, mut chunkmesh) in query.iter_mut() {
        // Handle empty chunks
        if chunk.is_empty() {
            chunkmesh.stage_empty();
            chunkmesh.try_swap(current_frame);
            chunkmesh.cleanup_old_ranges(&mut instances, current_frame);
            commands.entity(entity).remove::<Dirty>();
            continue;
        }
        
        // Build mesh with view-based face culling
        build_mesh_into(chunk, &exclude_faces, &mut results);
        chunkmesh.stage_face_data(&results.data);
        
        commands.entity(entity).remove::<Dirty>();
    }
    
    frame_counter.advance_frame();
}

fn get_backfacing_faces(camera_transform: &Transform) -> Vec<FaceDirection> {
    let view_dir = camera_transform.forward();
    
    const FACE_NORMALS: [(FaceDirection, Vec3); 6] = [
        (FaceDirection::YP, Vec3::new(0.0, 1.0, 0.0)),
        (FaceDirection::YN, Vec3::new(0.0, -1.0, 0.0)),
        (FaceDirection::XP, Vec3::new(1.0, 0.0, 0.0)),
        (FaceDirection::XN, Vec3::new(-1.0, 0.0, 0.0)),
        (FaceDirection::ZP, Vec3::new(0.0, 0.0, 1.0)),
        (FaceDirection::ZN, Vec3::new(0.0, 0.0, -1.0)),
    ];
    
    FACE_NORMALS
        .iter()
        .filter(|(_, normal)| view_dir.dot(*normal) > 0.1)
        .map(|(face_dir, _)| *face_dir)
        .collect()
}

fn prepare_buffers(
    mut multi_draw_buffer: ResMut<MultiDrawBuffer>,
    mut instances: ResMut<VoxelInstancePool>,
    mut meta: ResMut<InstancedMeta>,
    mut query: Query<(&mut ChunkMeshRange, &ChunkIndex)>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    frame_counter: Res<FrameCounter>,
) {
    meta.ensure_created(&render_device);
    multi_draw_buffer.ensure_created(&render_device);
    multi_draw_buffer.begin_frame();
    let current_frame = frame_counter.current_frame();
    
    for (mut chunkmesh, index) in query.iter_mut() {
        if chunkmesh.upload(&mut instances, &render_queue, current_frame) {
            if let Some((first_instance, data)) = chunkmesh.face_metas(*index) {
                meta.write_at_index(
                    &render_queue, 
                    first_instance as usize, 
                    &data
                );
            }
        }
        
        let draws = chunkmesh.draw_buffer();
        chunkmesh.cleanup_old_ranges(&mut instances, current_frame);
        multi_draw_buffer.stage(&draws);
    }
    
    multi_draw_buffer.upload(&render_device);
}

// ============================================================================
// HELPER RESOURCES
// ============================================================================

#[derive(Resource, Deref, DerefMut, Clone)]
pub struct FaceQuad(pub Handle<Mesh>);

impl FaceQuad {
    #[inline(always)]
    pub fn init(
        mut commands: Commands,
        mut meshes: ResMut<Assets<Mesh>>,
    ) {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleStrip, 
            RenderAssetUsages::RENDER_WORLD
        );
        
        // Triangle strip vertices forming a quad
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![
                [0.0, 0.0, 0.0], // Bottom-left
                [1.0, 0.0, 0.0], // Bottom-right
                [0.0, 0.0, 1.0], // Top-left  
                [1.0, 0.0, 1.0], // Top-right
            ],
        );
        
        mesh.insert_indices(Indices::U16(vec![0, 1, 2, 3]));
        
        let base_quad = meshes.add(mesh);
        commands.insert_resource(FaceQuad(base_quad));
        println!("Initialized FaceQuad");
    }
}