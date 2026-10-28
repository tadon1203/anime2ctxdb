use super::{App, count_of};
use anime2ctxdb::{Action, SuggestionId};
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
}

impl Confirm {
    fn title(&self, app: &App) -> String {
        match self {
            Confirm::DiscardNew => format!("Discard {}?", count_of(app.detected.len(), "new file")),
            Confirm::Leave(_) => "Leave without saving?".to_string(),
            Confirm::ApproveShown(ids) => {
                format!("Approve {}?", count_of(ids.len(), "suggestion"))
            }
        }
    }

    fn detail(&self, app: &App) -> String {
        match self {
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
        }
    }
}
