use eframe::egui::{self, TopBottomPanel};

use super::Application;

pub fn show(app: &mut Application, ctx: &egui::Context) {
    TopBottomPanel::top("menu_bar").show(ctx, |ui| {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Open Disk Image").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_file() {
                        println!("Path: {:#?}", path);
                        app.open_file(&path);
                    }
                    ui.close_menu();
                }
                if ui.button("Open Recent").clicked() {
                    // TODO: Implement recent files
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Search").clicked() {
                    // TODO: Search
                    ui.close_menu();
                }

                if ui.button("Exit").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("About").clicked() {
                    // Show about dialog or info
                    ui.close_menu();
                }
            });
        });
    });
}