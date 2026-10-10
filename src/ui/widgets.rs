//! Small reusable UI pieces so pages look like one system.

use super::theme::Palette;
use eframe::egui::{self, Color32, CornerRadius, Response, RichText, Sense, Stroke, Ui, Vec2};

/// A filled accent button for the primary action in a group.
pub fn primary_button(ui: &mut Ui, palette: &Palette, label: &str) -> Response {
    let text = RichText::new(label).color(palette.accent_text).strong();
    ui.add(egui::Button::new(text).fill(palette.accent).corner_radius(8.0).min_size(Vec2::new(0.0, 30.0)))
}

/// A quiet, outlined button for secondary actions.
pub fn ghost_button(ui: &mut Ui, palette: &Palette, label: &str) -> Response {
    ui.add(egui::Button::new(RichText::new(label).color(palette.text)).fill(Color32::TRANSPARENT).stroke(Stroke::new(1.0, palette.line)).corner_radius(8.0))
}

/// A small coloured pill, e.g. a group name or a status label.
pub fn chip(ui: &mut Ui, text: &str, fg: Color32, bg: Color32) {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), egui::FontId::proportional(11.0), fg);
    let padding = Vec2::new(8.0, 3.0);
    let (rect, _) = ui.allocate_exact_size(galley.size() + padding * 2.0, Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(9), bg);
    ui.painter().galley(rect.min + padding, galley, fg);
}

/// A status dot (online / in-game / offline).
pub fn status_dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
}

/// A section heading with a thin rule beneath it.
pub fn section(ui: &mut Ui, palette: &Palette, title: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(title).size(13.0).strong().color(palette.muted));
    let rect = ui.available_rect_before_wrap();
    let y = ui.cursor().top() + 2.0;
    ui.painter().hline(rect.left()..=rect.right(), y, Stroke::new(1.0, palette.line));
    ui.add_space(8.0);
}

/// A labelled row for settings: label on the left, control on the right.
pub fn setting_row(ui: &mut Ui, palette: &Palette, label: &str, help: &str, control: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new(label).color(palette.text));
            if !help.is_empty() {
                ui.label(RichText::new(help).size(11.0).color(palette.faint));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
    });
    ui.add_space(6.0);
}

/// A rounded card container.
pub fn card(ui: &mut Ui, palette: &Palette, add: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(palette.surface)
        .stroke(Stroke::new(1.0, palette.line))
        .corner_radius(12.0)
        .inner_margin(egui::Margin::same(14))
        .show(ui, add);
}

/// Centered empty-state message.
pub fn empty_state(ui: &mut Ui, palette: &Palette, icon: &str, title: &str, hint: &str) {
    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(icon).size(44.0).color(palette.faint));
        ui.add_space(6.0);
        ui.label(RichText::new(title).size(16.0).strong().color(palette.muted));
        if !hint.is_empty() {
            ui.label(RichText::new(hint).size(12.0).color(palette.faint));
        }
    });
}
