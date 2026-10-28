use anime2ctxdb::{
    Action, EpisodeProgress, Language, Position, Prompt, PromptId, PromptKind, Series, Source,
    SourceKind, Status, SuggestionFilter, SuggestionId, SuggestionKind, SuggestionRow, parse_clock,
};
use anyhow::Result;
use eframe::egui;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

const SUBTITLE_AND_VIDEO: [&str; 5] = ["ass", "mkv", "mp4", "webm", "avi"];

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

    fn sources_tab(&mut self, ui: &mut egui::Ui) {
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

    fn suggestions_tab(&mut self, ui: &mut egui::Ui) {
        let busy = self.busy();
        let ctx = ui.ctx().clone();
        let before = self.filter;
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("suggestion-status")
                .selected_text(status_text(self.filter.status))
                .show_ui(ui, |ui| {
                    for option in [
                        None,
                        Some(Status::Pending),
                        Some(Status::Approved),
                        Some(Status::Rejected),
                        Some(Status::Invalid),
                    ] {
                        ui.selectable_value(&mut self.filter.status, option, status_text(option));
                    }
                });
            egui::ComboBox::from_id_salt("suggestion-kind")
                .selected_text(kind_filter_text(self.filter.kind))
                .show_ui(ui, |ui| {
                    for option in [
                        None,
                        Some(SuggestionKind::Entity),
                        Some(SuggestionKind::Label),
                        Some(SuggestionKind::Color),
                        Some(SuggestionKind::LineSpeaker),
                        Some(SuggestionKind::Fact),
                        Some(SuggestionKind::Scene),
                    ] {
                        ui.selectable_value(
                            &mut self.filter.kind,
                            option,
                            kind_filter_text(option),
                        );
                    }
                });
        });
        if self.filter != before {
            self.reload();
        }

        let pending: Vec<SuggestionId> = self
            .rows
            .iter()
            .filter(|row| row.status == Status::Pending)
            .map(|row| row.id.clone())
            .collect();
        let mut decision: Option<(Vec<SuggestionId>, Action)> = None;
        ui.horizontal(|ui| {
            let approve_all = ui
                .add_enabled(
                    !busy && !pending.is_empty(),
                    egui::Button::new("Approve all shown"),
                )
                .clicked();
            if approve_all {
                decision = Some((pending.clone(), Action::Approve));
            }
        });

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("suggestions").striped(true).show(ui, |ui| {
                for heading in ["id", "kind", "content", "evidence", ""] {
                    ui.label(heading);
                }
                ui.end_row();
                for row in &self.rows {
                    ui.label(row.id.0.as_str());
                    ui.label(suggestion_kind_text(row.kind));
                    ui.label(row.content.as_str());
                    ui.label(row.evidence.join("\n"));
                    ui.horizontal(|ui| match row.status {
                        Status::Pending => {
                            if ui
                                .add_enabled(!busy && !row.blocked, egui::Button::new("Approve"))
                                .clicked()
                            {
                                decision = Some((vec![row.id.clone()], Action::Approve));
                            }
                            if ui.add_enabled(!busy, egui::Button::new("Reject")).clicked() {
                                decision = Some((vec![row.id.clone()], Action::Reject));
                            }
                            if row.blocked {
                                ui.label("blocked");
                            }
                        }
                        Status::Approved | Status::Rejected => {
                            if ui.add_enabled(!busy, egui::Button::new("Revoke")).clicked() {
                                decision = Some((vec![row.id.clone()], Action::Revoke));
                            }
                        }
                        Status::Invalid => {
                            ui.label("invalid");
                        }
                    });
                    ui.end_row();
                }
            });
        });

        if let Some((ids, action)) = decision {
            self.decide(&ctx, ids, action);
        }
    }

    fn prompts_tab(&mut self, ui: &mut egui::Ui) {
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

    fn package_tab(&mut self, ui: &mut egui::Ui) {
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

fn status_text(status: Option<Status>) -> &'static str {
    match status {
        None => "all",
        Some(Status::Pending) => "pending",
        Some(Status::Approved) => "approved",
        Some(Status::Rejected) => "rejected",
        Some(Status::Invalid) => "invalid",
    }
}

fn kind_filter_text(kind: Option<SuggestionKind>) -> &'static str {
    match kind {
        None => "all",
        Some(kind) => suggestion_kind_text(kind),
    }
}

fn suggestion_kind_text(kind: SuggestionKind) -> &'static str {
    match kind {
        SuggestionKind::Entity => "entity",
        SuggestionKind::Label => "label",
        SuggestionKind::Color => "color",
        SuggestionKind::LineSpeaker => "line_speaker",
        SuggestionKind::Fact => "fact",
        SuggestionKind::Scene => "scene",
    }
}
