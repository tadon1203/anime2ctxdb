use super::{App, series_name};
use eframe::egui;
use std::path::PathBuf;

/// The width of the startup column. A wide window keeps the column in the middle.
const COLUMN_WIDTH: f32 = 560.0;

impl App {
    /// The first view: the list of series to open, and the form for a new series.
    pub(super) fn startup_view(&mut self, ui: &mut egui::Ui) {
        let busy = self.busy();
        let mut open: Option<PathBuf> = None;
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                let width = ui.available_width().min(COLUMN_WIDTH);
                let margin = (ui.available_width() - width) / 2.0;
                ui.horizontal_top(|ui| {
                    ui.add_space(margin);
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        ui.add_space(48.0);
                        ui.label(egui::RichText::new("anime2ctxdb").size(32.0).strong());
                        ui.weak("Open a series to continue, or make a new one.");

                        ui.add_space(32.0);
                        ui.strong("Series");
                        if self.series_list.is_empty() {
                            ui.weak("No series yet.");
                        }
                        for dir in &self.series_list {
                            // The empty right text pushes the name to the left edge.
                            let button = egui::Button::new(series_name(dir))
                                .right_text("")
                                .min_size(egui::vec2(width, 0.0));
                            if ui
                                .add_enabled(!busy, button)
                                .on_hover_text(dir.display().to_string())
                                .clicked()
                            {
                                open = Some(dir.clone());
                            }
                        }

                        ui.add_space(32.0);
                        ui.strong("New series");
                        self.new_series_form(ui);

                        ui.add_space(32.0);
                        ui.horizontal(|ui| {
                            ui.weak(format!("Series folder: {}", self.series_root.display()));
                            if ui.small_button("Copy path").clicked() {
                                ui.ctx().copy_text(self.series_root.display().to_string());
                            }
                        });
                    });
                });
            });
        if let Some(dir) = open {
            self.open_series(ui.ctx(), &dir);
        }
    }
}
