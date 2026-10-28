use super::App;
use anime2ctxdb::{Position, parse_clock};
use eframe::egui;

impl App {
    pub(super) fn package_tab(&mut self, ui: &mut egui::Ui) {
        let busy = self.busy();
        let ctx = ui.ctx().clone();
        let subtitle_episodes: Vec<u32> = self
            .progress
            .iter()
            .filter(|item| item.has_subtitle)
            .map(|item| item.episode)
            .collect();

        let generate = ui
            .horizontal(|ui| {
                let selected = self
                    .package_episode
                    .map_or("-".to_string(), |episode| episode.to_string());
                egui::ComboBox::from_id_salt("package-episode")
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        for episode in &subtitle_episodes {
                            ui.selectable_value(
                                &mut self.package_episode,
                                Some(*episode),
                                episode.to_string(),
                            );
                        }
                    });
                ui.label("Time (mm:ss)");
                ui.add(egui::TextEdit::singleline(&mut self.package_time).desired_width(80.0));
                ui.add_enabled(
                    !busy && self.package_episode.is_some(),
                    egui::Button::new("Generate"),
                )
                .clicked()
            })
            .inner;

        if let Some(episode) = self.package_episode.filter(|_| generate) {
            match parse_clock(&self.package_time) {
                Ok(time_ms) => self.create_package(&ctx, Position { episode, time_ms }),
                Err(error) => self.error = Some(format!("{error:#}")),
            }
        }

        if let Some(text) = &self.package {
            let mut shown = text.as_str();
            ui.add(
                egui::TextEdit::multiline(&mut shown)
                    .desired_rows(20)
                    .desired_width(f32::INFINITY),
            );
            if ui.add_enabled(!busy, egui::Button::new("Copy")).clicked() {
                ctx.copy_text(text.clone());
                self.status = "Copied the package.".to_string();
            }
        }
    }
}
