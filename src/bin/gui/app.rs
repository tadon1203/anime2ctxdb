mod package_tab;
mod prompts_tab;
mod sources_tab;
mod suggestions_tab;

use anime2ctxdb::{
    Action, EpisodeProgress, Position, Prompt, PromptId, PromptKind, Series, Source, Status,
    SuggestionFilter, SuggestionId, SuggestionRow,
};
use anyhow::Result;
use eframe::egui;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

/// The result of a job that runs on a worker thread.
enum JobOutput {
    Detected(Vec<Source>),
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

pub struct App {
    series: Option<Series>,
    tab: Tab,
    error: Option<String>,
    status: String,
    job: Option<Receiver<Result<JobOutput>>>,
    clear_on_success: Option<ClearTarget>,
    detected: Vec<Source>,
    saved: Vec<Source>,
    filter: SuggestionFilter,
    rows: Vec<SuggestionRow>,
    progress: Vec<EpisodeProgress>,
    episode: Option<u32>,
    prompt: Option<Prompt>,
    answer: String,
    package_episode: Option<u32>,
    package_time: String,
    package: Option<String>,
}

impl Default for App {
    fn default() -> Self {
        App {
            series: None,
            tab: Tab::Sources,
            error: None,
            status: String::new(),
            job: None,
            clear_on_success: None,
            detected: Vec::new(),
            saved: Vec::new(),
            filter: SuggestionFilter {
                status: Some(Status::Pending),
                kind: None,
            },
            rows: Vec::new(),
            progress: Vec::new(),
            episode: None,
            prompt: None,
            answer: String::new(),
            package_episode: None,
            package_time: "00:00".to_string(),
            package: None,
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll();
        egui::CentralPanel::default().show(ui, |ui| self.body(ui));
    }
}

/// Sets the text sizes and the spacing. The defaults are too small to read and to click.
pub fn set_sizes(ctx: &egui::Context) {
    ctx.global_style_mut(|style| {
        use egui::{FontFamily::Monospace, FontFamily::Proportional, FontId, TextStyle, vec2};
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
                self.finish(result);
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => panic!("the worker thread panicked"),
        }
    }

    fn finish(&mut self, result: Result<JobOutput>) {
        let clear = self.clear_on_success.take();
        match result {
            Ok(JobOutput::Detected(records)) => {
                self.status = format!("Detected {} files.", records.len());
                self.detected = records;
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
                    Some(ClearTarget::Detected) => self.detected.clear(),
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
                self.saved = saved;
                self.rows = rows;
                self.progress = progress;
                if self.episode.is_none() {
                    self.episode = first_unfinished(&self.progress);
                }
            }
            Err(error) => self.error = Some(format!("{error:#}")),
        }
    }

    fn open_series(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Open series")
            .pick_folder()
        else {
            return;
        };
        match Series::open(&path) {
            Ok(series) => self.use_series(ctx, series),
            Err(error) => self.error = Some(format!("{error:#}")),
        }
    }

    fn new_series(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Choose an empty folder for the new series")
            .pick_folder()
        else {
            return;
        };
        match Series::create(&path) {
            Ok(series) => self.use_series(ctx, series),
            Err(error) => self.error = Some(format!("{error:#}")),
        }
    }

    fn use_series(&mut self, ctx: &egui::Context, series: Series) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "anime2ctxdb - {}",
            series.dir().display()
        )));
        self.series = Some(series);
        self.detected.clear();
        self.prompt = None;
        self.answer.clear();
        self.episode = None;
        self.package = None;
        self.status = "Series opened.".to_string();
        self.reload();
    }

    fn save(&mut self, ctx: &egui::Context, records: Vec<Source>, clear: Option<ClearTarget>) {
        let Some(series) = self.series.clone() else {
            return;
        };
        self.clear_on_success = clear;
        self.start(ctx, move || {
            series.add_sources(&records)?;
            Ok(JobOutput::Status(format!(
                "saved {} sources",
                records.len()
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
                Action::Approve => "approved",
                Action::Reject => "rejected",
                Action::Revoke => "revoked",
            };
            Ok(JobOutput::Status(format!(
                "{verb} {}, skipped {}",
                decided.applied, decided.skipped
            )))
        });
    }

    fn create_prompt(&mut self, ctx: &egui::Context, kind: PromptKind, episode: u32) {
        let Some(series) = self.series.clone() else {
            return;
        };
        self.start(ctx, move || {
            series.create_prompt(kind, episode).map(JobOutput::Prompt)
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
                "answer {prompt}: {count} suggestions"
            )))
        });
    }

    fn body(&mut self, ui: &mut egui::Ui) {
        self.header(ui);
        if let Some(message) = self.error.clone() {
            ui.horizontal(|ui| {
                ui.label(format!("Error: {message}"));
                if ui.button("Clear").clicked() {
                    self.error = None;
                }
            });
        }
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Sources, "Sources");
            let suggestions = format!("Suggestions ({})", self.rows.len());
            ui.selectable_value(&mut self.tab, Tab::Suggestions, suggestions);
            ui.selectable_value(&mut self.tab, Tab::Prompts, "Prompts");
            ui.selectable_value(&mut self.tab, Tab::Package, "Package");
        });
        if self.busy() {
            ui.label("Working...");
        } else if !self.status.is_empty() {
            ui.label(self.status.as_str());
        }
        ui.separator();
        match self.tab {
            Tab::Sources => self.sources_tab(ui),
            Tab::Suggestions => self.suggestions_tab(ui),
            Tab::Prompts => self.prompts_tab(ui),
            Tab::Package => self.package_tab(ui),
        }
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        let busy = self.busy();
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!busy, egui::Button::new("Open series"))
                .clicked()
            {
                self.open_series(&ctx);
            }
            if ui
                .add_enabled(!busy, egui::Button::new("New series"))
                .clicked()
            {
                self.new_series(&ctx);
            }
            let title = match &self.series {
                Some(series) => format!("anime2ctxdb - {}", series.dir().display()),
                None => "anime2ctxdb - no series open".to_string(),
            };
            ui.label(title);
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

/// The first episode with a subtitle that is not done. Else the first episode with a subtitle.
fn first_unfinished(progress: &[EpisodeProgress]) -> Option<u32> {
    progress
        .iter()
        .find(|item| item.has_subtitle && item.unknown_lines != Some(0))
        .or_else(|| progress.iter().find(|item| item.has_subtitle))
        .map(|item| item.episode)
}
