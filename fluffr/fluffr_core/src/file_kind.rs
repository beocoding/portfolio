// fluffr/flatr_core/src/file_kind.rs
use std::{fmt, marker::PhantomData};

use crate::{DataType, Verify, VerifyError, VerifyResult, buffer::Buffer, check_bounds, read::ReadAt, serialize::Serialize};

pub trait FileKind {
    const MIME: &'static str;
}
#[derive(Default, Copy, Clone, PartialEq, Eq, Debug)]
pub struct Jpeg;
impl FileKind for Jpeg {
    const MIME: &'static str = "image/jpeg";
}
#[derive(Default, Copy, Clone, PartialEq, Eq, Debug)]
pub struct Png;
impl FileKind for Png {
    const MIME: &'static str = "image/png";
}
#[derive(Default, Copy, Clone, PartialEq, Eq, Debug)]
pub struct Gif;
impl FileKind for Gif {
    const MIME: &'static str = "image/gif";
}
#[derive(Default, Copy, Clone, PartialEq, Eq, Debug)]
pub struct WebP;
impl FileKind for WebP {
    const MIME: &'static str = "image/webp";
}
#[derive(Default, Copy, Clone, PartialEq, Eq, Debug)]
pub struct Pdf;
impl FileKind for Pdf {
    const MIME: &'static str = "application/pdf";
}
#[derive(Default, Copy, Clone, PartialEq, Eq, Debug)]
pub struct Json;
impl FileKind for Json {
    const MIME: &'static str = "application/json";
}


pub struct FileBlob<T: FileKind> {
    pub data: Vec<u8>,
    _ext: PhantomData<T>,
}
impl <T: FileKind> FileBlob<T> {
    pub const EMPTY: FileBlob<T> = FileBlob {
        data: Vec::new(), _ext: PhantomData,
    };
}
impl<T: FileKind> fmt::Debug for FileBlob<T> {

    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let full = std::any::type_name::<T>();
        let short = full.rsplit("::").next().unwrap_or(full);

        write!(
            f,
            "FileBlob{{ size: {} bytes, file type: {} }}",
            self.data.len(),
            short
        )
    }
}
impl<T: FileKind> Clone for FileBlob<T> {
    fn clone(&self) -> Self {
        Self { data: self.data.clone(), _ext: PhantomData }
    }
}
impl<T: FileKind> Default for FileBlob<T> {
    fn default() -> Self {
        Self { data: Vec::default(), _ext: PhantomData }
    }
}
#[derive(Clone, Copy)]
pub struct FileBlobView<'a, T: FileKind> {
    pub data: &'a [u8],
    _ext: PhantomData<T>,
}

impl<'a,T: FileKind> fmt::Debug for FileBlobView<'a,T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let full = std::any::type_name::<T>();
        let short = full.rsplit("::").next().unwrap_or(full);

        write!(
            f,
            "FileBlobView{{ size: {} bytes, file type: {} }}",
            self.data.len(),
            short
        )
    }
}
impl<'a, T: FileKind> FileBlobView<'a, T> {
    pub const EMPTY: FileBlobView<'static, T> = FileBlobView {
        data: &[], _ext: PhantomData,
    };
    pub fn mime() -> &'static str { T::MIME }
    pub fn len(&self)     -> usize { self.data.len() }
    pub fn is_empty(&self) -> bool { self.data.is_empty() }
    pub fn as_bytes(&self) -> &'a [u8] { self.data }
}
impl<'a, T: FileKind> Default for FileBlobView<'a, T> {
    fn default() -> Self {
        Self { data: &[], _ext: PhantomData }
    }
}
impl<T: FileKind> FileBlob<T> {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data, _ext: PhantomData }
    }
    pub fn is_empty(&self) -> bool { self.data.is_empty() }
    pub fn mime() -> &'static str { T::MIME }
    pub fn len(&self) -> usize {
        self.data.len()
    }
}



impl<'a,T: FileKind> Serialize for FileBlobView<'a,T> {
    const SIZE: usize = 4;
    const ALIGN: usize = 4;
    const MODE: DataType = DataType::Offset;

    #[inline(always)]
    fn size_hint(&self) -> usize {
        self.data.len() + 4 + Self::ALIGNR
    }

