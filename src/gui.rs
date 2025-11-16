use byte_unit::Byte as ByteUnit;
use eframe::egui::{self, CentralPanel, Frame, Stroke};
use std::path::{Path, PathBuf};

use crate::{
    mapped_disk::{MappedDisk, SECTOR_SIZE},
    partition_tables::{
        gpt::{parse_gpt, GptPartitionTableEntry},
        mbr::{parse_partition_tables, MbrPartitionTableEntry},
        GPT_PARTITION_TYPE,
    },
};

pub mod menu;
pub mod partition;
pub mod partition_tables;
pub mod utils;
pub mod welcome;

#[derive(Debug)]
pub struct DiskContext {
    pub path: PathBuf,
    pub disk: MappedDisk,
    pub partition_table: Vec<PartitionTableEntry>,
    pub inner: ViewState,
}

impl DiskContext {
    pub fn new(path: &Path) -> Self {
        let disk = MappedDisk::new(&path).unwrap();
        let partition_table = Self::load_partition_tables(&disk);
        Self {
            path: path.to_owned(),
            disk,
            partition_table,
            inner: ViewState::PartitionTable,
        }
    }

    fn load_partition_tables(disk: &MappedDisk) -> Vec<PartitionTableEntry> {
        let partition_table = parse_partition_tables(&disk, 0).unwrap();
        match partition_table.get(0) {
            Some(entry) if entry.partition_type() == GPT_PARTITION_TYPE => parse_gpt(&disk)
                .unwrap()
                .into_iter()
                .map(PartitionTableEntry::from)
                .collect(),
            Some(_) => partition_table
                .into_iter()
                .map(PartitionTableEntry::from)
                .collect(),
            None => unreachable!("Unknown Partition Table Type"),
        }
    }

    fn show(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        match self.inner {
            ViewState::PartitionTable => partition_tables::show(ui, ctx, self),
            ViewState::Partition { index } => partition::show(ui, ctx, self, index),
        }
    }
}

#[derive(Debug)]
pub enum ViewState {
    PartitionTable,
    Partition { index: usize },
}

#[derive(Debug)]
enum AppState {
    Welcome,
    DiskLoaded(DiskContext),
}

impl Default for AppState {
    fn default() -> Self {
        Self::Welcome
    }
}

#[derive(Debug)]
pub enum PartitionTableEntry {
    Mbr(MbrPartitionTableEntry),
    Gpt(GptPartitionTableEntry),
}

impl From<MbrPartitionTableEntry> for PartitionTableEntry {
    fn from(value: MbrPartitionTableEntry) -> Self {
        Self::Mbr(value)
    }
}

impl From<GptPartitionTableEntry> for PartitionTableEntry {
    fn from(value: GptPartitionTableEntry) -> Self {
        Self::Gpt(value)
    }
}

impl PartitionTableEntry {
    fn starting_lba(&self) -> usize {
        match self {
            PartitionTableEntry::Mbr(mbr_partition_table_entry) => {
                mbr_partition_table_entry.starting_lba()
            }
            PartitionTableEntry::Gpt(gpt_partition_table_entry) => {
                gpt_partition_table_entry.starting_lba() as usize
            }
        }
    }
    fn ending_lba(&self) -> usize {
        match self {
            PartitionTableEntry::Mbr(mbr_partition_table_entry) => {
                mbr_partition_table_entry.starting_lba()
                    + mbr_partition_table_entry.num_sectors() as usize
                    - 1usize
            }
            PartitionTableEntry::Gpt(gpt_partition_table_entry) => {
                gpt_partition_table_entry.ending_lba() as usize
            }
        }
    }
    fn number_of_sectors(&self) -> usize {
        match self {
            PartitionTableEntry::Mbr(mbr_partition_table_entry) => {
                mbr_partition_table_entry.num_sectors() as usize
            }
            PartitionTableEntry::Gpt(gpt_partition_table_entry) => {
                gpt_partition_table_entry.number_of_sectors() as usize
            }
        }
    }
    fn partition_type_str(&self) -> &'static str {
        match self {
            PartitionTableEntry::Mbr(mbr_partition_table_entry) => {
                mbr_partition_table_entry.partition_type_str()
            }
            PartitionTableEntry::Gpt(gpt_partition_table_entry) => {
                gpt_partition_table_entry.partition_type_str()
            }
        }
    }

    fn headers(&self) -> Vec<&'static str> {
        match self {
            PartitionTableEntry::Mbr(_) => Vec::from([
                "Device",
                "Bootable",
                "Starting LBA",
                "Ending LBA",
                "Total Sectors",
                "Size",
                "Partition Type",
            ]),
            PartitionTableEntry::Gpt(_) => Vec::from([
                "Device",
                "Starting LBA",
                "Ending LBA",
                "Total Sectors",
                "Size",
                "Partition Type",
            ]),
        }
    }

    fn to_row(&self) -> Vec<String> {
        let mut row = Vec::from([
            self.starting_lba().to_string(),
            self.ending_lba().to_string(),
            self.number_of_sectors().to_string(),
            format!(
                "{:.2}",
                ByteUnit::from_u64(self.number_of_sectors() as u64 * SECTOR_SIZE as u64)
                    .get_appropriate_unit(byte_unit::UnitType::Binary)
            ),
            self.partition_type_str().to_string(),
        ]);

        if let PartitionTableEntry::Mbr(mbr_partition_table_entry) = self {
            row.insert(0, mbr_partition_table_entry.is_bootable().to_string())
        }
        row
    }
}

#[derive(Default, Debug)]
pub struct Application {
    state: AppState,
}

impl Application {
    pub fn new() -> Self {
        let mut s = Self {
            state: AppState::Welcome,
        };
        s.open_file(&PathBuf::new().join("kingston_gpt_2.dd"));
        s
    }

    pub fn open_file(&mut self, file: &Path) {
        self.state = AppState::DiskLoaded(DiskContext::new(&file));
    }
}

impl eframe::App for Application {
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        menu::show(self, ctx);

        CentralPanel::default()
            .frame(Frame::default().stroke(Stroke::NONE))
            .show(ctx, |ui| match &mut self.state {
                AppState::Welcome => welcome::show(ui, ctx),
                AppState::DiskLoaded(disk_context) => disk_context.show(ui, ctx),
            });
    }
}
