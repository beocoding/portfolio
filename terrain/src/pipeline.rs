
use bevy::{core_pipeline::core_3d::Transparent3d, ecs::{query::ROQueryItem, system::{lifetimeless::{Read, SRes}, SystemParamItem}}, mesh::{PrimitiveTopology, VertexBufferLayout, VertexFormat}, pbr::{MeshPipeline, MeshPipelineKey, MeshViewBindGroup, SetMeshViewBindGroup}, prelude::*, render::{extract_component::ExtractComponentPlugin, extract_resource::ExtractResourcePlugin, render_phase::{AddRenderCommand, DrawFunctions, PhaseItem, PhaseItemExtraIndex, RenderCommand, RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewSortedRenderPhases}, render_resource::{BindGroupLayout, BlendState, Buffer, BufferDescriptor, BufferUsages, CachedPipelineState, Canonical, ColorTargetState, ColorWrites, CompareFunction, DepthBiasState, DepthStencilState, DrawIndirectArgs, FragmentState, MultisampleState, PipelineCache, PrimitiveState, PushConstantRange, RenderPipeline, RenderPipelineDescriptor, ShaderStages, Specializer, SpecializerKey, StencilState, TextureFormat, Variants, VertexAttribute, VertexState, VertexStepMode}, renderer::{RenderDevice, RenderQueue}, settings::{WgpuFeatures, WgpuSettings}, sync_world::RenderEntity, view::{ExtractedView, ViewTarget, ViewUniformOffset}, Extract, Render, RenderApp, RenderStartup, RenderSystems}};

use crate::{buffer::{TerrainMeshData, TerrainMeshGpuBuffer}, config::{ChunkSettings, NoiseSettings, NoisePatterns}, noise::{generate_octave_fractals, generate_tile_mesh, DirtyTile, TerrainMeshlet, TerrainTileId, TerrainVertex}};
const SHADER_ASSET_PATH: &str = "shaders/terrain.wgsl";

#[derive(Resource, Default)]
pub struct FrameCounter {
    pub current_frame: u32,
}

impl FrameCounter{
    #[inline(always)]
    pub const fn tick(&mut self) {
        self.current_frame+=1
    }
}
// ============================================================================
// PIPELINE KEY
// ============================================================================


#[derive(Clone, Copy, PartialEq, Eq, Hash, SpecializerKey)]
pub struct TerrainMeshPipelineKey {
    mesh_key: MeshPipelineKey,
}

impl From<MeshPipelineKey> for TerrainMeshPipelineKey {
    fn from(mesh_key: MeshPipelineKey) -> Self {
        Self { mesh_key }
    }
}


// ============================================================================
// SPECIALIZER
// ============================================================================


pub struct TerrainMeshSpecializer {
    pub mesh_pipeline: MeshPipeline,
    pub bind_group_layout: BindGroupLayout,
}
impl Specializer<RenderPipeline> for TerrainMeshSpecializer {
    type Key = TerrainMeshPipelineKey;

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
// PIPELINE RESOURCE
// ============================================================================

#[derive(Resource)]
pub struct TerrainMeshPipeline {
    pub variants: Variants<RenderPipeline, TerrainMeshSpecializer>,
    pub bind_group_layout: BindGroupLayout,
}

impl FromWorld for TerrainMeshPipeline {
    fn from_world(world: &mut World) -> Self {
        println!("🔧 TerrainMeshPipeline::from_world - Initializing pipeline");
        
        let render_device = world.resource::<RenderDevice>();
        let asset_server = world.resource::<AssetServer>();
        let mesh_pipeline = world.resource::<MeshPipeline>().clone();

        // No bind group layout needed anymore (or keep it empty)
        let bind_group_layout = render_device.create_bind_group_layout(
            Some("terrain_mesh_bind_group_layout"),
            &[],
        );

        let shader: Handle<Shader> = asset_server.load(SHADER_ASSET_PATH);
        println!("  📄 Shader loaded: terrain.wgsl");

        let vertex_layouts = vec![VertexBufferLayout {
            array_stride: std::mem::size_of::<TerrainVertex>() as u64,
            step_mode: VertexStepMode::Vertex,
            attributes: vec![
                VertexAttribute {
                    format: VertexFormat::Float32,
                    offset: 0,
                    shader_location: 0,
                },
                VertexAttribute {
                    format: VertexFormat::Uint32,
                    offset: 4,
                    shader_location: 1,
                },
            ],
        }];

        let push_constant_ranges = vec![
            PushConstantRange {
                stages: ShaderStages::VERTEX,
                range: 0..std::mem::size_of::<TerrainPushConstants>() as u32,  // Use ChunkConfig size
            }
        ];

        // Create base pipeline descriptor
        let base_descriptor = RenderPipelineDescriptor {
            label: Some("terrain_pipeline".into()),
            layout: vec![],
            push_constant_ranges,  // Add this!
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
            depth_stencil: None,
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                shader: shader.clone(),
                entry_point: Some("fragment".into()),
                ..default()
            }),
            zero_initialize_workgroup_memory: true,
        };

