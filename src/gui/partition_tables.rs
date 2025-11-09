use eframe::egui::{
    self, Color32, CornerRadius, CursorIcon, FontId, Frame, Label, Margin, Rect, RichText, Sense,
    Stroke, Ui, Vec2,
};
use egui_extras::{Column, TableBody, TableBuilder, TableRow};

use crate::{
    gui::{DiskContext, ViewState, utils::draw_grid},
};


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

pub fn show(ui: &mut egui::Ui, ctx: &egui::Context, disk_context: &mut DiskContext) {
    let file_name = disk_context.path.file_name().unwrap().to_str().unwrap();
    let (headers, mut rows) = {
        (
            disk_context.partition_table.first().unwrap().headers(),
            disk_context
                .partition_table
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
            draw_table(ui, disk_context, &headers, &rows);
        });
    });
}

fn draw_table(
    ui: &mut Ui,
    disk_context: &mut DiskContext,
    headers: &[&'static str],
    rows: &[Vec<String>],
) {
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
                    draw_table_rows(disk_context, &mut body, rows);
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

fn draw_table_rows(disk_context: &mut DiskContext, body: &mut TableBody, rows: &[Vec<String>]) {
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
                            disk_context.inner = ViewState::Partition { index: row_index };
                        }
                    } else {
                        ui.label(col);
                    }
                });
            }
        });
    }
}
