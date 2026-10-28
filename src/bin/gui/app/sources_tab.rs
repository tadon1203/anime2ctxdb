use super::{App, ClearTarget, JobOutput};
use anime2ctxdb::{
    Detected, EpisodeOrigin, Language, Problem, Series, Severity, Source, SourceKind,
};
use eframe::egui;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

/// A detected file in the review list. The user can change it before the save.
#[derive(Debug)]
pub(super) struct DetectedRow {
    pub(super) source: Source,
    origin: EpisodeOrigin,
    /// False: the save leaves this file out.
    include: bool,
}

impl DetectedRow {
    pub(super) fn new(item: Detected) -> DetectedRow {
        DetectedRow {
            source: item.source,
            origin: item.origin,
            include: true,
        }
    }
}

impl App {
    pub(super) fn sources_tab(&mut self, ui: &mut egui::Ui) {
        let busy = self.busy();
        let can_import = !busy && self.series.is_some();
        let ctx = ui.ctx().clone();

        ui.horizontal(|ui| {
            if ui
                .add_enabled(can_import, egui::Button::new("Import files"))
                .clicked()
                && let Some(paths) = rfd::FileDialog::new()
                    .set_title("Import files")
                    .pick_files()
            {
                self.scan_paths(&ctx, paths);
            }
            if ui
                .add_enabled(can_import, egui::Button::new("Import folder"))
                .clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .set_title("Import folder")
                    .pick_folder()
            {
                self.scan_paths(&ctx, vec![path]);
            }
            ui.label("You can also drop files and folders here.");
        });

        if !self.detected.is_empty() || !self.failures.is_empty() {
            self.detected_section(ui, &ctx);
        }

        ui.label("Saved sources");
        let mut saved = std::mem::take(&mut self.saved);
        let save_one = saved_grid(ui, &mut saved, !busy);
        self.saved = saved;
        if let Some(index) = save_one {
            let record = vec![self.saved[index].clone()];
            self.save(&ctx, record, None);
        }
    }

    /// Reads files and folders on a worker thread.
    fn scan_paths(&mut self, ctx: &egui::Context, paths: Vec<PathBuf>) {
        let Some(series) = self.series.clone() else {
            return;
        };
        let progress = self.scan_progress.clone();
        let repaint = ctx.clone();
        self.start(ctx, move || {
            let scan = series.scan(&paths, |done, total| {
                progress.done.store(done, Ordering::Relaxed);
                progress.total.store(total, Ordering::Relaxed);
                repaint.request_repaint();
            })?;
            Ok(JobOutput::Scanned(scan))
        });
    }

    /// Reads the files that the user dropped on the window.
    pub(super) fn import_dropped(&mut self, ctx: &egui::Context, paths: Vec<PathBuf>) {
        if self.series.is_none() {
            self.status = "Open a series before you drop files.".to_string();
        } else if self.busy() {
            self.status = "Wait for the current job before you drop files.".to_string();
        } else {
            self.tab = super::Tab::Sources;
            self.scan_paths(ctx, paths);
        }
    }

    /// The list of the new files, with the problems of each row, and the Save button.
    fn detected_section(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let busy = self.busy();
        let included: Vec<usize> = (0..self.detected.len())
            .filter(|&index| self.detected[index].include)
            .collect();
        let pending: Vec<Source> = included
            .iter()
            .map(|&index| self.detected[index].source.clone())
            .collect();
        let problems = Series::check_sources(&self.saved, &pending);
        let mut by_row: Vec<Vec<&Problem>> = vec![Vec::new(); self.detected.len()];
        for problem in &problems {
            if let Some(row) = problem.row {
                by_row[included[row]].push(problem);
            }
        }
        let has_error = problems
            .iter()
            .any(|problem| problem.severity == Severity::Error);

        ui.label("New files");
        detected_grid(ui, &mut self.detected, &by_row);
        for problem in problems.iter().filter(|problem| problem.row.is_none()) {
            ui.colored_label(problem_color(ui, problem), &problem.message);
        }
        if !self.failures.is_empty() {
            ui.label("Failed files");
            for failure in &self.failures {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("{}: {}", failure.path.display(), failure.message),
                );
            }
        }

