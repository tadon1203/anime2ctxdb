use super::{App, count_of};
use anime2ctxdb::{EpisodeProgress, PromptKind};
use eframe::egui;

impl App {
    /// The episode list. A click on any part of a row selects an episode with a subtitle.
    /// Ctrl and click adds or removes one episode. Shift and click selects a range. The up and
    /// down keys move the selection when no text box has the focus.
    fn episode_list(&mut self, ui: &mut egui::Ui) {
        let selectable: Vec<u32> = self
            .progress
            .iter()
            .filter(|item| item.has_subtitle)
            .map(|item| item.episode)
            .collect();
        if !ui.ctx().egui_wants_keyboard_input() {
            let step = ui.input(|input| {
                i32::from(input.key_pressed(egui::Key::ArrowDown))
                    - i32::from(input.key_pressed(egui::Key::ArrowUp))
            });
            let current = selectable
                .iter()
                .position(|episode| Some(*episode) == self.episode);
            let next = match (step, current) {
                (0, _) => None,
                (_, None) => selectable.first(),
                (step, Some(index)) => selectable.get(index.saturating_add_signed(step as isize)),
            };
            if let Some(episode) = next {
                self.select_only(*episode);
            }
        }

        egui::Panel::left("episodes")
            .resizable(true)
            .default_size(360.0)
            .size_range(280.0..=560.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.small_button("Select all").clicked() {
                        self.selected.extend(selectable.iter().copied());
                    }
                    if ui.small_button("Clear").clicked() {
                        self.selected.clear();
                    }
                    ui.weak("Ctrl and Shift select more than one.");
                });
                list_header(ui);
                let mut clicked = None;
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        for item in &self.progress {
                            let selected = self.selected.contains(&item.episode);
                            if episode_row(ui, item, selected) {
                                clicked = Some(item.episode);
                            }
                        }
                    });
                if let Some(episode) = clicked {
                    let modifiers = ui.input(|input| input.modifiers);
                    if modifiers.shift {
                        self.select_range(&selectable, episode);
                    } else if modifiers.command {
                        if !self.selected.remove(&episode) {
                            self.selected.insert(episode);
                        }
                        self.episode = Some(episode);
                    } else {
                        self.select_only(episode);
                    }
                }
            });
    }

    /// Selects one episode and starts the range from it.
    fn select_only(&mut self, episode: u32) {
        self.selected.clear();
        self.selected.insert(episode);
        self.episode = Some(episode);
    }

    /// Selects the episodes from the one that the user picked last to `episode`.
    fn select_range(&mut self, selectable: &[u32], episode: u32) {
        let from = self
            .episode
            .and_then(|start| selectable.iter().position(|item| *item == start));
        let to = selectable.iter().position(|item| *item == episode);
        let (Some(from), Some(to)) = (from, to) else {
            self.select_only(episode);
            return;
        };
        self.selected.clear();
        self.selected
            .extend(selectable[from.min(to)..=from.max(to)].iter().copied());
    }

    pub(super) fn prompts_tab(&mut self, ui: &mut egui::Ui) {
        let busy = self.busy();
        let ctx = ui.ctx().clone();

        self.episode_list(ui);

        let episodes: Vec<u32> = self.selected.iter().copied().collect();
        let (new_speakers, new_story) = ui
            .horizontal(|ui| {
                match episodes.as_slice() {
                    [] => ui.weak("Select an episode"),
                    [episode] => ui.strong(format!("Episode {episode}")),
                    many => ui.strong(count_of(many.len(), "episode")),
                };
                let enabled = !busy && !episodes.is_empty();
                let speakers = ui
                    .add_enabled(enabled, egui::Button::new(new_prompt_label("speakers")))
                    .clicked();
                let story = ui
                    .add_enabled(enabled, egui::Button::new(new_prompt_label("story")))
                    .clicked();
                (speakers, story)
            })
            .inner;
        ui.separator();

        let PromptEvents { copy, submit } = self.prompt_view(ui, &ctx);
        if let Some(text) = self
            .prompt
            .as_ref()
            .filter(|_| copy)
            .map(|prompt| prompt.text.clone())
        {
            self.copy_text(&ctx, text, "Copied the prompt.");
        }
        if new_speakers {
            self.create_prompt(&ctx, PromptKind::Speakers, episodes.clone());
        }
        if new_story {
            self.create_prompt(&ctx, PromptKind::Story, episodes);
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

    /// The prompt and the answer box. Returns what the user clicked.
    fn prompt_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> PromptEvents {
        let busy = self.busy();
        let mut events = PromptEvents::default();
        let label = self.copy_label(ctx);
        if let Some(prompt) = &self.prompt {
            // The prompt and the answer get one half of the free height each.
            let row = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
            let text_height = ((ui.available_height() - 2.0 * row) / 2.0 - 16.0).max(80.0);

            ui.horizontal(|ui| {
                ui.strong(format!(
                    "Prompt for {} ({})",
                    episodes_text(&prompt.episodes),
                    kind_text(prompt.kind)
                ));
                events.copy = ui.add_enabled(!busy, egui::Button::new(label)).clicked();
            });
            let mut text = prompt.text.as_str();
            text_area(ui, "prompt-text", text_height, &mut text, "");

            ui.horizontal(|ui| {
                ui.strong("Answer");
                events.submit = ui
                    .add_enabled(
                        !busy && !self.answer.trim().is_empty(),
                        egui::Button::new("Submit"),
                    )
                    .clicked();
            });
            text_area(
                ui,
                "answer-text",
                text_height,
                &mut self.answer,
                "Paste the answer of the AI chat here.",
            );
        } else {
            ui.weak("Pick episodes, then make a prompt. Copy it to an AI chat and paste the answer here.");
        }
        events
    }
}

/// What the user clicked in the prompt view.
#[derive(Default)]
struct PromptEvents {
    copy: bool,
    submit: bool,
}

fn new_prompt_label(kind: &str) -> String {
    format!("New {kind} prompt")
}

/// The episodes in words, for example `episode 1` or `episodes 1, 2`.
fn episodes_text(episodes: &[u32]) -> String {
    let numbers: Vec<String> = episodes.iter().map(u32::to_string).collect();
    let noun = if episodes.len() == 1 {
        "episode"
    } else {
        "episodes"
    };
    format!("{noun} {}", numbers.join(", "))
}

fn kind_text(kind: PromptKind) -> &'static str {
    match kind {
        PromptKind::Speakers => "speakers",
        PromptKind::Story => "story",
    }
}

