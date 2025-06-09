use std::{cell::Cell, fs::File, os::unix::fs::FileTypeExt, path::Path};

use binary_struct::{BinaryParse, BinaryType};
use memmap2::{Mmap, MmapOptions};

pub const SECTOR_SIZE: usize = 512;

#[derive(Debug, thiserror::Error)]
pub enum MappedDiskError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Out of bounds access at offset {0}")]
    OutOfBounds(usize),
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
        println!("Got size: {}", size);
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

    /// Read `size_of<T>()` bytes starting from the current cursor location
    pub fn read<T: BinaryType>(&self) -> MappedDiskResult<T> {
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
