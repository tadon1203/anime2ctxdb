use super::{App, ClearTarget, JobOutput, confirm::Confirm, count_of};
use anime2ctxdb::{
    Detected, EpisodeOrigin, Language, Problem, Series, Severity, Source, SourceId, SourceKind,
};
use eframe::egui;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

/// The widths of the fixed columns, in points. The file column takes the rest of the window.
const CHECK_WIDTH: f32 = 28.0;
const ID_WIDTH: f32 = 90.0;
const KIND_WIDTH: f32 = 100.0;
const EPISODE_WIDTH: f32 = 90.0;
const ORIGIN_WIDTH: f32 = 220.0;
const LANGUAGE_WIDTH: f32 = 110.0;
const REMOVE_WIDTH: f32 = 110.0;

/// Why a Remove button is off.
const REMOVE_OFF: &str = "A job runs, or you have unsaved changes. Wait, save, or discard first.";

/// The narrowest the file column gets, in points.
const FILE_COLUMN_MIN_WIDTH: f32 = 200.0;

/// The file types that the import accepts.
const IMPORT_EXTENSIONS: [&str; 5] = ["ass", "mkv", "mp4", "webm", "avi"];

/// A detected file in the review list. The user can change it before the save.
#[derive(Debug)]
pub(super) struct DetectedRow {
    pub(super) source: Source,
    origin: EpisodeOrigin,
    /// False: the save leaves this file out.
    include: bool,
    /// The episode when the file was detected. The row stays in this group when the user
    /// changes its episode, so the row does not jump away while the user edits it.
    group: Option<u32>,
}

impl DetectedRow {
    pub(super) fn new(item: Detected) -> DetectedRow {
        DetectedRow {
            group: item.source.episode,
            source: item.source,
            origin: item.origin,
            include: true,
        }
    }
}

/// The state of the saved list that is not part of the records.
#[derive(Debug)]
pub(super) struct SavedView {
    /// The sources that the user ticked.
    checked: BTreeSet<SourceId>,
    /// The language that Set language gives to the ticked subtitles.
    language: Language,
    /// How many episodes Shift episodes moves the ticked sources.
    shift: i32,
}

impl Default for SavedView {
    fn default() -> SavedView {
        SavedView {
            checked: BTreeSet::new(),
            language: Language::Ja,
            shift: 1,
        }
    }
}

