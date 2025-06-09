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

use binary_struct::{BinaryStruct, Skip};
use mapped_disk::{MappedDisk, MappedDiskResult};
use ntfs::mft::{parse_attribute, parse_mft, NtfsReader};
use ntfs::pbr::{parse_pbr, validate_pbr};
use partition_tables::{gpt::parse_gpt, mbr::parse_partition_tables, GPT_PARTITION_TYPE};

mod bytestream;
mod mapped_disk;
mod ntfs;
mod partition_tables;

#[test]
fn test() {
    let high_nibble = 1u8;
    let mut offset = -10i64;
    println!("offset before: {:#066b}", offset);

    if high_nibble > 0 && (offset & (1 << (high_nibble * 8 - 1))) != 0 {
        let mask = !0 << (high_nibble * 8);
        offset |= mask;
    }
    println!("offset: {}", offset);
    println!("offset  after: {:#066b}", offset);
}


fn main() {
    // let disk = MappedDisk::new("/dev/sdd").unwrap();
    let disk = MappedDisk::new("kingston_gpt.dd").unwrap();
    let partition_table = parse_partition_tables(&disk, 0).unwrap();

    if partition_table
        .iter()
        .any(|entry| entry.partition_type() == GPT_PARTITION_TYPE)
    {
        let gpt_partition_table = parse_gpt(&disk).unwrap();
        for partition_table_entry in gpt_partition_table {
            // Is NTFS partition
            if partition_table_entry.partition_type() == "EBD0A0A2-B9E5-4433-87C0-68B6B72699C7" {
                let partition_boot_record =
                    parse_pbr(&disk, partition_table_entry.starting_lba() as usize).unwrap();
                let fs_reader = NtfsReader::new(
                    &disk,
                    &partition_boot_record,
                    partition_table_entry.starting_lba() as usize,
                );
                println!(
                    "Start of the partition boot record: {}",
                    partition_table_entry.starting_lba()
                );

                println!("partition_boot_record: {:#?}", partition_boot_record);

                parse_mft(&fs_reader, &partition_boot_record).unwrap();
                break;
            }
        }
    }
}


// fn main() {
//     let disk = MappedDisk::new("kingston_gpt.dd").unwrap();
//     disk.set_cursor(0x106550).unwrap();
//     let x = parse_attribute(&disk).unwrap();
//     println!("Attribute: {:#?}", x);
//     let y = parse_attribute(&disk).unwrap();
//     println!("Attribute 2: {:#?}", y);
// }