use std::{array::TryFromSliceError, cell::Cell, fs::File, os::unix::fs::FileTypeExt, path::Path};

use binary_struct::{BinaryParse, BinaryType};
use memmap2::{Mmap, MmapOptions};

pub const SECTOR_SIZE: usize = 512;

#[derive(Debug, thiserror::Error)]
pub enum MappedDiskError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Out of bounds access at offset {0}")]
    OutOfBounds(usize),
    #[error("TryFromSlice: {0}")]
    TryFromSlice(#[from] TryFromSliceError),
}

pub type MappedDiskResult<T> = Result<T, MappedDiskError>;

pub struct MappedDisk {
    mmap: Mmap,
    cursor: Cell<usize>,
}

#[cfg(unix)]
fn get_block_device_size(file: &File) -> std::io::Result<u64> {
    use std::{io, os::unix::io::AsRawFd};

    const BLKGETSIZE64: u64 = 0x80081272;
    let mut size: u64 = 0;

    let ret = unsafe { libc::ioctl(file.as_raw_fd(), BLKGETSIZE64, &mut size) };

    if ret < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(size)
    }
}

impl MappedDisk {
    pub fn new<P: AsRef<Path>>(path: P) -> MappedDiskResult<Self> {
        let file = File::open(path)?;
        let metadata = file.metadata()?;
        let mut size = metadata.len();
        if metadata.file_type().is_block_device() {
            size = get_block_device_size(&file)?;
        }
        let mmap = unsafe { MmapOptions::new().len(size as usize).map(&file)? };
        Ok(Self {
            mmap,
            cursor: Cell::new(0),
        })
    }

    /// Read `size` bytes starting from `offset` without advancing the cursor
    pub fn read_bytes_at(&self, offset: usize, size: usize) -> MappedDiskResult<&[u8]> {
        if offset + size > self.mmap.len() {
            Err(MappedDiskError::OutOfBounds(offset))
        } else {
            Ok(&self.mmap[offset..offset + size])
        }
    }

    /// Read `num_sectors` starting from `offset_lba` without advancing the cursor.
    /// Parameters are expected to be LBAs
    pub fn read_bytes_starting_at_sector(
        &self,
        offset_lba: usize,
        num_bytes: usize,
    ) -> MappedDiskResult<&[u8]> {
        let offset = offset_lba * SECTOR_SIZE;
        self.read_bytes_at(offset, num_bytes)
    }

    /// Read `num_sectors` starting from `offset_lba` without advancing the cursor.
    /// Parameters are expected to be LBAs
    pub fn read_sectors_at(
        &self,
        offset_lba: usize,
        num_sectors: usize,
    ) -> MappedDiskResult<&[u8]> {
        let offset = offset_lba * SECTOR_SIZE;
        let size = num_sectors * SECTOR_SIZE;
        self.read_bytes_at(offset, size)
    }

    /// Read bytes and interpret them as `T` starting from `offset`.
    /// This function does not start at or advance the cursor
    fn read_at<T>(&self, offset: usize) -> MappedDiskResult<T>
    where
        T: BinaryType,
    {
        let bytes = self.read_bytes_at(offset, T::SIZE)?;
        T::parse(bytes).map_err(|_| MappedDiskError::OutOfBounds(offset))
    }

    /// Read `size` bytes starting from the current cursor location
    /// This function advances the cursor after a read
    pub fn read_bytes(&self, size: usize) -> MappedDiskResult<&[u8]> {
        let offset = self.cursor.get();
        self.read_bytes_at(offset, size).map(|bytes| {
            self.cursor.set(offset + size);
            bytes
        })
    }

    pub fn read_byte(&self) -> MappedDiskResult<u8> {
        let offset = self.cursor.get();
        if offset > self.mmap.len() {
            Err(MappedDiskError::OutOfBounds(offset))
        } else {
            self.cursor.set(offset + 1);
            Ok(self.mmap[offset])
        }
    }

    pub fn peek_byte(&self) -> MappedDiskResult<u8> {
        let offset = self.cursor.get();
        if offset > self.mmap.len() {
            Err(MappedDiskError::OutOfBounds(offset))
        } else {
            Ok(self.mmap[offset])
        }
    }

    pub fn read_with_size<T: BinaryParse>(&self, size: usize) -> MappedDiskResult<T> {
        let bytes = self.read_bytes(size)?;
        T::parse(bytes).map_err(|_| MappedDiskError::OutOfBounds(self.cursor.get()))
    }

    pub fn read_string_utf8(&self, size: usize) -> MappedDiskResult<String> {
        let bytes = self.read_bytes(size)?;
        Ok(String::from_utf8(bytes.to_vec()).unwrap())
    }

    pub fn read_string_utf16(&self, size: usize) -> MappedDiskResult<String> {
        let bytes = self.read_bytes(size)?;
        let u16_iter = bytes.chunks_exact(2)
                            .map(|b| u16::from_le_bytes([b[0], b[1]]));
        Ok(String::from_utf16(&u16_iter.collect::<Vec<_>>()).unwrap())
    }
    