impl SavedView {
    /// Drops the ticks of the sources that are not in `saved` any more.
    pub(super) fn keep_saved(&mut self, saved: &[Source]) {
        self.checked
            .retain(|id| saved.iter().any(|record| &record.id == id));
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
                    .set_title("Choose subtitle or video files")
                    .add_filter("Subtitles and videos", &IMPORT_EXTENSIONS)
                    .pick_files()
            {
                self.scan_paths(&ctx, paths);
            }
            if ui
                .add_enabled(can_import, egui::Button::new("Import folder"))
                .clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .set_title("Choose a folder with subtitle or video files")
                    .pick_folder()
            {
                self.scan_paths(&ctx, vec![path]);
            }
            ui.weak("You can also drop files and folders on the window.");
        });

        let reviewing = !self.detected.is_empty() || !self.failures.is_empty();
        let changed = self.changed_rows();
        let can_remove = !busy && changed.is_empty();
        let pending: Vec<Source> = changed.iter().map(|&i| self.saved[i].clone()).collect();
        let problems = Series::check_sources(&self.saved_base, &pending);
        let mut by_row: Vec<Vec<Problem>> = vec![Vec::new(); self.saved.len()];
        let mut whole: Vec<Problem> = Vec::new();
        for problem in problems {
            match problem.row {
                Some(row) => by_row[changed[row]].push(problem),
                None => whole.push(problem),
            }
        }
        let mut remove: Option<Vec<SourceId>> = None;
        if reviewing {
            self.review_bar(ui, &ctx);
        }
        if !changed.is_empty() {
            self.changes_bar(ui, &ctx, &pending, &whole, &by_row);
        }
        ui.separator();
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                // A small margin on both sides keeps the list off the window edges.
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(12, 0))
                    .show(ui, |ui| {
                        if reviewing {
                            self.detected_section(ui);
                            ui.add_space(16.0);
                        }
                        ui.strong(format!("Saved sources ({})", self.saved.len()));
                        if self.saved.is_empty() {
                            ui.weak("No sources yet. Import subtitle files to start.");
                        } else {
                            for problem in &whole {
                                ui.colored_label(problem_color(ui, problem), &problem.message);
                            }
                            if !self.saved_view.checked.is_empty() {
                                remove = self.bulk_bar(ui, can_remove);
                            }
                            let grid = saved_grid(
                                ui,
                                &mut self.saved,
                                &self.saved_base,
                                &mut self.saved_view.checked,
                                &by_row,
                                can_remove,
                            );
                            if grid.is_some() {
                                remove = grid;
                            }
                        }
                    });
            });
        if let Some(ids) = remove {
            self.ask_remove(ids);
        }
    }

    /// The saved rows whose episode or language the user changed.
    fn changed_rows(&self) -> Vec<usize> {
        (0..self.saved.len())
            .filter(|&index| {
                let (now, base) = (&self.saved[index], &self.saved_base[index]);
                now.episode != base.episode || now.language != base.language
            })
            .collect()
    }

    /// The Save and Discard buttons for the changes in the saved list.
    fn changes_bar(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        pending: &[Source],
        whole: &[Problem],
        by_row: &[Vec<Problem>],
    ) {
        let busy = self.busy();
        let has_error = whole
            .iter()
            .chain(by_row.iter().flatten())
            .any(|problem| problem.severity == Severity::Error);
        ui.separator();
        ui.horizontal(|ui| {
            let why_not = if busy {
                Some("Wait for the current job to finish.")
            } else if has_error {
                Some("Fix the errors in the list first.")
            } else {
                None
            };
            let save = format!("Save {}", count_of(pending.len(), "change"));
            let button = ui.add_enabled(why_not.is_none(), egui::Button::new(save));
            if let Some(reason) = why_not {
                button.on_disabled_hover_text(reason);
            } else if button.clicked() {
                self.save(ctx, pending.to_vec(), None);
            }
            if ui
                .add_enabled(!busy, egui::Button::new("Discard changes"))
                .clicked()
            {
                self.saved.clone_from(&self.saved_base);
            }
        });
    }

    /// The actions for the ticked sources. Returns the sources that the user wants to remove.
    fn bulk_bar(&mut self, ui: &mut egui::Ui, can_remove: bool) -> Option<Vec<SourceId>> {
        let mut remove = None;
        let view = &mut self.saved_view;
        let below_one = self
            .saved
            .iter()
            .filter(|record| view.checked.contains(&record.id))
            .filter_map(|record| record.episode)
            .any(|episode| i64::from(episode) + i64::from(view.shift) < 1);
        ui.horizontal(|ui| {
            ui.label(format!("{} ticked.", view.checked.len()));
            egui::ComboBox::from_id_salt("bulk-language")
                .selected_text(language_text(view.language))
                .show_ui(ui, |ui| {
                    for option in [Language::Ja, Language::Zh, Language::En, Language::Other] {
                        ui.selectable_value(&mut view.language, option, language_text(option));
                    }
                });
            if ui.button("Set language").clicked() {
                for record in self
                    .saved
                    .iter_mut()
                    .filter(|record| view.checked.contains(&record.id))
                    .filter(|record| record.kind == SourceKind::Subtitle)
                {
                    record.language = Some(view.language);
                }
            }
            ui.add(
                egui::DragValue::new(&mut view.shift)
                    .range(-99..=99)
                    .speed(0.1),
            );
            let shift = ui.add_enabled(!below_one, egui::Button::new("Shift episodes"));
            if below_one {
                shift.on_disabled_hover_text("An episode cannot go below 1.");
            } else if shift.clicked() {
                for record in self
                    .saved
                    .iter_mut()
                    .filter(|record| view.checked.contains(&record.id))
                {
                    record.episode = record
                        .episode
                        .map(|episode| episode.saturating_add_signed(view.shift));
                }
            }
            let button = ui.add_enabled(can_remove, egui::Button::new("Remove ticked"));
            if can_remove {
                if button.clicked() {
                    remove = Some(view.checked.iter().cloned().collect());
                }
            } else {
                button.on_disabled_hover_text(REMOVE_OFF);
            }
            if ui.button("Clear ticks").clicked() {
                view.checked.clear();
            }
        });
        remove
    }

    /// Asks the user before it removes sources. A source that an answer uses stays.
    fn ask_remove(&mut self, ids: Vec<SourceId>) {
        let Some(series) = &self.series else {
            return;
        };
        match series.blocked_removals(&ids) {
            Ok(blocked) if blocked.len() == ids.len() => {
                self.status = "Nothing to remove: an answer uses every one of them.".to_string();
            }
            Ok(blocked) => self.confirm = Some(Confirm::Remove { ids, blocked }),
            Err(error) => self.error = Some(format!("{error:#}")),
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

    /// The files that the save would add. The saved sources and the problems use the same list.
    fn pending(&self) -> (Vec<usize>, Vec<Source>) {
        let included: Vec<usize> = (0..self.detected.len())
            .filter(|&index| self.detected[index].include)
            .collect();
        let pending = included
            .iter()
            .map(|&index| self.detected[index].source.clone())
            .collect();
        (included, pending)
    }

    /// The Save and Discard buttons for the new files. It stays above the list, so the user
    /// does not scroll to find it.
    fn review_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let busy = self.busy();
        let (_, pending) = self.pending();
        let problems = Series::check_sources(&self.saved, &pending);
        let has_error = problems
            .iter()
            .any(|problem| problem.severity == Severity::Error);

        ui.separator();
        ui.horizontal(|ui| {
            let why_not = if busy {
                Some("Wait for the current job to finish.")
            } else if pending.is_empty() {
                Some("Tick at least one file.")
            } else if has_error {
                Some("Fix the errors in the list first.")
            } else {
                None
            };
            let save = format!("Save {}", count_of(pending.len(), "file"));
            let button = ui.add_enabled(why_not.is_none(), egui::Button::new(save));
            if let Some(reason) = why_not {
                button.on_disabled_hover_text(reason);
            } else if button.clicked() {
                self.save(ctx, pending.clone(), Some(ClearTarget::Detected));
            }
            if ui
                .add_enabled(!busy, egui::Button::new("Discard all"))
                .clicked()
            {
                if self.detected.is_empty() {
                    self.failures.clear();
                } else {
                    self.confirm = Some(Confirm::DiscardNew);
                }
            }
        });
    }

    /// The list of the new files, with the problems of each row, and the files that failed.
    fn detected_section(&mut self, ui: &mut egui::Ui) {
        let (included, pending) = self.pending();
        let problems = Series::check_sources(&self.saved, &pending);
        let mut by_row: Vec<Vec<&Problem>> = vec![Vec::new(); self.detected.len()];
        for problem in &problems {
            if let Some(row) = problem.row {
                by_row[included[row]].push(problem);
            }
        }

        ui.strong(format!("New files ({})", self.detected.len()));
        for problem in problems.iter().filter(|problem| problem.row.is_none()) {
            ui.colored_label(problem_color(ui, problem), &problem.message);
        }
        if !self.detected.is_empty() {
            detected_grid(ui, &mut self.detected, &by_row);
        }
        if !self.failures.is_empty() {
            ui.add_space(16.0);
            ui.strong(format!("Failed files ({})", self.failures.len()));
            ui.weak("The app could not read these files.");
            for failure in &self.failures {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("{}: {}", failure.path.display(), failure.message),
                );
            }
        }
    }
}