        let variants = Variants::new(
            TerrainMeshSpecializer {
                mesh_pipeline: mesh_pipeline.clone(),
                bind_group_layout: bind_group_layout.clone(),
            },
            base_descriptor,
        );

        println!("  ✅ Pipeline variants created");

        Self { 
            variants,
            bind_group_layout,
        }
    }
}

// ============================================================================
// Plugin
// ============================================================================

pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        let chunk_config = ChunkSettings::default();
        let mut noise_config = NoiseSettings::default();
        // noise_config.seed = Some(noise_config.seed());
        
        app
            .insert_resource(chunk_config.clone())
            .insert_resource(noise_config.clone())
            .init_resource::<ChunkSettings>()
            .add_plugins(ExtractComponentPlugin::<TerrainTileId>::default())
            .add_plugins(ExtractResourcePlugin::<ChunkSettings>::default())
            .add_plugins(ExtractResourcePlugin::<NoiseSettings>::default());

    }
    
    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        // Request the INDRECT_FIRST_INSTANCE feature
        let mut wgpu_settings = WgpuSettings::default();
        wgpu_settings.features |= WgpuFeatures::INDIRECT_FIRST_INSTANCE;
        render_app
            .init_resource::<FrameCounter>()
            .init_resource::<TerrainMeshGpuBuffer>()
            .init_resource::<TerrainMultiDrawBuffer>()
            // inside TerrainPlugin::finish (after you get render_app)
            .add_render_command::<Transparent3d, DrawCustomIndirect>()

            .add_systems(
                RenderStartup,
                (init_terrain_pipeline, init_terrain_gpu_buffer),
            )
            .add_systems(ExtractSchedule, extract_dirty_tiles)
            .add_systems(
                Render,
                (                   
                    prepare_vertex_buffers.in_set(RenderSystems::PrepareResources),
                    prepare_multi_draw.in_set(RenderSystems::PrepareResources).after(prepare_vertex_buffers),
                    queue_terrain_tiles.in_set(RenderSystems::Queue),
                    cleanup_terrain_buffers.in_set(RenderSystems::Cleanup),  // Runs after GPU finishes
                    tick_frame_counter.in_set(RenderSystems::PostCleanup)
                ),
            )
            .add_systems(Update, tick_frame_counter);
    }
}

// ============================================================================
// Systems
// ============================================================================

fn tick_frame_counter(mut frame_counter: ResMut<FrameCounter>) {
    frame_counter.tick();
}

fn init_terrain_pipeline(world: &mut World) {
    println!("🚀 init_terrain_pipeline called");
    let pipeline = TerrainMeshPipeline::from_world(world);
    world.insert_resource(pipeline);
    println!("  ✅ TerrainMeshPipeline inserted into world");
}

fn init_terrain_gpu_buffer(
    mut gpu_buffer: ResMut<TerrainMeshGpuBuffer>,
    render_device: Res<RenderDevice>,
) {
    println!("💾 Initializing GPU buffer");
    println!("  Vertex size: {} bytes", std::mem::size_of::<TerrainVertex>());
    println!("  Expected: 8 bytes (f32 altitude + u32 normal)");
    println!("  Vertices per chunk: 2176");
    println!("  Bytes per chunk: {}", 2176 * std::mem::size_of::<TerrainVertex>());
    
    gpu_buffer.ensure_created(&render_device);
    println!("  ✅ Buffer size: {} bytes", gpu_buffer.buffer_size);
}

