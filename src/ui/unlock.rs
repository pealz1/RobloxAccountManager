//! A small window that asks for the vault password before the main app opens.

use crate::core::Core;
use crate::ui::theme;
use eframe::egui::{self, RichText};
use std::path::PathBuf;
use std::sync::Arc;

struct Unlock {
    data_dir: PathBuf,
    password: String,
    error: Option<String>,
    result: Arc<std::sync::Mutex<Option<Arc<Core>>>>,
    palette: theme::Palette,
}

impl eframe::App for Unlock {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(self.palette.bg).inner_margin(egui::Margin::same(24))).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(20.0);
                ui.label(RichText::new(egui_phosphor::regular::LOCK_KEY).size(40.0).color(self.palette.accent));
                ui.label(RichText::new("Nova RAM is locked").size(18.0).strong().color(self.palette.text));
                ui.label(RichText::new("Enter your vault password.").size(12.0).color(self.palette.muted));
                ui.add_space(14.0);
                let field =
                    ui.add(egui::TextEdit::singleline(&mut self.password).password(true).desired_width(240.0).hint_text("password"));
                field.request_focus();
                if let Some(error) = &self.error {
                    ui.add_space(6.0);
                    ui.label(RichText::new(error).size(12.0).color(self.palette.danger));
                }
                ui.add_space(12.0);
                let submit = ui.add(
                    egui::Button::new(RichText::new("Unlock").color(self.palette.accent_text).strong())
                        .fill(self.palette.accent)
                        .corner_radius(8.0)
                        .min_size(egui::vec2(240.0, 32.0)),
                );
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if submit.clicked() || enter {
                    match Core::open(&self.data_dir, Some(&self.password)) {
                        Ok(core) => {
                            *self.result.lock().unwrap_or_else(|p| p.into_inner()) = Some(core);
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        Err(err) => self.error = Some(err.message),
                    }
                }
                ui.add_space(8.0);
                if ui.button(RichText::new("Quit").color(self.palette.muted)).clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        });
    }
}

/// Shows the unlock window and returns the opened core, or None if the user quit.
pub fn prompt(data_dir: &std::path::Path) -> Option<Arc<Core>> {
    let shared: Arc<std::sync::Mutex<Option<Arc<Core>>>> = Arc::new(std::sync::Mutex::new(None));
    let result = Arc::clone(&shared);
    let data_dir = data_dir.to_path_buf();
    let viewport = egui::ViewportBuilder::default().with_inner_size([340.0, 320.0]).with_resizable(false).with_title("Nova RAM — Unlock");
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    let _ = eframe::run_native(
        "Nova RAM Unlock",
        options,
        Box::new(move |cc| {
            theme::install_fonts(&cc.egui_ctx);
            let palette = theme::palette(crate::store::settings::ThemeMode::Dark, "#5B8CFF", true);
            theme::apply(&cc.egui_ctx, &palette, 1.0);
            Ok(Box::new(Unlock { data_dir: data_dir.clone(), password: String::new(), error: None, result: Arc::clone(&result), palette })
                as Box<dyn eframe::App>)
        }),
    );

    shared.lock().unwrap_or_else(|p| p.into_inner()).clone()
}