/// Shows the new files, grouped by episode. The files without an episode come first.
fn detected_grid(ui: &mut egui::Ui, rows: &mut [DetectedRow], problems: &[Vec<&Problem>]) {
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by_key(|&index| {
        let row = &rows[index];
        (
            row.group,
            row.source.kind == SourceKind::Video,
            row.source.path.clone(),
        )
    });
    let mut groups: Vec<(Option<u32>, Vec<usize>)> = Vec::new();
    for index in order {
        match groups.last_mut() {
            Some((group, members)) if *group == rows[index].group => members.push(index),
            _ => groups.push((rows[index].group, vec![index])),
        }
    }
    let file_width = file_column_width(
        ui,
        &[
            CHECK_WIDTH,
            KIND_WIDTH,
            EPISODE_WIDTH,
            ORIGIN_WIDTH,
            LANGUAGE_WIDTH,
        ],
    );

    egui::Grid::new("detected").striped(true).show(ui, |ui| {
        for heading in ["", "Kind", "File", "Episode", "Episode from", "Language"] {
            ui.weak(heading);
        }
        ui.end_row();
        for (group, members) in &groups {
            // The box in the group row ticks or clears every file of the group.
            let mut all = members.iter().all(|&index| rows[index].include);
            let toggled = fixed_cell(ui, CHECK_WIDTH, |ui| ui.checkbox(&mut all, "")).changed();
            if toggled {
                for &index in members {
                    rows[index].include = all;
                }
            }
            ui.label("");
            match group {
                Some(number) => ui.strong(format!("Episode {number}")),
                None => ui.strong("No episode"),
            };
            ui.end_row();
            for &index in members {
                let row = &mut rows[index];
                fixed_cell(ui, CHECK_WIDTH, |ui| ui.checkbox(&mut row.include, ""));
                fixed_cell(ui, KIND_WIDTH, |ui| ui.label(kind_text(row.source.kind)));
                file_cell(ui, file_width, &row.source.path, &problems[index]);
                fixed_cell(ui, EPISODE_WIDTH, |ui| {
                    episode_input(ui, &mut row.source.episode);
                });
                fixed_cell(ui, ORIGIN_WIDTH, |ui| ui.label(origin_text(row.origin)));
                fixed_cell(ui, LANGUAGE_WIDTH, |ui| match row.source.kind {
                    SourceKind::Subtitle => language_combo(
                        ui,
                        &format!("detected-language-{index}"),
                        &mut row.source.language,
                    ),
                    SourceKind::Video => {
                        ui.label("-");
                    }
                });
                ui.end_row();
            }
        }
    });
}

