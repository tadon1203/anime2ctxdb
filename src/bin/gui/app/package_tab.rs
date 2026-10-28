use super::App;
use super::prompts_tab::text_area;
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

        // Start with the episode that the user picked in the Prompts tab.
        if self.package_episode.is_none() {
            self.package_episode = self
                .episode
                .filter(|episode| subtitle_episodes.contains(episode));
        }

        let copy_label = self.copy_label(&ctx);
        let (generate, copy) = ui
            .horizontal(|ui| {
                ui.label("Episode");
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
                let time =
                    ui.add(egui::TextEdit::singleline(&mut self.package_time).desired_width(80.0));
                let can_generate = !busy && self.package_episode.is_some();
                // Enter in the time field works like the Generate button.
                let enter = time.lost_focus()
                    && ui.input(|input| input.key_pressed(egui::Key::Enter))
                    && can_generate;
                let generate = ui
                    .add_enabled(can_generate, egui::Button::new("Generate"))
                    .on_disabled_hover_text("Pick an episode first.")
                    .clicked()
                    || enter;
                let copy = ui
                    .add_enabled(
                        !busy && self.package.is_some(),
                        egui::Button::new(copy_label),
                    )
                    .clicked();
                if let Some(message) = &self.package_time_error {
                    ui.colored_label(ui.visuals().error_fg_color, message);
                }
                (generate, copy)
            })
            .inner;
        ui.separator();

        if let Some(episode) = self.package_episode.filter(|_| generate) {
            match parse_clock(&self.package_time) {
                Ok(time_ms) => {
                    self.package_time_error = None;
                    self.create_package(&ctx, Position { episode, time_ms });
                }
                Err(error) => self.package_time_error = Some(format!("{error:#}")),
            }
        }

        if self.package.is_some() {
            if copy && let Some(text) = self.package.clone() {
                self.copy_text(&ctx, text, "Copied the package.");
            }
            if let Some(text) = &self.package {
                let mut shown = text.as_str();
                let height = ui.available_height();
                text_area(ui, "package-text", height, &mut shown, "");
            }
        } else {
            ui.weak("Pick an episode and a time, then generate the package. It holds what the story knows at that time.");
        }
    }
}