#[inline(always)]
pub fn init_terrain(
    mut commands: Commands,
    mut noise_config: ResMut<NoiseSettings>,
    chunk_config: Res<ChunkSettings>,
) {
    noise_config.seed = Some(noise_config.seed());
    match &noise_config.noise_type {
        NoisePatterns::OctaveFractal => {
            spawn_octave_fractals(commands, &noise_config, &chunk_config);
        }
    }
}

#[inline(always)]
pub fn recalculate_terrain(
    mut query: Query<(&TerrainTileId, &mut TerrainMeshlet)>,
    noise_config: Res<NoiseSettings>,
    chunk_config: Res<ChunkSettings>,
) {

    match &noise_config.noise_type {
        NoisePatterns::OctaveFractal => {
            recalculate_octave_fractals(query, &noise_config, &chunk_config);
        }
    }
}


// Call this in your spawn_octave_fractals function:
pub fn spawn_octave_fractals(
    mut commands: Commands,
    noise_config: &NoiseSettings,
    chunk_config: &ChunkSettings
) {
    let chunk_size = chunk_config.chunk_size as usize;
    let samples_per_tile = (chunk_size+2)*(chunk_size+2);
    let mut heightmap = generate_octave_fractals(&noise_config, &chunk_config);

    for (idx, tile) in heightmap.chunks_exact_mut(samples_per_tile).enumerate() {
        let meshlet = generate_tile_mesh(tile, chunk_size);
        
        commands.spawn((
            TerrainTileId(idx as u32),
            meshlet,
            Visibility::default(),
            ViewVisibility::default(),
            InheritedVisibility::default(),
        ));
    }
    
}

pub fn recalculate_octave_fractals(
    mut query: Query<(&TerrainTileId, &mut TerrainMeshlet)>,
    noise_config: &NoiseSettings,
    chunk_config: &ChunkSettings,
) {
    let chunk_size = chunk_config.chunk_size as usize;
    let samples_per_tile = (chunk_size + 2) * (chunk_size + 2);

    // Recalculate full noise map
    let mut heightmap = generate_octave_fractals(&noise_config, &chunk_config);

    for (tile_id, mut meshlet) in query.iter_mut() {
        let id = tile_id.0 as usize;
        let start = id * samples_per_tile;
        let end = start + samples_per_tile;
        if end <= heightmap.len() {
            let tile_heights = &mut heightmap[start..end];

            // Rebuild mesh vertices/normals for this tile
            *meshlet = generate_tile_mesh(tile_heights, chunk_size);
        }
    }
}

#[inline(always)]
pub fn extract_dirty_tiles(
    mut commands: Commands,
    mut render_terrain_query: Query<&mut TerrainMeshData>,
    changed_tiles: Extract<Query<(&RenderEntity, &TerrainMeshlet), Or<(Added<TerrainMeshlet>, Changed<TerrainMeshlet>)>>>,
) {
    for (render_entity, meshlet) in changed_tiles.iter() {
        let render_entity_id = render_entity.id();
        if let Ok(mut terrain_data) = render_terrain_query.get_mut(render_entity_id) {
            terrain_data.stage_data(&meshlet.0);
        } else {
            commands.entity(render_entity_id).insert(TerrainMeshData::new(meshlet.clone()));
        }
        commands.entity(render_entity_id).insert(DirtyTile);
    }
}
pub fn prepare_vertex_buffers(
    mut terrain_buffer: ResMut<TerrainMeshGpuBuffer>,
    mut query: Query<&mut TerrainMeshData, With<DirtyTile>>,
    render_queue: Res<RenderQueue>,
    frame_counter: Res<FrameCounter>,
) {
    let current_frame = frame_counter.current_frame;    
    for mut meshlet in query.iter_mut() {
        meshlet.flush(&mut terrain_buffer, &render_queue, current_frame);
    }

}

