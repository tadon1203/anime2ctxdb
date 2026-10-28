use super::App;
use anime2ctxdb::Series;
use eframe::egui;
use std::path::PathBuf;

impl App {
    /// The name field and the Create button. The series goes into a new folder with that name
    /// inside the series root, so it never writes into another folder. Returns true when the
    /// series was made and opened.
    pub(super) fn new_series_form(&mut self, ui: &mut egui::Ui) -> bool {
        let target = if self.new_series_name.trim().is_empty() {
            None
        } else {
            Some(
                Series::new_dir(&self.series_root, &self.new_series_name)
                    .map_err(|error| format!("{error:#}")),
            )
        };
        let creatable = target.as_ref().and_then(|target| target.as_ref().ok());

        let focus = std::mem::take(&mut self.new_series_focus);
        let mut create: Option<PathBuf> = None;
        ui.horizontal(|ui| {
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.new_series_name).hint_text("Series name"),
            );
            if focus {
                edit.request_focus();
            }
            let enter = edit.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            let click = ui
                .add_enabled(creatable.is_some(), egui::Button::new("Create"))
                .clicked();
            if click || enter {
                create = creatable.cloned();
            }
        });
        match &target {
            None => ui.weak("Type a name for the series."),
            Some(Ok(dir)) => ui.weak(format!("The app makes: {}", dir.display())),
            Some(Err(message)) => ui.colored_label(ui.visuals().error_fg_color, message),
        };

        let Some(dir) = create else {
            return false;
        };
        match Series::create(&dir) {
            Ok(series) => {
                self.new_series_name.clear();
                self.load_series_list();
                self.use_series(ui.ctx(), series);
                true
            }
            Err(error) => {
                self.error = Some(format!("{error:#}"));
                false
            }
        }
    }

    /// Shows the New series dialog when it is open.
    pub(super) fn new_series_window(&mut self, ctx: &egui::Context) {
        if !self.new_series_open {
            return;
        }
        let mut open = true;
        let mut close = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        egui::Window::new("New series")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                close |= self.new_series_form(ui);
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        self.new_series_open = open && !close;
    }
}
