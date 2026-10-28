mod confirm;
mod new_series;
mod package_tab;
mod prompts_tab;
mod sources_tab;
mod startup;
mod suggestions_tab;

use anime2ctxdb::{
    Action, EpisodeProgress, Failure, Position, Prompt, PromptId, PromptKind, Scan, Series, Source,
    SourceId, Status, SuggestionFilter, SuggestionId, SuggestionRow,
};
use anyhow::Result;
use eframe::egui;
use sources_tab::{DetectedRow, SavedView};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

/// How long a status message stays in the status bar.
const STATUS_LIFETIME: Duration = Duration::from_secs(8);

/// How long a Copy button shows that it copied.
const COPIED_LIFETIME: Duration = Duration::from_millis(1500);

/// The result of a job that runs on a worker thread.
enum JobOutput {
    Scanned(Scan),
    Prompt(Prompt),
    Package(String),
    Status(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Sources,
    Suggestions,
    Prompts,
    Package,
}

/// What a finished job clears from the view.
#[derive(Clone, Copy)]
enum ClearTarget {
    Detected,
    Answer,
}

/// How far a scan is. A worker thread writes it. The UI reads it.
#[derive(Debug, Default)]
struct ScanProgress {
    done: AtomicUsize,
    total: AtomicUsize,
}

pub struct App {
    /// The directory that holds all series. It is next to the executable.
    series_root: PathBuf,
    /// The series directories in `series_root`.
    series_list: Vec<PathBuf>,
    /// The name that the user types for a new series.
    new_series_name: String,
    /// True: the New series dialog is open.
    new_series_open: bool,
    /// True: the name field takes the focus when it shows next.
    new_series_focus: bool,
    /// The question that waits for a yes, before the app throws away or changes much work.
    confirm: Option<confirm::Confirm>,
    series: Option<Series>,
    tab: Tab,
    error: Option<String>,
    status: String,
    /// The status that the timer below belongs to.
    shown_status: String,
    status_since: Instant,
    /// When the user last copied text. The Copy buttons show it for a short time.
    copied_at: Option<Instant>,
    job: Option<Receiver<Result<JobOutput>>>,
    scan_progress: Arc<ScanProgress>,
    clear_on_success: Option<ClearTarget>,
    detected: Vec<DetectedRow>,
    failures: Vec<Failure>,
    saved: Vec<Source>,
    /// The saved sources as they are on disk. A row that differs from it has unsaved changes.
    saved_base: Vec<Source>,
    saved_view: SavedView,
    filter: SuggestionFilter,
    rows: Vec<SuggestionRow>,
    progress: Vec<EpisodeProgress>,
    /// The episode that the user picked last. The range selection starts from it.
    episode: Option<u32>,
    /// Every episode that is selected in the Prompts tab.
    selected: BTreeSet<u32>,
    prompt: Option<Prompt>,
    answer: String,
    package_episode: Option<u32>,
    package_time: String,
    /// Why the time in `package_time` is not valid. It shows next to the field.
    package_time_error: Option<String>,
    package: Option<String>,
}

impl App {
    pub fn new(series_root: PathBuf) -> Self {
        let mut app = App {
            series_root,
            series_list: Vec::new(),
            new_series_name: String::new(),
            new_series_open: false,
            new_series_focus: true,
            confirm: None,
            series: None,
            tab: Tab::Sources,
            error: None,
            status: String::new(),
            shown_status: String::new(),
            status_since: Instant::now(),
            copied_at: None,
            job: None,
            scan_progress: Arc::default(),
            clear_on_success: None,
            detected: Vec::new(),
            failures: Vec::new(),
            saved: Vec::new(),
            saved_base: Vec::new(),
            saved_view: SavedView::default(),
            filter: SuggestionFilter {
                status: Some(Status::Pending),
                kind: None,
            },
            rows: Vec::new(),
            progress: Vec::new(),
            episode: None,
            selected: BTreeSet::new(),
            prompt: None,
            answer: String::new(),
            package_episode: None,
            package_time: "00:00".to_string(),
            package_time_error: None,
            package: None,
        };
        app.load_series_list();
        app
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll();
        let ctx = ui.ctx().clone();
        let dropped: Vec<PathBuf> = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .filter(|path| !path.as_os_str().is_empty())
                .collect()
        });
        if !dropped.is_empty() {
            self.import_dropped(&ctx, dropped);
        }
        // The panels at the edges stay in place. Only the content of the central panel scrolls.
        if self.series.is_some() {
            egui::Panel::top("header").show(ui, |ui| self.header(ui));
        }
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        if self.error.is_some() {
            egui::Panel::top("error").show(ui, |ui| self.error_bar(ui));
        }
        egui::CentralPanel::default().show(ui, |ui| self.body(ui));
        self.new_series_window(&ctx);
        self.confirm_window(&ctx);
        self.drop_overlay(&ctx);
        self.expire_status(&ctx);
    }
}

