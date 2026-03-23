// voxels/src/buffers.rs
use bevy::{
    prelude::*,
    render::{
        extract_component::ExtractComponent, extract_resource::ExtractResource, render_resource::*, renderer::{RenderDevice, RenderQueue}
    },
};
use bytemuck::{bytes_of, Pod, Zeroable};
use std::collections::VecDeque;

use crate::{buffer_config, constants::VoxelInstance, index::index::ChunkIndex};

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub struct DrawIndirectCommand {
    pub vertex_count: u32, // The number of vertices to draw.
    pub instance_count: u32, // The number of instances to draw.
    pub first_vertex: u32, // The vertex offset to start on
    pub first_instance: u32, // The instance ID of the first instance to draw.
}

#[derive(Component,Clone, Debug, Default,ExtractComponent)]
pub struct IndirectDrawBuffer{
    pub buffer: Vec<DrawIndirectCommand>, 
}

impl IndirectDrawBuffer {
    #[inline(always)]
    pub fn push(&mut self, command: DrawIndirectCommand) {
        self.buffer.push(command);
    }
    #[inline(always)]
    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    #[inline(always)]
    pub fn iter(&self) -> impl Iterator<Item = &DrawIndirectCommand> {
        self.buffer.iter()
    }
}

#[derive(Resource, FromWorld)]
pub struct MultiDrawBuffer {
    pub commands: Vec<DrawIndirectCommand>,
    pub gpu_buffer: Option<Buffer>,
}

