use eframe::egui::{self, Pos2, RichText, Vec2};

use crate::gui::utils::draw_grid;

pub fn show(ui: &mut egui::Ui, ctx: &egui::Context) {
    let available = ui.available_rect_before_wrap();

    let painter = ui.painter();
    painter.rect_filled(available, 0.0, egui::Color32::BLACK);

    draw_grid(&painter, available);
    let available = ui.available_rect_before_wrap();
    let text_size = ui.text_style_height(&egui::TextStyle::Heading);

    let desired_size = Vec2::new(400.0, text_size + 20.0); // Size of the content box
    let center_pos = Pos2::new(
        available.center().x - desired_size.x * 0.5,
        available.center().y - desired_size.y * 0.5,
    );

    let rect = egui::Rect::from_min_size(center_pos, desired_size);

    ui.allocate_ui_at_rect(rect, |ui| {
        ui.vertical_centered(|ui| {
            ui.heading(RichText::new("Drag and drop a file to begin").color(egui::Color32::WHITE).heading());
        });
    });
}