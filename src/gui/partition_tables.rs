use byte_unit::Byte as ByteUnit;
use eframe::egui::{
    self, Color32, CornerRadius, CursorIcon, FontId, Frame, Label, Margin, Rect, RichText, Sense,
    Stroke, Ui, Vec2,
};
use egui_extras::{Column, TableBody, TableBuilder, TableRow};

use crate::{
    gui::AppView, mapped_disk::SECTOR_SIZE, partition_tables::{gpt::GptPartitionTableEntry, mbr::MbrPartitionTableEntry}
};

use super::{draw_grid, Application};

fn show_floating_menu<R>(
    ui: &mut Ui,
    title: impl Into<RichText>,
    add_contents: impl FnOnce(&mut Ui) -> R,
) {
    let corner_radius = CornerRadius::ZERO.at_least(8);
    let frame = Frame {
        fill: ui.visuals().window_fill(),
        stroke: Stroke::new(1.0, Color32::DARK_GRAY),
        corner_radius,
        ..Default::default()
    };

    frame.show(ui, |ui| {
        let width = ui.available_width();
        let mut inner_corner_radius = corner_radius.clone();
        inner_corner_radius.se = 0;
        inner_corner_radius.sw = 0;
        Frame {
            fill: Color32::from_rgb(46, 46, 46),
            inner_margin: egui::Margin {
                left: 6,
                right: 2,
                top: 2,
                bottom: 2,
            },
            corner_radius: inner_corner_radius,
            ..Default::default()
        }
        .show(ui, |ui| {
            ui.set_min_width(width);
            let text = title
                .into()
                .color(Color32::WHITE)
                .font(FontId::proportional(16.0));
            ui.heading(text);
        });
        add_contents(ui);
    });
}

pub trait ToTable {
    fn headers(&self) -> Vec<&'static str>;
    fn to_row(&self) -> Vec<String>;
}

impl ToTable for GptPartitionTableEntry {
    fn headers(&self) -> Vec<&'static str> {
        Vec::from([
            "Device",
            "Starting LBA",
            "Ending LBA",
            "Total Sectors",
            "Size",
            "Partition Type",
        ])
    }

    fn to_row(&self) -> Vec<String> {
        Vec::from([
            self.starting_lba().to_string(),
            self.ending_lba().to_string(),
            self.number_of_sectors().to_string(),
            format!(
                "{:.2}",
                ByteUnit::from_u64(self.number_of_sectors() * SECTOR_SIZE as u64)
                    .get_appropriate_unit(byte_unit::UnitType::Binary)
            ),
            self.partition_type_str().to_string(),
        ])
    }
}

impl ToTable for MbrPartitionTableEntry {
    fn headers(&self) -> Vec<&'static str> {
        Vec::from([
            "Device",
            "Bootable",
            "Starting LBA",
            "Ending LBA",
            "Total Sectors",
            "Size",
            "Partition Type",
        ])
    }

    fn to_row(&self) -> Vec<String> {
        Vec::from([
            self.is_bootable().to_string(),
            self.starting_lba().to_string(),
            (self.starting_lba() + self.num_sectors() as usize - 1usize).to_string(),
            self.num_sectors().to_string(),
            format!(
                "{:.2}",
                ByteUnit::from_u64(self.num_sectors() as u64 * SECTOR_SIZE as u64)
                    .get_appropriate_unit(byte_unit::UnitType::Binary)
            ),
            self.partition_type_str().to_string(),
        ])
    }
}

pub fn show(ui: &mut egui::Ui, app: &mut Application, ctx: &egui::Context) {
    let path = &app.path.clone().unwrap();
    let file_name = path.file_name().unwrap().to_str().unwrap();
    app.load_partition_tables();
    let (headers, mut rows) = {
        (
            app.partition_table.first().unwrap().headers(),
            app.partition_table
                .iter()
                .map(|entry| entry.to_row())
                .collect::<Vec<_>>(),
        )
    };

    rows.iter_mut()
        .enumerate()
        .for_each(|(i, row)| row.insert(0, format!("{}{}", file_name.to_owned(), i + 1)));

    let available = ui.available_rect_before_wrap();

    let painter = ui.painter();
    painter.rect_filled(available, 0.0, egui::Color32::BLACK);

    draw_grid(&painter, available);
    let menu_size = Vec2::new(800.0, 340.0);
    let center = ui.max_rect().center();
    let rect = Rect::from_center_size(center, menu_size);
    let builder = egui::UiBuilder::new().max_rect(rect);

    let title = format!("{} Partitions", file_name);
    ui.allocate_new_ui(builder, |ui| {
        show_floating_menu(ui, title, |ui| {
            draw_table(ui, app, &headers, &rows);
        });
    });
}

fn draw_table(ui: &mut Ui, app: &mut Application, headers: &[&'static str], rows: &[Vec<String>]) {
    Frame::default()
        .inner_margin(Margin::symmetric(6, 0))
        .show(ui, |ui| {
            TableBuilder::new(ui)
                .striped(true)
                .columns(Column::remainder(), headers.len()) // striped rows for readability
                .header(20.0, move |mut table_header| {
                    draw_table_headers(&mut table_header, headers);
                })
                .body(|mut body| {
                    draw_table_rows(app, &mut body, rows);
                });
        });
}

fn draw_table_headers(table_header: &mut TableRow, headers: &[&'static str]) {
    for header_text in headers {
        table_header.col(|ui| {
            ui.label(RichText::new(*header_text).strong());
        });
    }
}

fn draw_table_rows(app: &mut Application, body: &mut TableBody, rows: &[Vec<String>]) {
    for (row_index, row_text) in rows.into_iter().enumerate() {
        body.row(18.0, |mut row| {
            for (col_index, col) in row_text.into_iter().enumerate() {
                row.col(|ui| {
                    if col_index == 0 {
                        let label =
                            Label::new(RichText::new(col).underline().color(Color32::LIGHT_BLUE))
                                .sense(Sense::click());
                        let response = ui.add(label);
                        if response.hovered() {
                            response.ctx.set_cursor_icon(CursorIcon::PointingHand);
                        }
                        if response.clicked() {
                            app.view = AppView::Partition { index: row_index };
                        }
                    } else {
                        ui.label(col);
                    }
                });
            }
        });
    }
}