impl MultiDrawBuffer {
    #[inline(always)]
    pub fn ensure_created(&mut self, device: &RenderDevice) {
        if self.gpu_buffer.is_none() {
            // Create a GPU buffer with some size and usage flags
            self.gpu_buffer = Some(device.create_buffer(&BufferDescriptor {
                label: Some("MultiDrawBuffer GPU Buffer"),
                size: 1024 * 1024, // example size, tune as needed
                usage: BufferUsages::INDIRECT | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
    }
    #[inline(always)]
    pub fn begin_frame(&mut self) {
        self.commands.clear();
    }

    #[inline(always)]
    pub fn push(&mut self, command: DrawIndirectCommand) {
        self.commands.push(command);
    }
    #[inline(always)]
    pub fn stage(&mut self, buffer: &IndirectDrawBuffer) {
        let commands = &buffer.buffer;
        self.commands.extend_from_slice(commands);
    }
    
    #[inline(always)]
    pub fn upload(&mut self, device: &RenderDevice) {
        if self.commands.is_empty() {
            return;
        }
        let bytes = bytemuck::cast_slice(&self.commands);
        let buffer = device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("Frame IndirectBuffer"),
            contents: bytes,
            usage: BufferUsages::INDIRECT | BufferUsages::COPY_DST,
        });

        self.gpu_buffer = Some(buffer);
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default, Pod, Zeroable)]
pub struct InstanceMetaBuffer {
    pub chunk_index: u32,
    pub face_id: u32, 
}
impl InstanceMetaBuffer {
    #[inline(always)]
    pub const fn default() -> Self {
        Self{chunk_index: 0,face_id: 0}
    }

    #[inline(always)]
    pub fn to_bytes(&self) -> &[u8] {
        bytes_of(self)
    }
}


// =============================================================================
// SHARED GLOBAL RESOURCES - THE ACTUAL GPU BUFFER + ALLOCATOR
// =============================================================================

#[derive(Resource, Clone, ExtractResource)]
pub struct InstancedMeta {
    pub buffer: Option<Buffer>,
    pub buffer_size: u32,
}
impl FromWorld for InstancedMeta {
    fn from_world(_world: &mut World) -> Self {
        // Use a reasonable default size instead of 0
        let default_size = buffer_config::DEFAULT_INSTANCE_BUFFER_CAPACITY * std::mem::size_of::<InstanceMetaBuffer>();
        Self::new(default_size as u32)
    }
}

impl InstancedMeta {
    #[inline(always)]
    pub fn new(buffer_size: u32) -> Self {
        Self {
            buffer: None,
            buffer_size,
        }
    }

    #[inline(always)]
    pub fn ensure_created(&mut self, device: &RenderDevice) {
        if self.buffer.is_none() {
            self.buffer = Some(device.create_buffer(&BufferDescriptor {
                label: Some("Indirect Meta Buffer"),
                size: self.buffer_size as u64,
                usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
    }

    #[inline(always)]
    pub fn write_at_index(&self, queue: &RenderQueue, index: usize, data: &[InstanceMetaBuffer]) {
        if let Some(buffer) = &self.buffer {
            let offset = (index * std::mem::size_of::<(u32, u32)>()) as u64;
            let bytes = bytemuck::cast_slice(data); // ✅ correct for slice
            queue.write_buffer(buffer, offset, bytes);
        }
    }

}

#[derive(Deref,DerefMut, Clone, Resource, ExtractResource)]
pub struct VoxelInstancePool(pub GlobalBufferPool);

impl VoxelInstancePool {
    #[inline(always)]
    pub fn new(buffer_size: u32, usage: BufferUsages) -> Self {
        Self(GlobalBufferPool::new(buffer_size,usage))
    }
}

impl FromWorld for VoxelInstancePool {
    fn from_world(_world: &mut World) -> Self {
        // Define a reasonable default buffer size
        VoxelInstancePool::new(buffer_config::DEFAULT_INSTANCE_BUFFER_CAPACITY as u32,BufferUsages::VERTEX | BufferUsages::COPY_DST)
    }
}

#[derive(Resource, Clone, Copy, Default, ExtractResource)]
pub struct FrameCounter(pub u32);


impl FrameCounter {
    #[inline(always)]
    pub const fn new()-> Self {
        Self(0)
    }
    #[inline(always)]
    pub const fn current_frame(&self)-> u32{
        self.0
    }
    #[inline(always)]
    pub const fn advance_frame(&mut self) {
        self.0+=1
    }
}

#[derive(Clone)]
pub struct GlobalBufferPool {
    pub buffer: Option<Buffer>,
    pub range_allocator: RangeAllocator,
    pub buffer_size: u32,
    pub usage: BufferUsages,
}


impl GlobalBufferPool {
    #[inline(always)]
    pub fn new(buffer_size: u32, usage: BufferUsages) -> Self {
        Self {
            buffer: None,
            range_allocator: RangeAllocator::new(buffer_size),
            buffer_size,
            usage,
        }
    }

    #[inline(always)]
    pub fn ensure_created(&mut self, device: &RenderDevice) {
        if self.buffer.is_none() {
            self.buffer = Some(device.create_buffer(&BufferDescriptor {
                label: Some("Shared Instance Buffer"),
                size: self.buffer_size as u64,
                usage: self.usage,
                mapped_at_creation: false,
            }));
        }
    }

    // ✅ Thread-safe allocation (could add Mutex if needed for parallel access)
    #[inline(always)]
    pub fn allocate_range(&mut self, size: u32) -> Option<BufferRange> {
        self.range_allocator.allocate(size)
    }

    // ✅ Thread-safe deallocation
    #[inline(always)]
    pub fn deallocate_range(&mut self, range: BufferRange) {
        self.range_allocator.deallocate(range);
    }

    #[inline(always)]
    pub fn write_data<T: Pod>(&self, queue: &RenderQueue, range: &BufferRange, data: &[T]) {
        if let Some(buffer) = &self.buffer {
            let bytes = bytemuck::cast_slice(data);
            queue.write_buffer(buffer, range.offset as u64, bytes);
        }
    }
}

// =============================================================================
// PER-CHUNK BUFFER COMPONENTS - EACH CHUNK MANAGES ITS OWN STATE
// =============================================================================

#[derive(Component, ExtractComponent, Clone, Copy)]
pub struct Dirty;


#[derive(Component, Clone, ExtractComponent)]
pub struct ChunkMeshRange {
    // ✅ Per-chunk state management
    staged_data: Vec<VoxelInstance>,
    
    // Triple-buffered range data
    current_range: Option<BufferRange>,
    next_range: Option<BufferRange>,
    cleanup_queue: VecDeque<PendingFaceCleanup>,

    
    // ✅ Triple-buffered face data too!
    current_face_ranges: [u32; 6],    // Current frame's face sizes
    current_face_offsets: [u32; 6],   // Current frame's face offsets
    next_face_ranges: [u32; 6],       // Next frame's face sizes  
    next_face_offsets: [u32; 6],    

    
    // Metadata
    frame_allocated: u32,
    is_uploaded: bool,
}


impl ChunkMeshRange {
    #[inline(always)]
    pub const fn default() -> Self {
        let def_arr = [0;6];
        Self {
            staged_data: Vec::new(),
            current_range: None,
            next_range: None,
            current_face_ranges: def_arr,
            current_face_offsets: def_arr,
            next_face_ranges: def_arr,
            next_face_offsets: def_arr,
            cleanup_queue: VecDeque::new(),
            frame_allocated: 0,
            is_uploaded: false,
        }
    }
    #[inline(always)]
    pub fn stage_empty(&mut self) {
        self.staged_data.clear();
        self.next_range = None;
        self.next_face_ranges = [0; 6];
        self.next_face_offsets = [0; 6];
        self.is_uploaded = false;
    }

    #[inline(always)]
    pub fn stage_face_data(&mut self, faces: &[Vec<VoxelInstance>; 6]) {
        // Prepare next frame's layout
        self.next_face_ranges = [0; 6];
        self.staged_data.clear();
        
        let mut offset = 0;
        for (i, face_data) in faces.iter().enumerate() {
            self.next_face_ranges[i] = face_data.len() as u32;
            self.next_face_offsets[i] = offset;
            offset += face_data.len() as u32;
            
            self.staged_data.extend_from_slice(face_data);
        }
    }


    // ✅ Request allocation from global pool and upload
    pub fn upload(
        &mut self, 
        global_pool: &mut GlobalBufferPool,
        render_queue: &RenderQueue,
        current_frame: u32,
    ) -> bool {
        if self.staged_data.is_empty() {
            return false;
        }

        let element_size = std::mem::size_of::<VoxelInstance>() as u32;
        let total_size = self.staged_data.len() as u32 * element_size;

        // ✅ Request range from shared allocator
        if let Some(new_range) = global_pool.allocate_range(total_size) {
            // ✅ Upload data to shared buffer at allocated range
            global_pool.write_data(render_queue, &new_range, &self.staged_data);
            
            self.next_range = Some(new_range);
            self.frame_allocated = current_frame;
            self.staged_data.clear();
            self.is_uploaded = true;
            
            return self.try_swap(current_frame)
        }
        
        false // Not enough space
        
    }

    // ✅ Triple-buffered swap (like C# TrySwap)
    pub fn try_swap(&mut self, current_frame: u32) -> bool {
        if let Some(next) = self.next_range.take() {
            // Swap BOTH allocation AND face layout atomically
            if let Some(old) = self.current_range.take() {
                self.cleanup_queue.push_back(PendingFaceCleanup {
                    range: old,
                    face_ranges: self.current_face_ranges,
                    face_offsets: self.current_face_offsets,
                    frame_index: current_frame + 3, // Safe delay for GPU to finish
                });
            }
            
            self.current_range = Some(next);
            self.current_face_ranges = self.next_face_ranges; // Swap face layout too!
            self.current_face_offsets = self.next_face_offsets;
            
            true
        } else {
            false
        }
    }

    // ✅ Cleanup old ranges (like C# CheckOldSyncs)
    pub fn cleanup_old_ranges(&mut self, global_pool: &mut GlobalBufferPool, current_frame: u32) {
        while let Some(cleanup) = self.cleanup_queue.front() {
            // Check if the frame has advanced far enough
            if current_frame >= cleanup.frame_index {
                // Safe to deallocate
                let cleanup = self.cleanup_queue.pop_front().unwrap();
                global_pool.deallocate_range(cleanup.range);
            } else {
                break; // Front item is too new — stop checking
            }
        }
    }

    #[inline(always)]
    pub fn draw_buffer(&self) -> IndirectDrawBuffer {
        let mut commands = IndirectDrawBuffer::default();

        if let Some(range) = &self.current_range {
            let element_size = std::mem::size_of::<VoxelInstance>() as u32;
            let base_instance = range.offset / element_size;
            
            for (face_idx, &face_size) in self.current_face_ranges.iter().enumerate() {
                if face_size > 0 {
                    let first_instance = base_instance + self.current_face_offsets[face_idx];
                    commands.push(
                        DrawIndirectCommand {
                            vertex_count: 4,
                            instance_count: face_size,
                            first_vertex: 0,
                            first_instance,
                        },
                    );
                }
            }
        }
        commands
    }

    #[inline(always)]
    pub fn face_metas(&self, index: ChunkIndex) -> Option<(u32, Vec<InstanceMetaBuffer>)> {
        let chunk_id = index.0;

        let base_instance = self.base_instance()?;


        let mut meta = Vec::with_capacity(self.element_count() as usize);

        for (face_idx, face_size) in self.current_face_ranges.iter().enumerate() {
            for _ in 0..*face_size {
                meta.push(InstanceMetaBuffer { chunk_index: chunk_id, face_id: face_idx as u32 });
            }
        }

        Some((base_instance, meta))
    }

    #[inline(always)]
    pub fn base_instance(&self) -> Option<u32> {
        self.current_range.as_ref().map(|range| {
            let element_size = std::mem::size_of::<VoxelInstance>() as u32;
            range.offset / element_size
        })
    }

    #[inline(always)]
    pub fn has_active_data(&self) -> bool {
        self.current_range.is_some()
    }

    #[inline(always)]
    pub fn element_count(&self) -> u32 {
        if let Some(range) = &self.current_range {
            let element_size = std::mem::size_of::<VoxelInstance>() as u32;
            range.size / element_size
        } else {0}
    }
}

// =============================================================================
// PER-DRAW-CALL BUFFER COMPONENT - SSBO INDEXED BY BASE_INSTANCE
// =============================================================================

// =============================================================================
// SUPPORTING TYPES
// =============================================================================

#[derive(Debug, Clone, Copy)]
pub struct BufferRange {
    pub offset: u32,
    pub size: u32,
}

#[derive(Debug, Clone)]
pub struct PendingFaceCleanup {
    pub range: BufferRange,
    pub face_ranges: [u32; 6],     // ✅ Face info needs cleanup too
    pub face_offsets: [u32; 6],
    pub frame_index: u32,
}

#[derive(Clone)]
pub struct RangeAllocator {
    _buffer_size: u32,
    free_ranges: Vec<BufferRange>,
}

impl RangeAllocator {
    #[inline(always)]
    pub fn new(_buffer_size: u32) -> Self {
        Self {
            _buffer_size,
            free_ranges: vec![BufferRange { offset: 0, size: _buffer_size }],
        }
    }

    pub fn allocate(&mut self, size: u32) -> Option<BufferRange> {
        let aligned_size = (size + 255) & !255; // 256-byte alignment
        
        for i in 0..self.free_ranges.len() {
            if self.free_ranges[i].size >= aligned_size {
                let range = self.free_ranges.remove(i);
                
                if range.size > aligned_size {
                    self.free_ranges.push(BufferRange {
                        offset: range.offset + aligned_size,
                        size: range.size - aligned_size,
                    });
                }
                
                return Some(BufferRange {
                    offset: range.offset,
                    size: aligned_size,
                });
            }
        }
        None
    }

    pub fn deallocate(&mut self, range: BufferRange) {
        self.free_ranges.push(range);
        self.merge_adjacent();
    }

    fn merge_adjacent(&mut self) {
        self.free_ranges.sort_by_key(|r| r.offset);
        
        let mut i = 0;
        while i < self.free_ranges.len().saturating_sub(1) {
            let current = self.free_ranges[i];
            let next = self.free_ranges[i + 1];
            
            if current.offset + current.size == next.offset {
                self.free_ranges[i] = BufferRange {
                    offset: current.offset,
                    size: current.size + next.size,
                };
                self.free_ranges.remove(i + 1);
            } else {
                i += 1;
            }
        }
    }
}


#[derive(Resource, Clone)]
pub struct ExtractedGpuBuffers {
    pub instance_buffer: Option<Buffer>,      // Handle to GPU buffer
    pub ssbo_buffer: Option<Buffer>,          // Handle to SSBO buffer
}

