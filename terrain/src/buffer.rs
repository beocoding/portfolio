use std::collections::VecDeque;

use bevy::{ecs::{component::Component, resource::Resource}, prelude::{Deref, DerefMut}, render::{extract_component::ExtractComponent, render_resource::{Buffer, BufferDescriptor, BufferUsages}, renderer::{RenderDevice, RenderQueue}}};
use bytemuck::{cast_slice, Pod};
use crate::noise::{TerrainMeshlet, TerrainVertex};

#[derive(Clone, Debug)]
pub struct AllocatedBuffer {
    pub buffer: Option<Buffer>,
    pub range_allocator: RangeAllocator,
    pub buffer_size: u32,
    pub usage: BufferUsages,
}

impl AllocatedBuffer {
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
    pub fn write_buffer<T: Pod>(&self, queue: &RenderQueue, range: &BufferRange, data: &[T]) {
        if let Some(buffer) = &self.buffer {
            let bytes = cast_slice(data);
            let end = (range.offset as usize).saturating_add(bytes.len());
            let buf_size = self.buffer_size as usize;
            if end > buf_size {
                // Defensive: log and skip the write (to avoid invalid write that would create undefined GPU state)
                println!(
                    "  ❌ write_buffer out-of-bounds: offset={} + len={} > buffer_size={}",
                    range.offset, bytes.len(), self.buffer_size
                );
                return;
            }
            queue.write_buffer(buffer, range.offset as u64, bytes);
        } else {
            println!("  ❌ write_buffer called but buffer is None");
        }
    }
}


#[derive(Debug, Clone, Copy)]
pub struct BufferRange {
    pub offset: u32,
    pub size: u32,
}


#[derive(Clone, Debug,)]
pub struct RangeAllocator {
    pub free_ranges: Vec<BufferRange>,
}

impl RangeAllocator {
    #[inline(always)]
    pub fn new(buffer_size: u32) -> Self {
        Self {
            free_ranges: vec![BufferRange { offset: 0, size: buffer_size }],
        }
    }