/// The Japanese and Chinese font. It is part of the executable.
static CJK_FONT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/NotoSansCJK.ttc"
));

/// Adds the Japanese and Chinese font after the default fonts. The default fonts have no such
/// glyphs, and subtitles need them.
pub fn set_fonts(ctx: &egui::Context) {
    let bytes = CJK_FONT;
    let mut fonts = egui::FontDefinitions::default();
    for (family, name) in [
        (egui::FontFamily::Proportional, "cjk-proportional"),
        (egui::FontFamily::Monospace, "cjk-monospace"),
    ] {
        // The offset depends on the first font of the family, so each family has its own copy.
        let offset = fonts
            .families
            .get(&family)
            .and_then(|list| list.first())
            .and_then(|first| fonts.font_data.get(first))
            .and_then(|first| baseline_offset(first.font.as_ref().as_ref(), bytes));
        let mut data = egui::FontData::from_static(bytes);
        data.tweak.y_offset_factor = offset.unwrap_or(0.0);
        fonts.font_data.insert(name.to_string(), Arc::new(data));
        if let Some(list) = fonts.families.get_mut(&family) {
            list.push(name.to_string());
        }
    }
    ctx.set_fonts(fonts);
}

/// How far down, as a fraction of the font size, the glyphs of `fallback` must move to stand on
/// the baseline of `primary`. egui centers the box of a fallback font in the box of the
/// primary font. The two boxes differ in height, so the baselines do not match without this.
fn baseline_offset(primary: &[u8], fallback: &[u8]) -> Option<f32> {
    use skrifa::instance::{LocationRef, Size};
    use skrifa::{FontRef, MetadataProvider};

    // Ascent, and the height of the box (ascent, descent, and line gap), in em.
    let measure = |data: &[u8]| {
        let font = FontRef::from_index(data, 0).ok()?;
        let metrics = font.metrics(Size::unscaled(), LocationRef::default());
        let em = f32::from(metrics.units_per_em);
        let height = metrics.ascent - metrics.descent + metrics.leading;
        Some((metrics.ascent / em, height / em))
    };
    let (primary_ascent, primary_height) = measure(primary)?;
    let (fallback_ascent, fallback_height) = measure(fallback)?;
    Some(primary_ascent - fallback_ascent - 0.5 * (primary_height - fallback_height))
}

/// Names a count: `1 file`, `2 files`.
fn count_of(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("{count} {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

/// Sets the text sizes, the spacing, and a flat look for the dark and the light theme.
/// The default sizes are too small to read and to click.
pub fn set_style(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        use egui::{FontFamily::Monospace, FontFamily::Proportional, FontId, TextStyle, vec2};
        let radius = egui::CornerRadius::same(2);
        let visuals = &mut style.visuals;
        visuals.window_corner_radius = radius;
        visuals.menu_corner_radius = radius;
        visuals.window_shadow = egui::Shadow::NONE;
        visuals.popup_shadow = egui::Shadow::NONE;
        for widget in [
            &mut visuals.widgets.noninteractive,
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widget.corner_radius = radius;
        }
        style.spacing.item_spacing = vec2(12.0, 8.0);
        style.spacing.button_padding = vec2(12.0, 6.0);
        style.spacing.interact_size = vec2(48.0, 30.0);
        style.spacing.indent = 24.0;
        style
            .text_styles
            .insert(TextStyle::Small, FontId::new(14.0, Proportional));
        style
            .text_styles
            .insert(TextStyle::Body, FontId::new(16.0, Proportional));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::new(16.0, Proportional));
        style
            .text_styles
            .insert(TextStyle::Heading, FontId::new(22.0, Proportional));
        style
            .text_styles
            .insert(TextStyle::Monospace, FontId::new(16.0, Monospace));
    });
}

impl App {
    fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// Runs a slow call on a new thread. The result comes back through the channel.
    fn start(
        &mut self,
        ctx: &egui::Context,
        work: impl FnOnce() -> Result<JobOutput> + Send + 'static,
    ) {
        let (sender, receiver) = mpsc::channel();
        let repaint = ctx.clone();
        thread::spawn(move || {
            let result = work();
            // The receiver is gone only when the app closed, so the result is not needed then.
            let _ = sender.send(result);
            repaint.request_repaint();
        });
        self.job = Some(receiver);
    }

