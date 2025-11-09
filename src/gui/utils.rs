use eframe::egui;

pub fn draw_grid(painter: &egui::Painter, rect: egui::Rect) {
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