/// Where the columns of the episode list start, as fractions of the row width.
const SPEAKERS_AT: f32 = 0.34;

/// The inner margin of a row of the episode list, in points.
const ROW_PADDING: f32 = 8.0;

fn list_header(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ui.spacing().interact_size.y),
        egui::Sense::hover(),
    );
    let font = egui::TextStyle::Small.resolve(ui.style());
    let color = ui.visuals().weak_text_color();
    let painter = ui.painter();
    let at =
        |fraction: f32| rect.left() + ROW_PADDING + fraction * (rect.width() - 2.0 * ROW_PADDING);
    painter.text(
        egui::pos2(at(0.0), rect.center().y),
        egui::Align2::LEFT_CENTER,
        "Episode",
        font.clone(),
        color,
    );
    painter.text(
        egui::pos2(at(SPEAKERS_AT), rect.center().y),
        egui::Align2::LEFT_CENTER,
        "Speakers",
        font.clone(),
        color,
    );
    painter.text(
        egui::pos2(at(1.0), rect.center().y),
        egui::Align2::RIGHT_CENTER,
        "Facts",
        font,
        color,
    );
}

/// One row of the episode list. Returns true when the user clicked it.
fn episode_row(ui: &mut egui::Ui, item: &EpisodeProgress, selected: bool) -> bool {
    let sense = if item.has_subtitle {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ui.spacing().interact_size.y),
        sense,
    );
    let visuals = ui.visuals();
    let (fill, text_color) = if selected {
        (visuals.selection.bg_fill, visuals.selection.stroke.color)
    } else if item.has_subtitle && response.hovered() {
        (visuals.widgets.hovered.weak_bg_fill, visuals.text_color())
    } else if item.has_subtitle {
        (egui::Color32::TRANSPARENT, visuals.text_color())
    } else {
        (egui::Color32::TRANSPARENT, visuals.weak_text_color())
    };
    let warn = visuals.warn_fg_color;
    let font = egui::TextStyle::Body.resolve(ui.style());
    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, fill);

    let at =
        |fraction: f32| rect.left() + ROW_PADDING + fraction * (rect.width() - 2.0 * ROW_PADDING);
    let y = rect.center().y;
    painter.text(
        egui::pos2(at(0.0), y),
        egui::Align2::LEFT_CENTER,
        format!("Episode {}", item.episode),
        font.clone(),
        text_color,
    );
    let (speakers, speakers_color) = match item.unknown_lines {
        None => ("no subtitle".to_string(), text_color),
        Some(0) => ("done".to_string(), text_color),
        Some(count) => (
            format!("{count} lines unknown"),
            if selected { text_color } else { warn },
        ),
    };
    painter.text(
        egui::pos2(at(SPEAKERS_AT), y),
        egui::Align2::LEFT_CENTER,
        speakers,
        font.clone(),
        speakers_color,
    );
    painter.text(
        egui::pos2(at(1.0), y),
        egui::Align2::RIGHT_CENTER,
        item.facts.to_string(),
        font,
        text_color,
    );
    if !item.has_subtitle {
        response.on_hover_text("This episode has no subtitle yet.");
        return false;
    }
    response.clicked()
}

/// A multiline text box with a fixed height. Long text scrolls inside the box.
pub(super) fn text_area(
    ui: &mut egui::Ui,
    salt: &str,
    height: f32,
    text: &mut dyn egui::TextBuffer,
    hint: &str,
) {
    let width = ui.available_width();
    egui::ScrollArea::vertical()
        .id_salt(salt)
        .max_width(width)
        .max_height(height)
        .auto_shrink(false)
        .show(ui, |ui| {
            // The box takes the whole width that the scroll area has, up to its scroll bar.
            ui.add(
                egui::TextEdit::multiline(text)
                    .hint_text(hint)
                    .desired_width(ui.available_width())
                    .min_size(egui::vec2(ui.available_width(), height)),
            );
        });
}
