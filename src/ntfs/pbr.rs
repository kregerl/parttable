use binary_struct::prelude::*;
use binary_struct::{BinaryStruct, Skip};

use crate::mapped_disk::{MappedDisk, MappedDiskResult, SECTOR_SIZE};

#[derive(Debug)]
pub enum NtfsError {
    InvalidOEMId,
}

#[derive(BinaryStruct, Debug)]
pub struct NtfsPartitionBootRecord {
    _jump_instruction: Skip<3>,
    #[binary_struct(num_bytes = 8)]
    oem_id: String,
    sector_size: u16,
    sectors_per_cluster: u8,
    // Two reserved bytes, always 0 on NTFS
    _reserved: Skip<2>,
    // Always 0 on NTFS
    // Originally the number of FATs for a FAT partition
    _fat_num_fats: Skip<1>,
    // Always 0 on NTFS
    // Originally the max number of root directory entries under FAT12/FAT16
    _fat_max_root_dir_entries_fat: Skip<2>,
    // Not used by NTFS but are usually set to 0
    // Originally used for the FAT12/FAT16 small sectors count - must be 0 for FAT32
    _fat_small_sectors_count: Skip<2>,
    media_description_id: u8,
    // Always 0 on NTFS
    // Originally for FAT12/FAT16 sectors per fat
    _fat_sectors_per_fat: Skip<2>,
    sectors_per_track: u16,
    num_heads: u16,
    num_hidden_sectors: u32,
    // Usually 0 in NTFS
    // Originally used for the total number of sectors in a FAT32 volume
    _fat_total_number_of_sectors: Skip<4>,
    // The first byte is the drive number
    _ntfs_drive_number: Skip<4>,
    // This value will always be 1 sector less than the total number of sectors listed in the volumes
    // partition table entry because the NTFS "Backup Boot Sector" is not considered part of the NTFS volume.
    number_of_sectors_in_volume: u64,
    logical_cluster_number_of_mft: u64,
    logical_cluster_number_of_mft_mirr: u64,
    // - If this value, when read in two’s complement, is positive,
    //   i.e. if its value goes from 00h to 7Fh (0000 0000 a 0111 1111),
    //   it actually designates the number of clusters per register
    // - If this value, when read in two’s complement, is negative,
    //   i.e. if its value goes from 80h to FFh (1000 0000 a 1111 1111), the
    //   size in bytes of each register will be equal to  2 to the power of the byte absolute value.
    mft_size: i8,
    // Usually 0 in NTFS
    _unused: Skip<2>,
    // Used to allocate space for NTFS structures such as directories.
    clusters_per_index_buffer: i8,
    // Usually 0 in NTFS
    _unused2: Skip<3>,
    // volume_serial_number: [u8; 8],
    _unused3: Skip<4>,
}

impl NtfsPartitionBootRecord {
    pub fn sector_size(&self) -> u16 {
        self.sector_size
    }

    pub fn sectors_per_cluster(&self) -> u8 {
        self.sectors_per_cluster
    }

    pub fn mft_size(&self) -> usize {
        if self.mft_size < 0 {
            2u32.pow(self.mft_size.abs() as u32) as usize
        } else {
            self.mft_size as usize * self.sectors_per_cluster as usize * self.sector_size as usize
        }
    }

    pub fn number_of_sectors_in_volume(&self) -> usize {
        self.number_of_sectors_in_volume as usize
    }
}

pub fn parse_pbr(
    disk: &MappedDisk,
    starting_lba: usize,
) -> MappedDiskResult<NtfsPartitionBootRecord> {
    disk.set_cursor_from_lba(starting_lba)?;
    disk.read::<NtfsPartitionBootRecord>()
}

pub fn validate_pbr(
    partition_boot_record: &NtfsPartitionBootRecord,
    starting_lba: u64,
) -> Result<usize, NtfsError> {
    if partition_boot_record.oem_id.trim() != "NTFS" {
        return Err(NtfsError::InvalidOEMId);
    }

    Ok((starting_lba
        + (partition_boot_record.logical_cluster_number_of_mft
            * partition_boot_record.sectors_per_cluster as u64)) as usize)
}