/// The indexes of the saved records, grouped by the episode on disk. The group without an
/// episode comes first. A video comes after the subtitles of its group.
fn saved_groups(records: &[Source], base: &[Source]) -> Vec<(Option<u32>, Vec<usize>)> {
    let mut order: Vec<usize> = (0..records.len()).collect();
    order.sort_by_key(|&index| {
        (
            base[index].episode,
            records[index].kind == SourceKind::Video,
            records[index].path.clone(),
        )
    });
    let mut groups: Vec<(Option<u32>, Vec<usize>)> = Vec::new();
    for index in order {
        match groups.last_mut() {
            Some((group, members)) if *group == base[index].episode => members.push(index),
            _ => groups.push((base[index].episode, vec![index])),
        }
    }
    groups
}

/// Shows the saved records in a grid, grouped by episode. The files without an episode come
/// first. `base` holds the records as they are on disk. A row stays in the group of its saved
/// episode while the user edits it. `checked` holds the ticked sources. `problems` has the
/// problems of each row. Returns the sources that the user wants to remove.
fn saved_grid(
    ui: &mut egui::Ui,
    records: &mut [Source],
    base: &[Source],
    checked: &mut BTreeSet<SourceId>,
    problems: &[Vec<Problem>],
    can_remove: bool,
) -> Option<Vec<SourceId>> {
    let mut remove = None;
    let groups = saved_groups(records, base);
    let file_width = file_column_width(
        ui,
        &[
            CHECK_WIDTH,
            ID_WIDTH,
            KIND_WIDTH,
            EPISODE_WIDTH,
            LANGUAGE_WIDTH,
            REMOVE_WIDTH,
        ],
    );
    egui::Grid::new("saved").striped(true).show(ui, |ui| {
        for heading in ["", "Id", "Kind", "File", "Episode", "Language", ""] {
            ui.weak(heading);
        }
        ui.end_row();
        for (group, members) in &groups {
            // The box in the group row ticks or clears every source of the group.
            let mut all = members
                .iter()
                .all(|&index| checked.contains(&records[index].id));
            if fixed_cell(ui, CHECK_WIDTH, |ui| ui.checkbox(&mut all, "")).changed() {
                for &index in members {
                    set_checked(checked, &records[index].id, all);
                }
            }
            ui.label("");
            ui.label("");
            let title = match group {
                Some(number) => format!("Episode {number}"),
                None => "No episode".to_string(),
            };
            ui.strong(title);
            ui.label("");
            ui.label("");
            let remove_all = fixed_cell(ui, REMOVE_WIDTH, |ui| {
                ui.add_enabled(can_remove, egui::Button::new("Remove all"))
                    .on_disabled_hover_text(REMOVE_OFF)
                    .clicked()
            });
            if remove_all {
                remove = Some(
                    members
                        .iter()
                        .map(|&index| records[index].id.clone())
                        .collect(),
                );
            }
            ui.end_row();
            for &index in members {
                let record = &mut records[index];
                let mut on = checked.contains(&record.id);
                if fixed_cell(ui, CHECK_WIDTH, |ui| ui.checkbox(&mut on, "")).changed() {
                    set_checked(checked, &record.id, on);
                }
                fixed_cell(ui, ID_WIDTH, |ui| ui.monospace(&record.id.0[..8]));
                fixed_cell(ui, KIND_WIDTH, |ui| ui.label(kind_text(record.kind)));
                let row_problems: Vec<&Problem> = problems[index].iter().collect();
                file_cell(ui, file_width, &record.path, &row_problems);
                fixed_cell(ui, EPISODE_WIDTH, |ui| {
                    episode_input(ui, &mut record.episode);
                });
                fixed_cell(ui, LANGUAGE_WIDTH, |ui| match record.kind {
                    SourceKind::Subtitle => {
                        language_combo(
                            ui,
                            &format!("saved-language-{index}"),
                            &mut record.language,
                        );
                    }
                    SourceKind::Video => {
                        ui.label("-");
                    }
                });
                let remove_one = fixed_cell(ui, REMOVE_WIDTH, |ui| {
                    ui.add_enabled(can_remove, egui::Button::new("Remove"))
                        .on_disabled_hover_text(REMOVE_OFF)
                        .clicked()
                });
                if remove_one {
                    remove = Some(vec![record.id.clone()]);
                }
                ui.end_row();
            }
        }
    });
    remove
}

