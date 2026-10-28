use super::App;
use anime2ctxdb::PromptKind;
use eframe::egui;

impl App {
    pub(super) fn prompts_tab(&mut self, ui: &mut egui::Ui) {
        let busy = self.busy();
        let ctx = ui.ctx().clone();

        egui::Grid::new("progress").striped(true).show(ui, |ui| {
            for heading in ["episode", "subtitle", "speakers", "facts"] {
                ui.label(heading);
            }
            ui.end_row();
            for item in &self.progress {
                ui.label(item.episode.to_string());
                ui.label(if item.has_subtitle { "yes" } else { "no" });
                let speakers = match item.unknown_lines {
                    None => "-".to_string(),
                    Some(0) => "done".to_string(),
                    Some(count) => format!("{count} lines unknown"),
                };
                ui.label(speakers);
                ui.label(item.facts.to_string());
                ui.end_row();
            }
        });

        let subtitle_episodes: Vec<u32> = self
            .progress
            .iter()
            .filter(|item| item.has_subtitle)
            .map(|item| item.episode)
            .collect();
        let (new_speakers, new_story) = ui
            .horizontal(|ui| {
                let selected = self
                    .episode
                    .map_or("-".to_string(), |episode| episode.to_string());
                egui::ComboBox::from_id_salt("prompt-episode")
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        for episode in &subtitle_episodes {
                            ui.selectable_value(
                                &mut self.episode,
                                Some(*episode),
                                episode.to_string(),
                            );
                        }
                    });
                let enabled = !busy && self.episode.is_some();
                let speakers = ui
                    .add_enabled(enabled, egui::Button::new("New Speakers prompt"))
                    .clicked();
                let story = ui
                    .add_enabled(enabled, egui::Button::new("New Story prompt"))
                    .clicked();
                (speakers, story)
            })
            .inner;

        let mut submit = false;
        let mut copy = false;
        if let Some(prompt) = &self.prompt {
            ui.label(format!("Prompt for episode {}", prompt.episode));
            let mut text = prompt.text.as_str();
            ui.add(
                egui::TextEdit::multiline(&mut text)
                    .desired_rows(12)
                    .desired_width(f32::INFINITY),
            );
            copy = ui.add_enabled(!busy, egui::Button::new("Copy")).clicked();
            ui.label("Answer");
            ui.add(
                egui::TextEdit::multiline(&mut self.answer)
                    .desired_rows(8)
                    .desired_width(f32::INFINITY),
            );
            submit = ui
                .add_enabled(
                    !busy && !self.answer.trim().is_empty(),
                    egui::Button::new("Submit"),
                )
                .clicked();
        }

        if let Some(prompt) = self.prompt.as_ref().filter(|_| copy) {
            ctx.copy_text(prompt.text.clone());
            self.status = "Copied the prompt.".to_string();
        }
        if let Some(episode) = self.episode.filter(|_| new_speakers) {
            self.create_prompt(&ctx, PromptKind::Speakers, episode);
        }
        if let Some(episode) = self.episode.filter(|_| new_story) {
            self.create_prompt(&ctx, PromptKind::Story, episode);
        }
        if let Some(prompt) = self
            .prompt
            .as_ref()
            .map(|prompt| prompt.id)
            .filter(|_| submit)
        {
            let answer = self.answer.clone();
            self.submit(&ctx, prompt, answer);
        }
    }
}
