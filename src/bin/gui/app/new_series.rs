use super::App;
use anime2ctxdb::Series;
use eframe::egui;
use std::path::PathBuf;

impl App {
    /// Shows the New series dialog when it is open. The user types a name. The series goes into
    /// a new folder with that name inside the series root, so it never writes into another folder.
    pub(super) fn new_series_window(&mut self, ctx: &egui::Context) {
        let Some(mut name) = self.new_series.take() else {
            return;
        };
        let mut open = true;
        let mut cancel = false;
        let mut create: Option<PathBuf> = None;
        egui::Window::new("New series")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Name");
                    ui.text_edit_singleline(&mut name);
                });

                let target = if name.trim().is_empty() {
                    Err("Type a name.".to_string())
                } else {
                    Series::new_dir(&self.series_root, &name).map_err(|error| format!("{error:#}"))
                };
                match &target {
                    Ok(dir) => ui.label(format!("The app makes: {}", dir.display())),
                    Err(message) => ui.colored_label(ui.visuals().error_fg_color, message),
                };
                ui.horizontal(|ui| {
                    let creatable = target.as_ref().ok();
                    if ui
                        .add_enabled(creatable.is_some(), egui::Button::new("Create"))
                        .clicked()
                    {
                        create = creatable.cloned();
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });

        if let Some(dir) = create {
            match Series::create(&dir) {
                Ok(series) => {
                    self.use_series(ctx, series);
                    return;
                }
                Err(error) => self.error = Some(format!("{error:#}")),
            }
        }
        if open && !cancel {
            self.new_series = Some(name);
        }
    }
}
