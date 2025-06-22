use binary_struct::prelude::*;
use binary_struct::BinaryStruct;

use crate::mapped_disk::{MappedDisk, MappedDiskResult};

use super::BOOTSTRAPER_LENGTH;

pub const BOOT_SIGNATURE: [u8; 2] = [0x55, 0xAA];

#[derive(BinaryStruct, Debug)]
pub struct MbrPartitionTableEntry {
    bootable: u8,
    starting_chs: [u8; 3],
    partition_type: u8,
    ending_chs: [u8; 3],
    starting_lba: u32,
    num_sectors: u32,
}

impl MbrPartitionTableEntry {
    pub fn is_extended_partition(&self) -> bool {
        self.partition_type == 0x05 || self.partition_type == 0x0F
    }

    pub fn is_empty(&self) -> bool {
        self.bootable == 0
            && self.starting_chs.iter().all(|byte| *byte == 0)
            && self.partition_type == 0
            && self.ending_chs.iter().all(|byte| *byte == 0)
            && self.starting_lba == 0
            && self.num_sectors == 0
    }

    pub fn is_relative_to(&mut self, parent_entry: &MbrPartitionTableEntry) {
        self.starting_lba += parent_entry.starting_lba;
    }

    pub fn is_bootable(&self) -> bool {
        self.bootable == 0x80
    }

    pub fn is_valid_bootable_value(&self) -> bool {
        // https://en.wikipedia.org/wiki/Master_boot_record#PTE:
        // MBRs only accept 0x80, 0x00 means inactive, and 0x01–0x7F stand for invalid
        (self.bootable == 0x80 || self.bootable == 0x00) && !(0x01..0x7F).contains(&self.bootable)
    }

    pub fn starting_lba(&self) -> usize {
        self.starting_lba as usize
    }

    pub fn partition_type(&self) -> u8 {
        self.partition_type
    }

    pub fn partition_type_str(&self) -> &'static str {
        lookup_partition_type(self.partition_type)
    }

    pub fn num_sectors(&self) -> u32 {
        self.num_sectors
    }

    fn chs_head(chs: [u8; 3]) -> u8 {
        chs[0]
    }

    fn chs_sector(chs: [u8; 3]) -> u8 {
        chs[1] & ((1 << 6) - 1)
    }

    fn chs_cylinder(chs: [u8; 3]) -> u16 {
        ((chs[1] as u16 & 0b11000000) << 2) | (chs[2] as u16)
    }
}

pub fn parse_boot_records(
    disk: &MappedDisk,
    starting_lba: usize,
    is_ebr_pointer: bool,
) -> MappedDiskResult<Vec<MbrPartitionTableEntry>> {
    disk.set_cursor_from_lba(starting_lba)?;
    disk.set_cursor_relative(BOOTSTRAPER_LENGTH)?;
    let mut partition_table: Vec<MbrPartitionTableEntry> = Vec::new();
    for _ in 0..4 {
        let entry: MbrPartitionTableEntry = disk.read()?;
        if entry.is_empty() || !entry.is_valid_bootable_value() {
            break;
        }

        // If the partition is an extended partition, then we will jump to the EBR and parse the partition table there
        if entry.is_extended_partition() {
            let child_partition_table = parse_boot_records(disk, starting_lba + entry.starting_lba(), true)?;
            // Extended boot records have a starting LBA that is relative to the MBR entry that points to them
            let child_entries = child_partition_table
                .into_iter()
                .map(|mut child_entry| {
                    child_entry.is_relative_to(&entry);
                    child_entry
                })
                .collect::<Vec<_>>();

            if !is_ebr_pointer {
                partition_table.push(entry);
            } 
            partition_table.extend(child_entries);
        } else {
            partition_table.push(entry);
        }
    }
    Ok(partition_table)
}

pub fn parse_partition_tables(
    disk: &MappedDisk,
    starting_lba: usize,
) -> MappedDiskResult<Vec<MbrPartitionTableEntry>> {
    parse_boot_records(disk, starting_lba, false)
}

