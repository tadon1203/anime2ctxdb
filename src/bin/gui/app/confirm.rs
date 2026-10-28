use super::{App, count_of};
use anime2ctxdb::{Action, SourceId, SuggestionId};
use eframe::egui;
use std::path::PathBuf;

/// A step that needs a yes from the user before it runs.
pub(super) enum Confirm {
    /// Throw away the new files that are not saved.
    DiscardNew,
    /// Open another series (`Some`) or go to the start view (`None`) and lose the new files.
    Leave(Option<PathBuf>),
    /// Approve every pending suggestion in the list.
    ApproveShown(Vec<SuggestionId>),
    /// Remove saved sources. The ones in `blocked` stay, because an answer uses them.
    Remove {
        ids: Vec<SourceId>,
        blocked: Vec<SourceId>,
    },
}

/// How many file names the Remove question lists.
const NAMES_SHOWN: usize = 5;

impl Confirm {
    fn title(&self, app: &App) -> String {
        match self {
            Confirm::Remove { ids, blocked } => {
                format!("Remove {}?", count_of(ids.len() - blocked.len(), "source"))
            }
            Confirm::DiscardNew => format!("Discard {}?", count_of(app.detected.len(), "new file")),
            Confirm::Leave(_) => "Leave without saving?".to_string(),
            Confirm::ApproveShown(ids) => {
                format!("Approve {}?", count_of(ids.len(), "suggestion"))
            }
        }
    }

    fn detail(&self, app: &App) -> String {
        match self {
            Confirm::Remove { ids, blocked } => {
                let names: Vec<String> = ids
                    .iter()
                    .filter(|id| !blocked.contains(id))
                    .filter_map(|id| app.saved.iter().find(|record| &record.id == id))
                    .map(|record| {
                        record.path.file_name().map_or_else(
                            || record.path.display().to_string(),
                            |name| name.display().to_string(),
                        )
                    })
                    .collect();
                let mut lines: Vec<String> = names
                    .iter()
                    .take(NAMES_SHOWN)
                    .map(|name| format!("- {name}"))
                    .collect();
                if names.len() > NAMES_SHOWN {
                    lines.push(format!("and {} more", names.len() - NAMES_SHOWN));
                }
                lines.push(String::new());
                lines.push("The app removes its own copy. The original files stay.".to_string());
                if !blocked.is_empty() {
                    lines.push(format!(
                        "{} stay, because an answer uses them.",
                        count_of(blocked.len(), "source")
                    ));
                }
                lines.join("\n")
            }
            Confirm::DiscardNew => {
                "The files are not saved. You can import them again.".to_string()
            }
            Confirm::Leave(_) => format!(
                "{} not saved yet. They are lost if you leave.",
                count_of(app.detected.len(), "new file")
            ),
            Confirm::ApproveShown(_) => {
                "Every pending suggestion in the list becomes data. You can revoke each one later."
                    .to_string()
            }
        }
    }

    fn button(&self) -> &'static str {
        match self {
            Confirm::Remove { .. } => "Remove",
            Confirm::DiscardNew => "Discard",
            Confirm::Leave(_) => "Leave",
            Confirm::ApproveShown(_) => "Approve",
        }
    }
}

impl App {
    /// Shows the question, if there is one. Esc answers no.
    pub(super) fn confirm_window(&mut self, ctx: &egui::Context) {
        let Some(question) = &self.confirm else {
            return;
        };
        let title = question.title(self);
        let detail = question.detail(self);
        let button = question.button();
        let mut yes = false;
        let mut no = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_max_width(360.0);
                ui.label(detail);
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    yes = ui.button(button).clicked();
                    no |= ui.button("Cancel").clicked();
                });
            });
        if !yes && !no {
            return;
        }
        let Some(answered) = self.confirm.take() else {
            return;
        };
        if !yes {
            return;
        }
        match answered {
            Confirm::DiscardNew => {
                self.detected.clear();
                self.failures.clear();
            }
            Confirm::Leave(target) => self.go_to(ctx, target),
            Confirm::ApproveShown(ids) => self.decide(ctx, ids, Action::Approve),
            Confirm::Remove { ids, .. } => self.remove_sources(ctx, ids),
        }
    }
}