pub fn cleanup_terrain_buffers(
    mut commands: Commands,
    mut terrain_query: Query<(Entity, &mut TerrainMeshData), With<DirtyTile>>,
    mut global_pool: ResMut<TerrainMeshGpuBuffer>,
    frame_counter: Res<FrameCounter>,
) {
    let cleanup_count = terrain_query.iter().count();
    if cleanup_count > 0 {
        println!("🧹 Cleaning up {} dirty tiles at frame {}", cleanup_count, frame_counter.current_frame);
    }
    
    for (entity, mut mesh_data) in terrain_query.iter_mut() {
        mesh_data.cleanup_old_ranges(&mut global_pool, frame_counter.current_frame);
        commands.entity(entity).remove::<DirtyTile>();
    }
}
// ============================================================================
// QUEUE SYSTEM
// ============================================================================
fn queue_terrain_tiles(
    transparent_3d_draw_functions: Res<DrawFunctions<Transparent3d>>,
    mut terrain_pipeline: ResMut<TerrainMeshPipeline>,
    pipeline_cache: Res<PipelineCache>,
    mut transparent_render_phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    views: Query<(Entity, &ExtractedView, &Msaa)>,
) {

    let draw_custom = transparent_3d_draw_functions
        .read()
        .id::<DrawCustomIndirect>();

    for (view_entity, view, msaa) in &views {
        let Some(transparent_phase) = transparent_render_phases
            .get_mut(&view.retained_view_entity) 
        else {
            continue;
        };

        let msaa_key = MeshPipelineKey::from_msaa_samples(msaa.samples());
        let view_key = msaa_key | MeshPipelineKey::from_hdr(view.hdr);
        let key = view_key | MeshPipelineKey::from_primitive_topology(PrimitiveTopology::TriangleStrip);

        let Ok(pipeline) = terrain_pipeline.variants.specialize(&pipeline_cache, key.into()) else {
            continue;
        };

        if !matches!(pipeline_cache.get_render_pipeline_state(pipeline), CachedPipelineState::Ok(_)) {
            continue;
        }

        // Add ONE phase item that will draw all tiles via multi-draw
        transparent_phase.add(Transparent3d {
            entity: (view_entity, view_entity.into()),  // Use view entity
            pipeline,
            draw_function: draw_custom,
            distance: 0.0,
            batch_range: 0..1,
            extra_index: PhaseItemExtraIndex::None,
            indexed: false,
        });
        
    }
}


// ============================================================================
// RENDER COMMANDS
// ============================================================================

// Update DrawCustomIndirect
type DrawCustomIndirect = (
    SetItemPipeline,
    SetMeshViewBindGroup<0>,
    SetTerrainPushConstants, 
    DrawTerrainMeshMultiIndirect,
);

pub struct SetTerrainViewBindGroup<const I: usize>;

impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetTerrainViewBindGroup<I> {
    type Param = ();
    type ViewQuery = (Read<ViewUniformOffset>, Read<MeshViewBindGroup>);
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _entity: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        _param: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        println!("🔧 SetTerrainViewBindGroup CALLED!");
        
        let (view_uniform_offset, mesh_view_bind_group) = view;
        pass.set_bind_group(I, &mesh_view_bind_group.main, &[view_uniform_offset.offset]);
        
        println!("  ✅ Bind group set");
        RenderCommandResult::Success
    }
}
pub struct DrawTerrainMeshMultiIndirect;

impl<P: PhaseItem> RenderCommand<P> for DrawTerrainMeshMultiIndirect {
    type Param = (SRes<TerrainMeshGpuBuffer>, SRes<TerrainMultiDrawBuffer>);
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        _view: (),
        _query_item: Option<ROQueryItem<Self::ItemQuery>>,
        (terrain_buffer, multi_draw): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let terrain_buffer = terrain_buffer.into_inner();
        let multi_draw = multi_draw.into_inner();
        
        let Some(vertex_buffer) = &terrain_buffer.buffer else {
            println!("  ⚠️ No vertex buffer");
            return RenderCommandResult::Skip;
        };
        
