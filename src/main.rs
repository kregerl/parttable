// use crate::{
//     apm::{display_apm_partitions, parse_apm},
//     mbr::parse_mbr,
// };
// use apm::is_apm_disk;
// use clap::{Parser, Subcommand};
// use gpt::{display_gpt, parse_gpt};
// use mbr::{format_partition_table_rows, lookup_partition_type, MbrPartitionTableEntryNode};
// use mft::{display_mft, mft_to_csv, parse_pbr, timestomp_mft};
// use std::path::{Path, PathBuf};

// #[cfg(test)]
// use std::io::Read;

// mod apm;
// mod bytestream;
// mod gpt;
// mod gui;
// mod mbr;
// mod mft;

// #[derive(Debug, Parser)]
// struct Arguments {
//     image_path: String,
//     #[arg(long)]
//     show_chs: bool,
//     #[arg(long)]
//     extract_mft: bool,
//     #[arg(long)]
//     dump_mft: Option<String>,
//     #[command(subcommand)]
//     timestomp: Option<Timestomp>,
// }

// #[derive(Debug, Subcommand)]
// pub enum Timestomp {
//     /// Timestomp `file_name` with the `timestamp`
//     Timestomp {
//         /// Name of the file entry in the MFT
//         file_name: String,
//         /// Unix epoch timestamp to timestomp with
//         timestamp: u64,
//     },
// }

// fn main() {
//     let path = Path::new("sandisk2.dd");
//     let options = eframe::NativeOptions::default();
//     eframe::run_native(
//         "NTFS Parser",
//         options,
//         Box::new(|_cc| Ok(Box::new(gui::Application::new(path)))),
//     )
//     .unwrap();

//     // let mbr = parse_mbr(path);
//     // let mbr_node = match mbr {
//     //     Ok(root_node) => root_node,
//     //     Err(error) => panic!("Error parsing MBR: {}", error),
//     // };

//     // println!("mbr_node: {:#?}", mbr_node);
//     // let rows = format_partition_table_rows(mbr_node, true);
//     // println!("Rows: {:#?}", rows);

//     // let args = Arguments::parse();
//     // let path = Path::new(&args.image_path);
//     // if is_apm_disk(&args.image_path).unwrap() {
//     //     let partitions = parse_apm(&args.image_path).unwrap();
//     //     display_apm_partitions(partitions);
//     // } else {
//     //     // FIXME: This could all be done nicer if the signature is checked first.
//     //     let mbr = parse_mbr(path);
//     //     let mbr_node = match mbr {
//     //         Ok(root_node) => root_node,
//     //         Err(error) => panic!("Error parsing MBR: {}", error),
//     //     };

//     //     if mbr_node.is_gpt() {
//     //         let partition_table = match parse_gpt(path) {
//     //             Ok(partition_table) => partition_table,
//     //             Err(error) => panic!("Error parsing GPT: {}", error),
//     //         };

//     //         if args.extract_mft || args.timestomp.is_some() || args.dump_mft.is_some() {
//     //             let ntfs_partition = partition_table.into_iter().find(|entry| {
//     //                 entry.get_partition_type_guid() == "EBD0A0A2-B9E5-4433-87C0-68B6B72699C7"
//     //             });
//     //             let mft_records = match ntfs_partition {
//     //                 Some(partition) => parse_pbr(path, partition.starting_lba()).unwrap(),
//     //                 None => panic!("Could not find a `Microsoft basic data` partition."),
//     //             };
//     //             if args.dump_mft.is_some() {
//     //                 mft_to_csv(mft_records, &args.dump_mft.unwrap()).unwrap();
//     //             } else if args.extract_mft {
//     //                 display_mft(mft_records);
//     //             } else {
//     //                 timestomp_mft(
//     //                     &PathBuf::from(args.image_path),
//     //                     mft_records,
//     //                     args.timestomp.unwrap(),
//     //                 );
//     //             }
//     //         } else {
//     //             display_gpt(partition_table);
//     //         }
//     //     } else {
//     //         if args.extract_mft || args.timestomp.is_some() || args.dump_mft.is_some() {
//     //             let first_child = mbr_node.children.unwrap();
//     //             let first_partition = first_child.get(0).unwrap();
//     //             let mft_records = parse_pbr(path, first_partition.starting_lba() as u64).unwrap();

//     //             if args.dump_mft.is_some() {
//     //                 mft_to_csv(mft_records, &args.dump_mft.unwrap()).unwrap();
//     //             } else if args.extract_mft {
//     //                 display_mft(mft_records);
//     //             } else {
//     //                 timestomp_mft(
//     //                     &PathBuf::from(args.image_path),
//     //                     mft_records,
//     //                     args.timestomp.unwrap(),
//     //                 );
//     //             }
//     //         } else {
//     //             display_mbr(mbr_node, args.show_chs);
//     //         }
//     //     }
//     // }
// }

// #[test]
// pub fn test_open_drive() {
//     use std::fs::OpenOptions;