pub fn lookup_partition_type(partition_type: u8) -> &'static str {
    match partition_type {
        0x0 => "Empty",
        0x1 => "FAT12",
        0x2 => "XENIX root",
        0x3 => "XENIX usr",
        0x4 => "FAT16 <32M",
        0x5 => "Extended",
        0x6 => "FAT16",
        0x7 => "HPFS/NTFS/exFAT",
        0x8 => "AIX",
        0x9 => "AIX bootable",
        0xa => "OS/2 Boot Manag",
        0xb => "W95 FAT32",
        0xc => "W95 FAT32 (LBA)",
        0xe => "W95 FAT16 (LBA)",
        0xf => "W95 Ext'd (LBA)",
        0x10 => "OPUS",
        0x11 => "Hidden FAT12",
        0x12 => "Compaq diagnost",
        0x14 => "Hidden FAT16 <3",
        0x16 => "Hidden FAT16",
        0x17 => "Hidden HPFS/NTF",
        0x18 => "AST SmartSleep",
        0x1b => "Hidden W95 FAT3",
        0x1c => "Hidden W95 FAT3",
        0x1e => "Hidden W95 FAT1",
        0x24 => "NEC DOS",
        0x27 => "Hidden NTFS Win",
        0x39 => "Plan 9",
        0x3c => "PartitionMagic",
        0x40 => "Venix 80286",
        0x41 => "PPC PReP Boot",
        0x42 => "SFS",
        0x4d => "QNX4.x",
        0x4e => "QNX4.x 2nd part",
        0x4f => "QNX4.x 3rd part",
        0x50 => "OnTrack DM",
        0x51 => "OnTrack DM6 Aux",
        0x52 => "CP/M",
        0x53 => "OnTrack DM6 Aux",
        0x54 => "OnTrackDM6",
        0x55 => "EZ-Drive",
        0x56 => "Golden Bow",
        0x5c => "Priam Edisk",
        0x61 => "SpeedStor",
        0x63 => "GNU HURD or Sys",
        0x64 => "Novell Netware",
        0x65 => "Novell Netware",
        0x70 => "DiskSecure Mult",
        0x75 => "PC/IX",
        0x80 => "Old Minix",
        0x81 => "Minix / old Lin",
        0x82 => "Linux swap / So",
        0x83 => "Linux",
        0x84 => "OS/2 hidden or",
        0x85 => "Linux extended",
        0x86 => "NTFS volume set",
        0x87 => "NTFS volume set",
        0x88 => "Linux plaintext",
        0x8e => "Linux LVM",
        0x93 => "Amoeba",
        0x94 => "Amoeba BBT",
        0x9f => "BSD/OS",
        0xa0 => "IBM Thinkpad hi",
        0xa5 => "FreeBSD",
        0xa6 => "OpenBSD",
        0xa7 => "NeXTSTEP",
        0xa8 => "Darwin UFS",
        0xa9 => "NetBSD",
        0xab => "Darwin boot",
        0xaf => "HFS / HFS+",
        0xb7 => "BSDI fs",
        0xb8 => "BSDI swap",
        0xbb => "Boot Wizard hid",
        0xbc => "Acronis FAT32 L",
        0xbe => "Solaris boot",
        0xbf => "Solaris",
        0xc1 => "DRDOS/sec (FAT-",
        0xc4 => "DRDOS/sec (FAT-",
        0xc6 => "DRDOS/sec (FAT-",
        0xc7 => "Syrinx",
        0xda => "Non-FS data",
        0xdb => "CP/M / CTOS / .",
        0xde => "Dell Utility",
        0xdf => "BootIt",
        0xe1 => "DOS access",
        0xe3 => "DOS R/O",
        0xe4 => "SpeedStor",
        0xea => "Rufus alignment",
        0xeb => "BeOS fs",
        0xee => "GPT",
        0xef => "EFI (FAT-12/16/",
        0xf0 => "Linux/PA-RISC b",
        0xf1 => "SpeedStor",
        0xf4 => "SpeedStor",
        0xf2 => "DOS secondary",
        0xfb => "VMware VMFS",
        0xfc => "VMware VMKCORE",
        0xfd => "Linux raid auto",
        0xfe => "LANstep",
        0xff => "BBT",
        _ => "Unknown Partition Type",
    }
}