        let Some(indirect_buffer) = &multi_draw.buffer else {
            println!("  ⚠️ No indirect buffer");
            return RenderCommandResult::Skip;
        };
        
        let active_draws = multi_draw.active_draw_count();
        if active_draws == 0 {
            println!("  ⚠️ No active draws");
            return RenderCommandResult::Skip;
        }
        
        // Bind the ENTIRE vertex buffer (not slices)
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        
        // Execute all draws in one multi-draw call
        // multi_draw_indirect(indirect_buffer, offset_in_bytes, draw_count)
        pass.multi_draw_indirect(indirect_buffer, 0, active_draws);
        
        // println!("  🎯 Multi-draw: {} tiles in ONE call", active_draws);
        
        RenderCommandResult::Success
    }
}

#[derive(Debug, Clone)]
pub struct MultiDrawBuffer {
    pub buffer: Option<Buffer>,
    pub commands: Vec<DrawIndirectArgs>,  // Index = tile_id
    pub buffer_size: u64,
}

impl MultiDrawBuffer {
    pub fn new(tile_count: usize) -> Self {
        let command_size = std::mem::size_of::<DrawIndirectArgs>() as u64;
        let buffer_size = command_size * tile_count as u64;
        
        // Initialize with zeroed commands
        let commands = vec![DrawIndirectArgs {
            vertex_count: 0,
            instance_count: 0,
            first_vertex: 0,
            first_instance: 0,
        }; tile_count];
        
        Self {
            buffer: None,
            commands,
            buffer_size,
        }
    }
    