//     let path = Path::new("\\\\.\\PhysicalDrive0");
//     let mut f = OpenOptions::new().read(true).open(path).unwrap();
//     // Windows requires that physical drives are read in sectors.
//     let mut x = vec![0u8; 512];
//     f.read_exact(&mut x).unwrap();
//     println!("Buffer: {:#?}", x);
// }

use std::{cell::Cell, fs::File, path::Path};

use binary_struct::{BinaryParse, Skip};
use binary_struct_derive::binary_struct;
use mbr::parse_mbr;
use memmap2::Mmap;


#[derive(Debug, thiserror::Error)]
pub enum MappedDiskError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Out of bounds access at offset {0}")]
    OutOfBounds(usize),
}

pub struct MappedDisk {
    mmap: Mmap,
    cursor: Cell<usize>,
}

impl MappedDisk {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, MappedDiskError> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        Ok(Self { mmap, cursor: Cell::new(0) })
    }

    /// Read `size` bytes starting from `offset` without advancing the cursor
    fn read_bytes_at(&self, offset: usize, size: usize) -> Result<&[u8], MappedDiskError> {
        if offset + size > self.mmap.len() {
            Err(MappedDiskError::OutOfBounds(offset))
        } else {
            Ok(&self.mmap[offset..offset + size])
        }
    }

    /// Read bytes and interpret them as `T` starting from `offset`.
    /// This function does not start at or advance the cursor
    fn read_at<T>(&self, offset: usize) -> Result<T, MappedDiskError>
    where
        T: BinaryParse,
    {
        let bytes = self.read_bytes_at(offset, T::SIZE)?;
        T::parse(bytes).map_err(|_| MappedDiskError::OutOfBounds(offset))
    }

    /// Read `size` bytes starting from the current cursor location
    /// This function advances the cursor after a read
    pub fn read_bytes(&self, size: usize) -> Result<&[u8], MappedDiskError> {
        let offset = self.cursor.get();
        self.read_bytes_at(offset, size).map(|bytes| {
            self.cursor.set(offset + size);
            bytes
        })
    }
    
    /// Read `size_of<T>()` bytes starting from the current cursor location
    pub fn read<T: BinaryParse>(&self) -> Result<T, MappedDiskError> {
        let bytes = self.read_bytes(T::SIZE)?;
        T::parse(bytes).map_err(|_| MappedDiskError::OutOfBounds(self.cursor.get()))
    }

    /// Read `size_of<T>()` bytes starting from the current cursor location without advancing the cursor
    pub fn peek<T: BinaryParse>(&self) -> Result<T, MappedDiskError> {
        let offset = self.cursor.get();
        self.read_at::<T>(offset)
    }

    /// Set cursor location in bytes
    pub fn set_cursor(&self, offset: usize) -> Result<(), MappedDiskError> {
        if offset > self.mmap.len() {
            Err(MappedDiskError::OutOfBounds(offset))
        } else {
            self.cursor.set(offset);
            Ok(())
        }
    }

    /// Set the cursor to the to the current cursor location + the `offset`
    pub fn set_cursor_relative(&self, offset: usize) -> Result<(), MappedDiskError> {
        if self.current_offset() + offset > self.mmap.len() {
            Err(MappedDiskError::OutOfBounds(offset))
        } else {
            self.cursor.set(self.current_offset() + offset);
            Ok(())
        }
    }

    /// Get the current cursor location
    pub fn current_offset(&self) -> usize {
        self.cursor.get()
    }
}

#[binary_struct]
pub struct MbrPartitionTableEntry {
    bootable: u8,
    starting_chs: [u8; 3],
    partition_type: u8,
    ending_chs: [u8; 3],
    lba_start: u32,
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
            && self.lba_start == 0
            && self.num_sectors == 0
    }
}

fn parse_partition_tables(disk: &MappedDisk, starting_lba: usize) {
    disk.set_cursor((512 * starting_lba) + 446).unwrap();
    let mut partition_table: Vec<MbrPartitionTableEntry> = Vec::new();
    for _ in 0..4 {
        let entry: MbrPartitionTableEntry = disk.read().unwrap();
        let is_not_bootable = entry.bootable != 0x00 && entry.bootable != 0x80 && (0x01..0x7F).contains(&entry.bootable);
        if entry.is_empty() || is_not_bootable {
            break;
        }
        
        if entry.is_extended_partition() {
            parse_partition_tables(disk, entry.lba_start as usize);
        }
        partition_table.push(entry);
    }
}

fn main() {
    let disk = MappedDisk::new("sandisk2.dd").unwrap();
    parse_partition_tables(&disk, 0);
}

mod mbr;
mod bytestream;

#[test]
fn test() {
    let path = Path::new("sandisk2.dd");

    let mbr = parse_mbr(path);
    let mbr_node = match mbr {
        Ok(root_node) => root_node,
        Err(error) => panic!("Error parsing MBR: {}", error),
    };

    println!("mbr_node: {:#?}", mbr_node);
}