        ui.horizontal(|ui| {
            let can_save = !busy && !pending.is_empty() && !has_error;
            if ui
                .add_enabled(can_save, egui::Button::new("Save"))
                .clicked()
            {
                self.save(ctx, pending, Some(ClearTarget::Detected));
            }
            if ui
                .add_enabled(!busy, egui::Button::new("Discard all"))
                .clicked()
            {
                self.detected.clear();
                self.failures.clear();
            }
        });
    }
}

/// Shows the new files, grouped by episode. The files without an episode come first.
fn detected_grid(ui: &mut egui::Ui, rows: &mut [DetectedRow], problems: &[Vec<&Problem>]) {
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by_key(|&index| {
        let source = &rows[index].source;
        (
            source.episode,
            source.kind == SourceKind::Video,
            source.path.clone(),
        )
    });

    egui::Grid::new("detected").striped(true).show(ui, |ui| {
        for heading in [
            "",
            "kind",
            "file",
            "episode",
            "episode from",
            "language",
            "",
        ] {
            ui.label(heading);
        }
        ui.end_row();
        let mut group = None;
        for index in order {
            let row = &mut rows[index];
            if group != Some(row.source.episode) {
                group = Some(row.source.episode);
                ui.label("");
                ui.label("");
                match row.source.episode {
                    Some(number) => ui.strong(format!("Episode {number}")),
                    None => ui.strong("No episode"),
                };
                ui.end_row();
            }
            ui.checkbox(&mut row.include, "");
            ui.label(kind_text(row.source.kind));
            ui.label(row.source.path.display().to_string());
            episode_combo(
                ui,
                &format!("detected-episode-{index}"),
                &mut row.source.episode,
            );
            ui.label(origin_text(row.origin));
            match row.source.kind {
                SourceKind::Subtitle => language_combo(
                    ui,
                    &format!("detected-language-{index}"),
                    &mut row.source.language,
                ),
                SourceKind::Video => {
                    ui.label("-");
                }
            }
            ui.vertical(|ui| {
                for problem in &problems[index] {
                    ui.colored_label(problem_color(ui, problem), &problem.message);
                }
            });
            ui.end_row();
        }
    });
}

/// Shows the saved records in a grid. Returns the index of the row whose Save button was clicked.
fn saved_grid(ui: &mut egui::Ui, records: &mut [Source], enabled: bool) -> Option<usize> {
    let mut clicked = None;
    egui::Grid::new("saved").striped(true).show(ui, |ui| {
        for heading in ["id", "kind", "path", "episode", "language", ""] {
            ui.label(heading);
        }
        ui.end_row();
        for (index, record) in records.iter_mut().enumerate() {
            ui.label(&record.id.0[..8]);
            ui.label(kind_text(record.kind));
            ui.label(record.path.display().to_string());
            episode_combo(ui, &format!("saved-episode-{index}"), &mut record.episode);
            match record.kind {
                SourceKind::Subtitle => {
                    language_combo(ui, &format!("saved-language-{index}"), &mut record.language);
                }
                SourceKind::Video => {
                    ui.label("-");
                }
            }
            if ui.add_enabled(enabled, egui::Button::new("Save")).clicked() {
                clicked = Some(index);
            }
            ui.end_row();
        }
    });
    clicked
}

fn problem_color(ui: &egui::Ui, problem: &Problem) -> egui::Color32 {
    match problem.severity {
        Severity::Error => ui.visuals().error_fg_color,
        Severity::Warning => ui.visuals().warn_fg_color,
    }
}

fn origin_text(origin: EpisodeOrigin) -> &'static str {
    match origin {
        EpisodeOrigin::Saved => "saved",
        EpisodeOrigin::FileName => "file name",
        EpisodeOrigin::FolderName => "folder name",
        EpisodeOrigin::SiblingFile => "file with the same name",
        EpisodeOrigin::Unknown => "-",
    }
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
