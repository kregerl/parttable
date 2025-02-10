use std::{
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
};

use eframe::egui;
use egui_extras::{Column, TableBuilder};

use crate::mbr::{format_partition_table_rows, parse_mbr};

#[derive(Default, Debug)]
pub struct Application {
    path: PathBuf,
    partition_table_data: Vec<Vec<String>>,
}

impl Application {
    pub fn new(path: &Path) -> Self {
        let mbr = parse_mbr(&path);
        let mbr_node = match mbr {
            Ok(root_node) => root_node,
            Err(error) => panic!("Error parsing MBR: {}", error),
        };

        Self {
            path: path.to_path_buf(),
            partition_table_data: format_partition_table_rows(mbr_node, true),
        }
    }
}

impl eframe::App for Application {
    fn update(&mut self, ctx: &eframe::egui::Context, frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("NTFS Parser Data Table");

            let table = TableBuilder::new(ui)
                .striped(true)
                .columns(Column::remainder(), 5) // File Name Column
                .resizable(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .min_scrolled_height(200.0);

            table
                .header(30.0, |mut header| {
                    for header_str in [
                        "Bootable",
                        "LBA Starting Sector",
                        "LBA Ending Sector",
                        "Total Sectors",
                        "Partition Type"
                    ] {
                        header.col(|ui| {
                            ui.strong(header_str);
                        });
                    }
                })
                .body(|mut body| {
                    for partition_table in &self.partition_table_data {
                        body.row(30.0, |mut row| {
                            for value in partition_table {
                                row.col(|ui| {
                                    ui.label(value);
                                });
                            }
                        });
                    }
                });

            ctx.request_repaint();
        });
    }
}
