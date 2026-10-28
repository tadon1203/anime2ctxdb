use super::{App, ClearTarget, JobOutput};
use anime2ctxdb::{Language, Source, SourceKind};
use eframe::egui;

const SUBTITLE_AND_VIDEO: [&str; 5] = ["ass", "mkv", "mp4", "webm", "avi"];

impl App {
    pub(super) fn sources_tab(&mut self, ui: &mut egui::Ui) {
        let busy = self.busy();
        let has_series = self.series.is_some();
        let ctx = ui.ctx().clone();

        let import = ui
            .add_enabled(!busy && has_series, egui::Button::new("Import files"))
            .clicked();
        if import {
            let picked = rfd::FileDialog::new()
                .add_filter("Subtitles and videos", &SUBTITLE_AND_VIDEO[..])
                .pick_files();
            if let (Some(paths), Some(series)) = (picked, self.series.clone()) {
                self.start(&ctx, move || {
                    series.read_files(&paths).map(JobOutput::Detected)
                });
            }
        }

        if !self.detected.is_empty() {
            ui.label("New files");
            let mut detected = std::mem::take(&mut self.detected);
            record_grid(ui, "detected", &mut detected, None);
            self.detected = detected;
            let save_all = ui.add_enabled(!busy, egui::Button::new("Save")).clicked();
            if save_all {
                let records = self.detected.clone();
                self.save(&ctx, records, Some(ClearTarget::Detected));
            }
        }

        ui.label("Saved sources");
        let mut saved = std::mem::take(&mut self.saved);
        let save_one = record_grid(ui, "saved", &mut saved, Some(!busy));
        self.saved = saved;
        if let Some(index) = save_one {
            let record = vec![self.saved[index].clone()];
            self.save(&ctx, record, None);
        }
    }
}

/// Shows the records in a grid. Returns the index of the row whose Save button was clicked.
fn record_grid(
    ui: &mut egui::Ui,
    salt: &str,
    records: &mut [Source],
    row_save: Option<bool>,
) -> Option<usize> {
    let mut clicked = None;
    egui::Grid::new(salt).striped(true).show(ui, |ui| {
        for heading in ["id", "kind", "path", "episode", "language"] {
            ui.label(heading);
        }
        if row_save.is_some() {
            ui.label("");
        }
        ui.end_row();
        for (index, record) in records.iter_mut().enumerate() {
            ui.label(&record.id.0[..8]);
            ui.label(kind_text(record.kind));
            ui.label(record.path.display().to_string());
            episode_combo(ui, &format!("{salt}-episode-{index}"), &mut record.episode);
            match record.kind {
                SourceKind::Subtitle => {
                    language_combo(
                        ui,
                        &format!("{salt}-language-{index}"),
                        &mut record.language,
                    );
                }
                SourceKind::Video => {
                    ui.label("-");
                }
            }
            if let Some(enabled) = row_save
                && ui.add_enabled(enabled, egui::Button::new("Save")).clicked()
            {
                clicked = Some(index);
            }
            ui.end_row();
        }
    });
    clicked
}

fn episode_combo(ui: &mut egui::Ui, salt: &str, episode: &mut Option<u32>) {
    let selected = match *episode {
        Some(number) => number.to_string(),
        None => "-".to_string(),
    };
    egui::ComboBox::from_id_salt(salt)
        .selected_text(selected)
        .show_ui(ui, |ui| {
            ui.selectable_value(episode, None, "-");
            for number in 1..=200 {
                ui.selectable_value(episode, Some(number), number.to_string());
            }
        });
}

fn language_combo(ui: &mut egui::Ui, salt: &str, language: &mut Option<Language>) {
    let selected = match *language {
        Some(language) => language_text(language),
        None => "-",
    };
    egui::ComboBox::from_id_salt(salt)
        .selected_text(selected)
        .show_ui(ui, |ui| {
            ui.selectable_value(language, None, "-");
            for option in [Language::Ja, Language::Zh, Language::En, Language::Other] {
                ui.selectable_value(language, Some(option), language_text(option));
            }
        });
}

fn kind_text(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::Subtitle => "subtitle",
        SourceKind::Video => "video",
    }
}

fn language_text(language: Language) -> &'static str {
    match language {
        Language::Ja => "Ja",
        Language::Zh => "Zh",
        Language::En => "En",
        Language::Other => "Other",
    }
}