    /// Read `size_of<T>()` bytes starting from the current cursor location
    pub fn read<T: BinaryType>(&self) -> MappedDiskResult<T> {
        // if let Some(record_size) = f

        let bytes = self.read_bytes(T::SIZE)?;
        T::parse(bytes).map_err(|_| MappedDiskError::OutOfBounds(self.cursor.get()))
    }

    /// Read `size_of<T>()` bytes starting from the current cursor location without advancing the cursor
    pub fn peek<T: BinaryType>(&self) -> MappedDiskResult<T> {
        let offset = self.cursor.get();
        self.read_at::<T>(offset)
    }

    /// Set cursor location in bytes
    pub fn set_cursor(&self, offset: usize) -> MappedDiskResult<()> {
        if offset > self.mmap.len() {
            Err(MappedDiskError::OutOfBounds(offset))
        } else {
            self.cursor.set(offset);
            Ok(())
        }
    }

    /// Set cursor location from the LBA
    pub fn set_cursor_from_lba(&self, lba: usize) -> MappedDiskResult<()> {
        let offset = lba * SECTOR_SIZE;
        self.set_cursor(offset)
    }

    /// Set the cursor to the to the current cursor location + the `offset` in bytes
    pub fn set_cursor_relative(&self, offset: usize) -> MappedDiskResult<()> {
        self.set_cursor(self.current_offset() + offset)
    }

    /// Get the current cursor location
    pub fn current_offset(&self) -> usize {
        self.cursor.get()
    }
}

pub struct BufferedMappedDisk<'a> {
    disk: &'a MappedDisk,
    buffer_start_offset: usize,
    pub buffer_size: usize,
    pub buffer: Vec<u8>,
    cursor: Cell<usize>
}

impl<'a> BufferedMappedDisk<'a> {
    pub fn new(disk: &'a MappedDisk, buffer_size: usize) -> Self {
        Self {
            disk,
            buffer_start_offset: disk.current_offset(),
            buffer_size,
            buffer: Vec::new(),
            cursor: Cell::new(disk.current_offset() % buffer_size)
        }
    }

    pub fn fill_buffer_at(&mut self, offset: usize) -> MappedDiskResult<()> {
        let bytes = self.disk.read_bytes_at(offset, self.buffer_size)?;
        self.buffer_start_offset = offset;
        self.buffer = bytes.to_vec();
        println!("fill_buffer_at: {:#?}", self.cursor);
        Ok(())
    }

    pub fn read_bytes_at(&self, offset: usize, size: usize) -> MappedDiskResult<&[u8]> {
        if offset + size > self.buffer_size {
            Err(MappedDiskError::OutOfBounds(offset))
        } else {
            Ok(&self.buffer[offset..offset + size])
        }
    }

    pub fn read_bytes(&self, size: usize) -> MappedDiskResult<&[u8]> {
        let offset = self.cursor.get();
        let bytes = self.read_bytes_at(offset, size);
        self.cursor.set(offset + size);
        bytes
    }

    pub fn read_string_utf8(&self, size: usize) -> MappedDiskResult<String> {
        let bytes = self.read_bytes(size)?;
        Ok(String::from_utf8(bytes.to_vec()).unwrap())
    }

    pub fn read_string_utf16(&self, size: usize) -> MappedDiskResult<String> {
        let bytes = self.read_bytes(size)?;
        let u16_iter = bytes.chunks_exact(2)
                            .map(|b| u16::from_le_bytes([b[0], b[1]]));
        Ok(String::from_utf16(&u16_iter.collect::<Vec<_>>()).unwrap())
    }


    pub fn read<T: BinaryType>(&self) -> MappedDiskResult<T> {
        let bytes = self.read_bytes(T::SIZE)?;
        T::parse(bytes).map_err(|_| MappedDiskError::OutOfBounds(self.current_buffer_offset()))
    }

    pub fn read_with_size<T: BinaryParse>(&self, size: usize) -> MappedDiskResult<T> {
        let bytes = self.read_bytes(size)?;
        T::parse(bytes).map_err(|_| MappedDiskError::OutOfBounds(self.current_buffer_offset()))
    }

    pub fn peek<T: BinaryType>(&self) -> MappedDiskResult<T> {
        let offset = self.current_buffer_offset();
        self.read_at::<T>(offset)
    }

    pub fn set_cursor(&self, offset: usize) -> MappedDiskResult<()> {
        self.cursor.set(offset % self.buffer_size);
        Ok(())
    }

    pub fn set_cursor_relative(&self, offset: usize) -> MappedDiskResult<()> {
        self.cursor.set(self.cursor.get() + offset);
        Ok(())
    }

    pub fn current_offset(&self) -> usize {
        self.buffer_start_offset + self.current_buffer_offset()
    }

    fn read_at<T>(&self, offset: usize) -> MappedDiskResult<T>
    where
        T: BinaryType,
    {
        let bytes = self.read_bytes_at(offset, T::SIZE)?;
        T::parse(bytes).map_err(|_| MappedDiskError::OutOfBounds(offset))
    }

    fn current_buffer_offset(&self) -> usize {
        self.cursor.get()
    }
}