    /// Allocate `size` bytes. Returns None if `size == 0` or no space.
    pub fn allocate(&mut self, size: u32) -> Option<BufferRange> {
        // refuse zero-size allocations — they lead to zero-element draws and subtle errors
        if size == 0 {
            return None;
        }

        // Use 4-byte alignment for vertex data (compatible with Vulkan/Metal/DirectX).
        // If you need 256-byte alignment for other kinds of buffers, make that configurable.
        let align = 4u32;
        let aligned_size = ((size + align - 1) / align) * align;

        // find first-fit range
        for i in 0..self.free_ranges.len() {
            if self.free_ranges[i].size >= aligned_size {
                let range = self.free_ranges.remove(i);
                if range.size > aligned_size {
                    // leave remainder
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

// Marker trait bound — ensures we can safely reinterpret `T` as bytes
pub trait GpuUploadable: Pod {}

impl<T: Pod> GpuUploadable for T {}

#[derive(Clone,Debug, Default)]
pub struct StagedData<T: GpuUploadable> {
    // Per-chunk state
    pub(crate) staged_data: Vec<T>,
    
    pub(crate)current_range: Option<BufferRange>,
    next_range: Option<BufferRange>,
    cleanup_queue: VecDeque<PendingCleanUpRange<T>>,

    frame_allocated: u32,
    is_uploaded: bool,
}

impl StagedData<TerrainVertex> {
    #[inline(always)]
    pub fn from_meshlet(meshlet: TerrainMeshlet) -> Self {
        Self {
            staged_data: meshlet.0,
            current_range: None,
            next_range: None,
            cleanup_queue: VecDeque::new(),
            frame_allocated: 0,
            is_uploaded: false,
        }
    }
}


impl<T: GpuUploadable> StagedData<T> {
    #[inline(always)]
    pub fn new() -> Self {
        Self {
            staged_data: Vec::new(),
            current_range: None,
            next_range: None,
            cleanup_queue: VecDeque::new(),
            frame_allocated: 0,
            is_uploaded: false,
        }
    }
    #[inline(always)]
    pub fn default() -> Self {
        Self::new()
    }

    #[inline(always)]
    pub fn stage_empty(&mut self) {
        self.staged_data.clear();
        self.next_range = None;
        self.is_uploaded = false;
    }

    #[inline(always)]
    pub fn stage_data(&mut self, data: &[T]) {
        self.staged_data.clear();
        self.staged_data.extend_from_slice(data);
        self.is_uploaded = false;
    }

    #[inline(always)]
    pub fn try_swap(&mut self, current_frame: u32) -> bool {
        if let Some(next) = self.next_range.take() {
            if let Some(old) = self.current_range.take() {
                self.cleanup_queue.push_back(PendingCleanUpRange {
                    range: old,
                    frame_index: current_frame + 3,
                    _marker: std::marker::PhantomData,
                });
            }
            
            self.current_range = Some(next);
            true
        } else {
            false
        }
    }
    
    pub fn upload(
        &mut self, 
        global_pool: &mut AllocatedBuffer,
        render_queue: &RenderQueue,
        current_frame: u32,
    ) -> bool {
        if self.is_uploaded {
            return false;
        }

        let element_size = std::mem::size_of::<T>() as u32;
        let total_size = (self.staged_data.len() as u32).saturating_mul(element_size);

        // refuse zero-size uploads
        if total_size == 0 {
            // nothing to upload
            if cfg!(debug_assertions) {
                println!("  ❗ upload requested with total_size == 0; skipping upload");
            }
            self.is_uploaded = false;
            return false;
        }

        if let Some(new_range) = global_pool.allocate_range(total_size) {
            // Validate that allocator gave us at least the space we expect
            if new_range.size < total_size {
                // Shouldn't happen because allocator aligns up, but be defensive
                println!(
                    "  ❌ Allocated range too small: allocated={}, needed={}",
                    new_range.size, total_size
                );
                global_pool.deallocate_range(new_range);
                return false;
            }

            // Ensure the bytes we write match the expected size
            let bytes:&[u8] = bytemuck::cast_slice(&self.staged_data);
            if bytes.len() as u32 != total_size {
                println!(
                    "  ❌ unexpected bytes length mismatch: bytes.len()={}, total_size={}",
                    bytes.len(),
                    total_size
                );
                // cleanup and bail
                global_pool.deallocate_range(new_range);
                return false;
            }

            // perform write and install next_range
            global_pool.write_buffer(render_queue, &new_range, &self.staged_data);
            self.next_range = Some(new_range);
            self.frame_allocated = current_frame;
            self.staged_data.clear();
            self.is_uploaded = true;
            return true
        }
        false
    }

    #[inline(always)]
    pub fn cleanup_old_ranges(&mut self, global_pool: &mut AllocatedBuffer, current_frame: u32) {
        while let Some(cleanup) = self.cleanup_queue.front() {
            if current_frame >= cleanup.frame_index {
                let cleanup = self.cleanup_queue.pop_front().unwrap();
                global_pool.deallocate_range(cleanup.range);
            } else {
                break;
            }
        }
    }

    #[inline(always)]
    pub fn flush(&mut self, global_pool: &mut AllocatedBuffer, render_queue: &RenderQueue, current_frame: u32) -> bool {        
        if self.upload(global_pool, render_queue, current_frame) {
            return self.try_swap(current_frame)
        }
        false
    }

    /// Returns the GPU buffer range for the current data
    #[inline(always)]
    pub fn current_range(&self) -> Option<BufferRange> {
        self.current_range
    }

    /// Returns (start_index, count) for indexed drawing
    #[inline(always)]
    pub fn draw_range(&self) -> Option<(u32, u32)> {
        self.current_range.map(|range| {
            let element_size = std::mem::size_of::<T>() as u32;
            let start = range.offset / element_size;
            let count = range.size / element_size;
            (start, count)
        })
    }
    
}


// ✅ Generic cleanup payload
#[derive(Debug,Clone, Copy)]
pub struct PendingCleanUpRange<T> {
    pub range: BufferRange,
    pub frame_index: u32,
    _marker: std::marker::PhantomData<T>,
}

// Render World
#[derive(Component, Debug, Default, Deref, DerefMut, ExtractComponent, Clone)]
pub struct TerrainMeshData(pub StagedData<TerrainVertex>);

impl TerrainMeshData {
    #[inline(always)]
    pub fn new(meshlet: TerrainMeshlet) -> Self {
        TerrainMeshData(StagedData::from_meshlet(meshlet))
    }
}

#[derive(Resource, Debug, Deref, DerefMut)]
pub struct TerrainMeshGpuBuffer(pub AllocatedBuffer);

impl Default for TerrainMeshGpuBuffer {
    fn default() -> Self {
        Self::new(64*1024*1024) //64 MB
    }
}

impl TerrainMeshGpuBuffer {
    #[inline(always)]
    pub fn new(buffer_size: u32) -> Self {
        Self (AllocatedBuffer{
            buffer: None,
            range_allocator: RangeAllocator::new(buffer_size),
            buffer_size,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
        })
    }

    #[inline(always)]
    pub fn ensure_created(&mut self, device: &RenderDevice) {
        if self.buffer.is_none() {
            self.buffer = Some(device.create_buffer(&BufferDescriptor {
                label: Some("TerrainVertexBuffer"),
                size: self.buffer_size as u64,
                usage: self.usage,
                mapped_at_creation: false,
            }));
        }
    }
}

