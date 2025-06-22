use std::path::{Path, PathBuf};

use eframe::egui::{self, CentralPanel, Frame, Stroke};
use partition_tables::ToTable;

use crate::{
    mapped_disk::MappedDisk,
    partition_tables::{
        gpt::{parse_gpt, GptPartitionTableEntry},
        mbr::{parse_partition_tables, MbrPartitionTableEntry},
        GPT_PARTITION_TYPE,
    },
};

pub mod menu;
pub mod partition_tables;
pub mod welcome;

#[derive(Debug)]
enum AppView {
    Welcome,
    PartitionTables,
    Partition { index: usize },
}

impl Default for AppView {
    fn default() -> Self {
        Self::Welcome
    }
}

pub trait PartitionAndTable: Partition + ToTable + std::fmt::Debug {}
impl<T> PartitionAndTable for T where T: Partition + ToTable + std::fmt::Debug {}

pub trait Partition: std::fmt::Debug {
    fn starting_lba(&self) -> usize;
    fn ending_lba(&self) -> usize;
    fn number_of_sectors(&self) -> usize;
    fn partition_type_str(&self) -> &'static str;
}

impl Partition for MbrPartitionTableEntry {
    fn starting_lba(&self) -> usize {
        self.starting_lba()
    }

    fn ending_lba(&self) -> usize {
        self.starting_lba() + self.number_of_sectors()
    }

    fn number_of_sectors(&self) -> usize {
        self.num_sectors() as usize
    }

    fn partition_type_str(&self) -> &'static str {
        self.partition_type_str()
    }
}

impl Partition for GptPartitionTableEntry {
    fn starting_lba(&self) -> usize {
        self.starting_lba() as usize
    }

    fn ending_lba(&self) -> usize {
        self.ending_lba() as usize
    }

    fn number_of_sectors(&self) -> usize {
        self.number_of_sectors() as usize
    }

    fn partition_type_str(&self) -> &'static str {
        self.partition_type_str()
    }
}

#[derive(Default, Debug)]
pub struct Application {
    view: AppView,
    path: Option<PathBuf>,
    partition_table: Vec<Box<dyn PartitionAndTable>>,
}

impl Application {
    pub fn new() -> Self {
        Self {
            view: AppView::PartitionTables,
            path: Some(PathBuf::new().join("kingston_gpt_2.dd")),
            partition_table: Vec::new(),
        }
    }

    pub fn open_file(&mut self, file: &Path) {
        self.path = Some(file.into());
        self.view = AppView::PartitionTables;
    }

    pub fn load_partition_tables(&mut self) {
        if let Some(path) = &self.path {
            let disk = MappedDisk::new(&path).unwrap();
            let partition_table = parse_partition_tables(&disk, 0).unwrap();
            match partition_table.get(0) {
                Some(entry) if entry.partition_type() == GPT_PARTITION_TYPE => {
                    let gpt_partition_table = parse_gpt(&disk).unwrap();
                    self.partition_table = gpt_partition_table
                        .into_iter()
                        .map(|entry| Box::new(entry) as Box<dyn PartitionAndTable>)
                        .collect();
                }
                Some(_) => {
                    self.partition_table = partition_table
                        .into_iter()
                        .map(|entry| Box::new(entry) as Box<dyn PartitionAndTable>)
                        .collect();
                }
                None => unreachable!("Unknown Partition Table Type"),
            }
        }
    }
}

impl eframe::App for Application {
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        menu::show(self, ctx);
        CentralPanel::default()
            .frame(Frame::default().stroke(Stroke::NONE))
            .show(ctx, |ui| match self.view {
                AppView::Welcome => welcome::show(ui, self, ctx),
                AppView::PartitionTables => partition_tables::show(ui, self, ctx),
                AppView::Partition { index } => todo!(),
            });
    }
}

fn draw_grid(painter: &egui::Painter, rect: egui::Rect) {
    let spacing = 20.0;

    let mut x = rect.left();
    while x <= rect.right() {
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            (1.0, egui::Color32::from_rgb(46, 46, 46)),
        );
        x += spacing;
    }

    let mut y = rect.top();
    while y <= rect.bottom() {
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            (1.0, egui::Color32::from_rgb(46, 46, 46)),
        );
        y += spacing;
    }
}