    #[inline(always)]
    pub fn ensure_created(&mut self, device: &RenderDevice) {
        if self.buffer.is_none() {
            self.buffer = Some(device.create_buffer(&BufferDescriptor {
                label: Some("MultiDrawBuffer"),
                size: self.buffer_size,
                usage: BufferUsages::INDIRECT | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
    }
    
    /// Update a single tile's command in CPU buffer
    pub fn update_command(&mut self, tile_id: u32, first_vertex: u32, vertex_count: u32) {
        let idx = tile_id as usize;
        if idx >= self.commands.len() { return; }
        
        let cmd = &mut self.commands[idx];
        cmd.vertex_count = vertex_count;
        cmd.instance_count = if vertex_count > 0 { 1 } else { 0 };
        cmd.first_vertex = first_vertex;
        cmd.first_instance = tile_id; // ✅ Use tile_id as instance index
    }
    
    /// Upload a single command to GPU
    #[inline(always)]
    pub fn upload_command(&self, queue: &RenderQueue, tile_id: u32) {
        let idx = tile_id as usize;
        
        if idx >= self.commands.len() {
            return;
        }
        
        if let Some(buffer) = &self.buffer {
            let command_size = std::mem::size_of::<DrawIndirectArgs>() as u64;
            let offset = idx as u64 * command_size;
            let data = bytemuck::cast_slice(&self.commands[idx..idx + 1]);
            queue.write_buffer(buffer, offset, data);
        }
    }
    
    /// Update and upload a single command atomically
    #[inline(always)]
    pub fn update_and_upload_command(
        &mut self, 
        queue: &RenderQueue, 
        tile_id: u32, 
        first_vertex: u32, 
        vertex_count: u32
    ) {
        self.update_command(tile_id, first_vertex, vertex_count);
        self.upload_command(queue, tile_id);
    }
    
    /// Upload a slice of commands to GPU
    #[inline(always)]
    pub fn upload_slice(&self, queue: &RenderQueue, start_tile: u32, count: u32) {
        let start_idx = start_tile as usize;
        let end_idx = (start_tile + count) as usize;
        
        if start_idx >= self.commands.len() || end_idx > self.commands.len() {
            return;
        }
        
        if let Some(buffer) = &self.buffer {
            let command_size = std::mem::size_of::<DrawIndirectArgs>() as u64;
            let offset = start_idx as u64 * command_size;
            let data = bytemuck::cast_slice(&self.commands[start_idx..end_idx]);
            queue.write_buffer(buffer, offset, data);
        }
    }
    
    /// Upload all commands to GPU
    #[inline(always)]
    pub fn upload_all(&self, queue: &RenderQueue) {
        if let Some(buffer) = &self.buffer {
            let data = bytemuck::cast_slice(&self.commands);
            queue.write_buffer(buffer, 0, data);
        }
    }
    
    /// Get the number of active draws (tiles with vertex_count > 0)
    #[inline(always)]
    pub fn active_draw_count(&self) -> u32 {
        self.commands.iter()
            .filter(|cmd| cmd.vertex_count > 0)
            .count() as u32
    }
    
    /// Get the total capacity
    #[inline(always)]
    pub fn capacity(&self) -> usize {
        self.commands.len()
    }
}

#[derive(Resource, Deref, DerefMut, Clone)]
pub struct TerrainMultiDrawBuffer(pub MultiDrawBuffer);

impl Default for TerrainMultiDrawBuffer {
    fn default() -> Self {
        Self(MultiDrawBuffer::new(256))
    }
}

pub fn prepare_multi_draw(
    mut multi_draw: ResMut<TerrainMultiDrawBuffer>,
    config: Res<ChunkSettings>,
    query: Query<(&TerrainMeshData, &TerrainTileId), Changed<TerrainMeshData>>,
    render_queue: Res<RenderQueue>,
    render_device: Res<RenderDevice>,
) {
    let tiles_x = config.map_width / config.chunk_size;
    let tiles_z = config.map_height / config.chunk_size;
    let expected_tile_count = (tiles_x * tiles_z) as usize;
    
    if multi_draw.capacity() != expected_tile_count {
        *multi_draw = TerrainMultiDrawBuffer(MultiDrawBuffer::new(expected_tile_count));
    }
    
    multi_draw.ensure_created(&render_device);
    
    let changed_count = query.iter().count();
    if changed_count > 0 {
        println!("\n🎮 ============ MULTI-DRAW UPDATE ============");
        println!("  {} tiles marked as changed", changed_count);
    }
    
    let mut updated_count = 0;
    for (mesh_data, tile_id) in query.iter() {
        if let Some((first_vertex, vertex_count)) = mesh_data.draw_range() {
            multi_draw.update_and_upload_command(&render_queue, tile_id.0, first_vertex, vertex_count);
            if updated_count < 3 {  // Print first 3
                println!("  Tile {}: first_vertex={}, vertex_count={}", tile_id.0, first_vertex, vertex_count);
            }
            updated_count += 1;
        } else {
            multi_draw.update_and_upload_command(&render_queue, tile_id.0, 0, 0);
        }
    }
    
    if changed_count > 0 {
        println!("  ✅ Updated {} tile commands", updated_count);
        println!("============================================\n");
    }
}

pub struct SetTerrainPushConstants;

impl<P: PhaseItem> RenderCommand<P> for SetTerrainPushConstants {
    type Param = (SRes<ChunkSettings>, SRes<NoiseSettings>);  // ← Both resources
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        _view: (),
        _entity: Option<()>,
        (chunk_config, noise_config): SystemParamItem<'w, '_, Self::Param>,  // ← Destructure both
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let push_constants = TerrainPushConstants::from_configs(
            chunk_config.into_inner(),
            noise_config.into_inner()
        );
        
        let bytes = bytemuck::bytes_of(&push_constants);
        pass.set_push_constants(ShaderStages::VERTEX, 0, bytes);
        
        RenderCommandResult::Success
    }
}

// Add this near the top of your file with other types
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct TerrainPushConstants {
    view_distance: u32,
    chunk_size: u32,
    map_height: u32,
    map_width: u32,
    height_multiplier: f32,
}

impl TerrainPushConstants {
    fn from_configs(chunk: &ChunkSettings, noise: &NoiseSettings) -> Self {
        Self {
            view_distance: chunk.view_distance,
            chunk_size: chunk.chunk_size,
            map_height: chunk.map_height,
            map_width: chunk.map_width,
            height_multiplier: noise.height_scale,
        }
    }
}