    #[inline(always)]
    fn write_to<B: Buffer>(&self, buffer: &mut B) -> usize {
        buffer.ensure_capacity(self.size_hint());
        self.write_to_unchecked(buffer)
    }

    #[inline(always)]
    fn write_to_unchecked<B: Buffer>(&self, buffer: &mut B) -> usize {
        let len = self.data.len();
        *buffer.head_mut() -= len + 4;
        *buffer.head_mut() &= Self::ALIGN_MASK;
        let head = buffer.head();
        let head2 = head+4;
        buffer.buffer_mut()[head..head2].copy_from_slice(&(len as u32).to_le_bytes());
        buffer.buffer_mut()[head2..head2+len].copy_from_slice(&self.data);
        buffer.slot()
    }

    #[inline(always)]
    fn is_absent(&self) -> bool {
        self.data.is_empty()
    }
}


impl<T: FileKind> Serialize for FileBlob< T> {
    const SIZE: usize = 4;
    const ALIGN: usize = 4;
    const MODE: DataType = DataType::Offset;

    #[inline(always)]
    fn size_hint(&self) -> usize {
        self.data.len() + 4 + Self::ALIGNR
    }

    #[inline(always)]
    fn write_to<B: Buffer>(&self, buffer: &mut B) -> usize {
        buffer.ensure_capacity(self.size_hint());
        self.write_to_unchecked(buffer)
    }

    #[inline(always)]
    fn write_to_unchecked<B: Buffer>(&self, buffer: &mut B) -> usize {
        let len = self.data.len();
        *buffer.head_mut() -= len + 4;
        *buffer.head_mut() &= Self::ALIGN_MASK;
        let head = buffer.head();
        let head2 = head+4;
        buffer.buffer_mut()[head..head2].copy_from_slice(&(len as u32).to_le_bytes());
        buffer.buffer_mut()[head2..head2+len].copy_from_slice(&self.data);
        buffer.slot()
    }

    #[inline(always)]
    fn is_absent(&self) -> bool {
        self.data.is_empty()
    }
}

impl<'a, T: FileKind> ReadAt<'a> for FileBlob<T> {
    
    const MODE: DataType = DataType::Offset;
    type ReadOutput = FileBlobView<'a, T>;

    #[inline(always)]
    fn read_at(buf: &'a [u8], offset: usize) -> FileBlobView<'a, T> {
        let len = u32::read_at(buf, offset) as usize;
        FileBlobView { data: &buf[offset + 4..offset + 4 + len], _ext: PhantomData }
    }

    #[inline(always)]
    fn default_output() -> FileBlobView<'a, T> { FileBlobView::EMPTY }

    #[inline(always)]
    fn payload_block_end(buf: &'a [u8], pos: usize) -> usize {
        let len = u32::read_at(buf, pos) as usize;
        pos + 4 + len
    }
}

impl<'a, T: FileKind> PartialEq<FileBlobView<'a, T>> for FileBlob<T> {
    fn eq(&self, other: &FileBlobView<'a, T>) -> bool {
        self.data.as_slice() == other.data
    }
}

impl<'a, T: FileKind> PartialEq<FileBlob<T>> for FileBlobView<'a, T> {
    fn eq(&self, other: &FileBlob<T>) -> bool {
        self.data == other.data.as_slice()
    }
}

// FileBlobView ↔ FileBlobView
impl<'a, T: FileKind> PartialEq for FileBlobView<'a, T> {
    fn eq(&self, other: &Self) -> bool {
        self.data == other.data
    }
}

impl<T: FileKind> PartialEq for FileBlob<T> {
    fn eq(&self, other: &Self) -> bool {
        self.data == other.data
    }
}


impl<T:FileKind> Verify for FileBlob<T> {
    const INLINE_SIZE: usize = 4;
    #[inline]
    fn verify_at(buf: &[u8], offset: usize, _depth: usize, _out: &mut Vec<usize>) -> VerifyResult {
        check_bounds(buf, offset, 4, "File length prefix")?;
        let len = u32::read_at(buf, offset) as usize;
        check_bounds(buf, offset + 4, len, "file bytes")?;
        std::str::from_utf8(&buf[offset + 4..offset + 4 + len])
            .map_err(|_| VerifyError::InvalidUtf8 { at: offset + 4 })?;
        Ok(())
    }
}