fn set_checked(checked: &mut BTreeSet<SourceId>, id: &SourceId, on: bool) {
    if on {
        checked.insert(id.clone());
    } else {
        checked.remove(id);
    }
}

/// The width of the file column. The fixed columns and the gaps between the columns take
/// their width first. The file column gets the rest, so the grid fills the window.
fn file_column_width(ui: &egui::Ui, fixed: &[f32]) -> f32 {
    let spacing = ui.spacing().item_spacing.x;
    let gaps: f32 = fixed.iter().map(|_| spacing).sum();
    (ui.available_width() - fixed.iter().sum::<f32>() - gaps).max(FILE_COLUMN_MIN_WIDTH)
}

/// Runs `add` in a cell that is exactly `width` wide. The grid uses this width for the column.
fn fixed_cell<R>(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.vertical(|ui| {
        ui.set_width(width);
        add(ui)
    })
    .inner
}

/// Shows the file name in bold, the folder below it, and the problems of the row.
/// The text wraps at `width`. The full path shows when the pointer is on the name.
fn file_cell(ui: &mut egui::Ui, width: f32, path: &Path, problems: &[&Problem]) {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.display().to_string(),
    );
    ui.vertical(|ui| {
        ui.set_width(width);
        ui.add(egui::Label::new(egui::RichText::new(name).strong()).wrap())
            .on_hover_text(path.display().to_string());
        if let Some(folder) = path.parent() {
            let folder = folder.display().to_string();
            ui.add(egui::Label::new(egui::RichText::new(folder).small().weak()).wrap());
        }
        for problem in problems {
            let text = egui::RichText::new(&problem.message).color(problem_color(ui, problem));
            ui.add(egui::Label::new(text).wrap());
        }
    });
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

/// An episode number that the user types or drags. 0 and `-` mean no episode.
fn episode_input(ui: &mut egui::Ui, episode: &mut Option<u32>) {
    let mut value = episode.unwrap_or(0);
    let input = egui::DragValue::new(&mut value)
        .range(0..=999)
        .speed(0.1)
        .custom_formatter(|number, _| {
            if number == 0.0 {
                "-".to_string()
            } else {
                format!("{number:.0}")
            }
        })
        .custom_parser(|text| {
            let text = text.trim();
            if text.is_empty() || text == "-" {
                Some(0.0)
            } else {
                text.parse().ok()
            }
        });
    ui.add_sized([ui.available_width(), ui.spacing().interact_size.y], input);
    *episode = (value != 0).then_some(value);
}

fn language_combo(ui: &mut egui::Ui, salt: &str, language: &mut Option<Language>) {
    let selected = match *language {
        Some(language) => language_text(language),
        None => "-",
    };
    egui::ComboBox::from_id_salt(salt)
        .selected_text(selected)
        .width(ui.available_width())
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
        Language::Ja => "Japanese",
        Language::Zh => "Chinese",
        Language::En => "English",
        Language::Other => "Other",
    }
}