    /// Takes the result of a finished job. A panicked worker closes the channel, so the UI panics too.
    fn poll(&mut self) {
        let Some(receiver) = &self.job else {
            return;
        };
        match receiver.try_recv() {
            Ok(result) => {
                self.job = None;
                self.scan_progress.done.store(0, Ordering::Relaxed);
                self.scan_progress.total.store(0, Ordering::Relaxed);
                self.finish(result);
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => panic!("the worker thread panicked"),
        }
    }

    fn finish(&mut self, result: Result<JobOutput>) {
        let clear = self.clear_on_success.take();
        match result {
            Ok(JobOutput::Scanned(scan)) => {
                self.status = format!(
                    "Detected {}, skipped {}, failed {}.",
                    count_of(scan.detected.len(), "file"),
                    scan.skipped,
                    scan.failed.len()
                );
                for item in scan.detected {
                    if !self
                        .detected
                        .iter()
                        .any(|row| row.source.id == item.source.id)
                    {
                        self.detected.push(DetectedRow::new(item));
                    }
                }
                self.failures.extend(scan.failed);
            }
            Ok(JobOutput::Prompt(prompt)) => {
                self.status = "Prompt made. Copy it to an AI chat.".to_string();
                self.prompt = Some(prompt);
            }
            Ok(JobOutput::Package(text)) => {
                self.status = "Package made. Copy it to an AI chat.".to_string();
                self.package = Some(text);
            }
            Ok(JobOutput::Status(message)) => {
                match clear {
                    Some(ClearTarget::Detected) => {
                        self.detected.clear();
                        self.failures.clear();
                    }
                    Some(ClearTarget::Answer) => self.answer.clear(),
                    None => {}
                }
                self.status = message;
            }
            Err(error) => self.error = Some(format!("{error:#}")),
        }
        self.reload();
    }

    /// Reads the views again on the UI thread.
    fn reload(&mut self) {
        let Some(series) = &self.series else {
            return;
        };
        match load_views(series, self.filter) {
            Ok((saved, rows, progress)) => {
                self.saved_base.clone_from(&saved);
                self.saved_view.keep_saved(&saved);
                self.saved = saved;
                self.rows = rows;
                self.progress = progress;
                if self.episode.is_none() {
                    self.episode = first_unfinished(&self.progress);
                    self.selected.extend(self.episode);
                }
            }
            Err(error) => self.error = Some(format!("{error:#}")),
        }
    }

    /// Reads the series directories in the series root again.
    fn load_series_list(&mut self) {
        match Series::list(&self.series_root) {
            Ok(list) => self.series_list = list,
            Err(error) => self.error = Some(format!("{error:#}")),
        }
    }

    fn open_series(&mut self, ctx: &egui::Context, dir: &Path) {
        match Series::open(dir) {
            Ok(series) => self.use_series(ctx, series),
            Err(error) => self.error = Some(format!("{error:#}")),
        }
    }

    fn use_series(&mut self, ctx: &egui::Context, series: Series) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "anime2ctxdb - {}",
            series_name(series.dir())
        )));
        self.series = Some(series);
        self.detected.clear();
        self.failures.clear();
        self.saved_view = SavedView::default();
        self.prompt = None;
        self.answer.clear();
        self.episode = None;
        self.selected.clear();
        self.package_episode = None;
        self.package_time_error = None;
        self.package = None;
        self.status = "Series opened.".to_string();
        self.reload();
    }

    /// Goes back to the start view.
    fn close_series(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Title("anime2ctxdb".to_string()));
        self.series = None;
        self.detected.clear();
        self.failures.clear();
        self.saved.clear();
        self.saved_base.clear();
        self.rows.clear();
        self.progress.clear();
        self.status.clear();
        self.load_series_list();
    }

    /// Opens another series, or goes to the start view when `target` is `None`. New files that
    /// are not saved would be lost, so the user has to agree first.
    fn leave_series(&mut self, ctx: &egui::Context, target: Option<PathBuf>) {
        if self.detected.is_empty() {
            self.go_to(ctx, target);
        } else {
            self.confirm = Some(confirm::Confirm::Leave(target));
        }
    }

    fn go_to(&mut self, ctx: &egui::Context, target: Option<PathBuf>) {
        match target {
            Some(dir) => self.open_series(ctx, &dir),
            None => self.close_series(ctx),
        }
    }

    /// Clears the status after a while, so an old result does not look new.
    fn expire_status(&mut self, ctx: &egui::Context) {
        if self.status != self.shown_status {
            self.shown_status.clone_from(&self.status);
            self.status_since = Instant::now();
        }
        if self.status.is_empty() {
            return;
        }
        let age = self.status_since.elapsed();
        if age >= STATUS_LIFETIME {
            self.status.clear();
            self.shown_status.clear();
        } else {
            ctx.request_repaint_after(STATUS_LIFETIME.saturating_sub(age));
        }
    }

    /// Puts text on the clipboard. The Copy buttons show it for a short time.
    fn copy_text(&mut self, ctx: &egui::Context, text: String, status: &str) {
        ctx.copy_text(text);
        self.copied_at = Some(Instant::now());
        self.status = status.to_string();
    }

    /// The label of a Copy button: `Copied` for a short time after a copy.
    fn copy_label(&self, ctx: &egui::Context) -> &'static str {
        match self.copied_at.map(|at| at.elapsed()) {
            Some(age) if age < COPIED_LIFETIME => {
                ctx.request_repaint_after(COPIED_LIFETIME.saturating_sub(age));
                "Copied"
            }
            _ => "Copy",
        }
    }

    /// Dims the window and says what happens, while the user holds files over it.
    fn drop_overlay(&self, ctx: &egui::Context) {
        if ctx.input(|input| input.raw.hovered_files.is_empty()) {
            return;
        }
        let text = if self.series.is_none() {
            "Open a series before you drop files"
        } else if self.busy() {
            "Wait for the current job to finish"
        } else {
            "Drop files or folders to import them"
        };
        let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("drop-overlay"));
        let painter = ctx.layer_painter(layer);
        let rect = ctx.content_rect();
        painter.rect_filled(rect, 0.0, egui::Color32::from_black_alpha(170));
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(24.0),
            egui::Color32::WHITE,
        );
    }

    fn save(&mut self, ctx: &egui::Context, records: Vec<Source>, clear: Option<ClearTarget>) {
        let Some(series) = self.series.clone() else {
            return;
        };
        self.clear_on_success = clear;
        self.start(ctx, move || {
            series.add_sources(&records)?;
            Ok(JobOutput::Status(format!(
                "Saved {}.",
                count_of(records.len(), "source")
            )))
        });
    }

    fn remove_sources(&mut self, ctx: &egui::Context, ids: Vec<SourceId>) {
        let Some(series) = self.series.clone() else {
            return;
        };
        self.start(ctx, move || {
            let done = series.remove_sources(&ids)?;
            Ok(JobOutput::Status(format!(
                "Removed {}, kept {} that a prompt or an answer uses.",
                count_of(done.removed, "source"),
                done.blocked
            )))
        });
    }

    fn decide(&mut self, ctx: &egui::Context, ids: Vec<SuggestionId>, action: Action) {
        let Some(series) = self.series.clone() else {
            return;
        };
        self.start(ctx, move || {
            let decided = series.decide(&ids, action)?;
            let verb = match action {
                Action::Approve => "Approved",
                Action::Reject => "Rejected",
                Action::Revoke => "Revoked",
            };
            Ok(JobOutput::Status(format!(
                "{verb} {}, skipped {}.",
                count_of(decided.applied, "suggestion"),
                decided.skipped
            )))
        });
    }

    /// Makes one prompt for all the episodes.
    fn create_prompt(&mut self, ctx: &egui::Context, kind: PromptKind, episodes: Vec<u32>) {
        let Some(series) = self.series.clone() else {
            return;
        };
        self.start(ctx, move || {
            series.create_prompt(kind, &episodes).map(JobOutput::Prompt)
        });
    }

    fn create_package(&mut self, ctx: &egui::Context, at: Position) {
        let Some(series) = self.series.clone() else {
            return;
        };
        self.start(ctx, move || {
            series.build_package(at).map(JobOutput::Package)
        });
    }

    fn submit(&mut self, ctx: &egui::Context, prompt: PromptId, answer: String) {
        let Some(series) = self.series.clone() else {
            return;
        };
        self.clear_on_success = Some(ClearTarget::Answer);
        self.start(ctx, move || {
            let count = series.submit_answer(prompt, &answer)?;
            Ok(JobOutput::Status(format!(
                "Answer {prompt} gave {count} suggestions."
            )))
        });
    }

    fn body(&mut self, ui: &mut egui::Ui) {
        if self.series.is_none() {
            self.startup_view(ui);
            return;
        }
        match self.tab {
            Tab::Sources => self.sources_tab(ui),
            Tab::Suggestions => self.suggestions_tab(ui),
            Tab::Prompts => self.prompts_tab(ui),
            Tab::Package => self.package_tab(ui),
        }
    }

    /// The series list, the New series button, and the tabs.
    fn header(&mut self, ui: &mut egui::Ui) {
        let busy = self.busy();
        let ctx = ui.ctx().clone();
        let current = self
            .series
            .as_ref()
            .map(|series| series.dir().to_path_buf());
        let mut open: Option<PathBuf> = None;
        let mut close = false;
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!busy, |ui| {
                let selected = current.as_deref().map(series_name).unwrap_or_default();
                egui::ComboBox::from_id_salt("series")
                    .selected_text(egui::RichText::new(selected).strong())
                    .width(240.0)
                    .show_ui(ui, |ui| {
                        for dir in &self.series_list {
                            let is_current = current.as_ref() == Some(dir);
                            if ui.selectable_label(is_current, series_name(dir)).clicked()
                                && !is_current
                            {
                                open = Some(dir.clone());
                            }
                        }
                    });
                if ui.button("New series").clicked() {
                    self.new_series_open = true;
                    self.new_series_focus = true;
                }
                close = ui.button("All series").clicked();
            });
        });
        // The count is the suggestions that wait for a decision. It shows only when the list
        // can hold them.
        let waiting = self
            .rows
            .iter()
            .filter(|row| row.status == Status::Pending)
            .count();
        let suggestions = match self.filter.status {
            None | Some(Status::Pending) if waiting > 0 => format!("Suggestions ({waiting})"),
            _ => "Suggestions".to_string(),
        };
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Sources, "Sources");
            ui.selectable_value(&mut self.tab, Tab::Suggestions, suggestions);
            ui.selectable_value(&mut self.tab, Tab::Prompts, "Prompts");
            ui.selectable_value(&mut self.tab, Tab::Package, "Package");
        });
        if let Some(dir) = open {
            self.leave_series(&ctx, Some(dir));
        } else if close {
            self.leave_series(&ctx, None);
        }
        ui.add_space(4.0);
    }

    /// The last error, in a box that stays until the user dismisses it.
    fn error_bar(&mut self, ui: &mut egui::Ui) {
        let Some(message) = &self.error else {
            return;
        };
        let color = ui.visuals().error_fg_color;
        let mut dismiss = false;
        ui.add_space(4.0);
        egui::Frame::new()
            .fill(color.gamma_multiply(0.15))
            .stroke(egui::Stroke::new(1.0, color))
            .corner_radius(2)
            .inner_margin(8)
            .show(ui, |ui| {
                // The message takes the width that the button leaves. A long message wraps.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    dismiss = ui.button("Dismiss").clicked();
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                        ui.add(egui::Label::new(egui::RichText::new(message).color(color)).wrap());
                    });
                });
            });
        ui.add_space(4.0);
        if dismiss {
            self.error = None;
        }
    }

    /// The job that runs now, or the result of the last job.
    fn status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if self.busy() {
                let total = self.scan_progress.total.load(Ordering::Relaxed);
                if total > 0 {
                    let done = self.scan_progress.done.load(Ordering::Relaxed);
                    ui.label(format!("Working... {done}/{total} files"));
                } else {
                    ui.label("Working...");
                }
            } else {
                ui.label(self.status.as_str());
            }
        });
    }
}

/// Reads the three views from the series.
fn load_views(
    series: &Series,
    filter: SuggestionFilter,
) -> Result<(Vec<Source>, Vec<SuggestionRow>, Vec<EpisodeProgress>)> {
    Ok((
        series.sources()?,
        series.suggestions(&filter)?,
        series.progress()?,
    ))
}

/// The name of a series: the name of its directory.
fn series_name(dir: &Path) -> String {
    dir.file_name().map_or_else(
        || dir.display().to_string(),
        |name| name.display().to_string(),
    )
}

/// The first episode with a subtitle that is not done. Else the first episode with a subtitle.
fn first_unfinished(progress: &[EpisodeProgress]) -> Option<u32> {
    progress
        .iter()
        .find(|item| item.has_subtitle && item.unknown_lines != Some(0))
        .or_else(|| progress.iter().find(|item| item.has_subtitle))
        .map(|item| item.episode)
}
