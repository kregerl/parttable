use eframe::egui;
use log::debug;

use crate::{
    gui::{DiskContext, PartitionTableEntry}, ntfs::{mft::{NtfsReader, parse_mft}, pbr::parse_pbr}, partition_tables::{NTFS_GPT_PARTITION_TYPE, NTFS_MBR_PARTITION_TYPE}
};

pub fn show(ui: &mut egui::Ui, ctx: &egui::Context, disk_context: &mut DiskContext, index: usize) {
    let maybe_partition = disk_context.partition_table.get(index);
    if let Some(partition) = maybe_partition {
        ui.label(partition.partition_type_str());
        let maybe_starting_lba: Option<usize> = match partition {
            PartitionTableEntry::Mbr(mbr_partition_table_entry) if mbr_partition_table_entry.partition_type() == NTFS_MBR_PARTITION_TYPE => Some(mbr_partition_table_entry.starting_lba()),
            PartitionTableEntry::Gpt(gpt_partition_table_entry) if gpt_partition_table_entry.partition_type() == NTFS_GPT_PARTITION_TYPE => Some(gpt_partition_table_entry.starting_lba() as usize),
            _ => None
        };
        if let Some(starting_lba) = maybe_starting_lba {
            let before_pbr = disk_context.disk.current_offset();
            debug!("before PBR: {:#?}", starting_lba * 512);
            let partition_boot_record = parse_pbr(&disk_context.disk, starting_lba).unwrap();
            debug!("After PBR: {:#?}", disk_context.disk.current_offset());
            debug!("Difference: {}", disk_context.disk.current_offset() - before_pbr);
            let mut fs_reader = NtfsReader::new(
                &disk_context.disk,
                &partition_boot_record,
                starting_lba,
            );
            debug!(
                "Start of the partition boot record: {}",
                starting_lba
            );

            debug!("partition_boot_record: {:#?}", partition_boot_record);

            parse_mft(&mut fs_reader, &partition_boot_record).unwrap();
        }
    }
}
