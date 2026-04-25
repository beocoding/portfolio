// /fluffr/flatr_core/src/lib.rs
pub mod serialize;
pub mod read;
pub mod buffer;
use std::borrow::Cow;
use bytemuck::{Pod, Zeroable};
pub mod verify;
pub use verify::*;
pub mod merge;
pub use merge::*;
use crate::{buffer::Buffer, read::ReadAt};
pub mod filter;
pub use filter::*;

pub mod file_kind;
pub use file_kind::*;
pub trait Query<'a> {
    type QueryType;
    type Key: ReadAt<'a>;        // fixed per implementor, not per call

    fn query(&self, query: Self::QueryType) -> BitMask;
    fn query_by_key(&self, val: <Self::Key as ReadAt<'a>>::ReadOutput) -> Option<usize>;
    fn query_by_keys(&self, vals: &[<Self::Key as ReadAt<'a>>::ReadOutput],) -> BitMask;
}
pub trait QueryType {
    fn new()-> Self;
}
#[repr(u8)]
pub enum DataType { Inline, Offset, Union }

impl DataType {
    #[inline(always)]
    pub fn is_inline_flag(&self) -> bool {
        matches!(self,DataType::Inline)
    }
    #[inline(always)]
    pub fn is_offset_flag(&self) -> bool {
        match self {
            DataType::Offset | DataType::Union => true,
            _ => false
        }
    }
    #[inline(always)]
    pub fn is_union_flag(&self) -> bool {
        matches!(self,DataType::Union)
    }
}

pub trait Table: Default {
    type VTableTemplate;
    const VTABLE_TEMPLATE: Self::VTableTemplate;
    type View<'a>;

    fn view<'a>(buf: &'a[u8], table_idx: usize) -> Self::View<'a>;
    fn as_buffer(&self) -> impl Buffer;
}
pub trait Flat: Pod + Zeroable{
    fn to_le_bytes(&self) -> Cow<'_, [u8]>;
    fn from_le_bytes(bytes:&[u8]) -> Self;
}
// Row trait (already exists from previous work — no change needed)
pub trait Row: Table {
    type Registry;
    fn write_as_registry<B: Buffer>(&self, buf: &mut B) -> usize;
}

// New: RegistryView trait
pub trait RegistryView<'a> {
    type RowRef;
    fn get_row(&self, i: usize) -> Self::RowRef;
    fn len(&self) -> usize;
    #[inline(always)]
    fn is_empty(&self) -> bool{
        self.len() == 0
    }
}
#[inline(always)]
pub const fn bitmax(a: usize, b: usize) -> usize {
    // Force the condition to a usize (0 or 1)
    let condition = (a < b) as usize;
    
    // Create the mask (0 -> 0x00...0, 1 -> 0xFF...F)
    let mask = 0usize.wrapping_sub(condition);
    
    a ^ ((a ^ b) & mask)
}
