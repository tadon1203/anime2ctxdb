use super::{App, confirm::Confirm};
use anime2ctxdb::{Action, Status, SuggestionId, SuggestionKind, SuggestionRow};
use eframe::egui;

/// The widths of the fixed columns, in points. The content and the evidence share the rest.
const ID_WIDTH: f32 = 60.0;
const KIND_WIDTH: f32 = 110.0;
const ACTION_WIDTH: f32 = 190.0;

/// The narrowest a text column gets, in points.
const TEXT_COLUMN_MIN_WIDTH: f32 = 200.0;

impl App {
    pub(super) fn suggestions_tab(&mut self, ui: &mut egui::Ui) {
        let busy = self.busy();
        let ctx = ui.ctx().clone();
        let before = self.filter;
        let pending: Vec<SuggestionId> = self
            .rows
            .iter()
            .filter(|row| row.status == Status::Pending)
            .map(|row| row.id.clone())
            .collect();
        let mut decision: Option<(Vec<SuggestionId>, Action)> = None;
        let mut approve_shown = false;
        ui.horizontal(|ui| {
            ui.label("Status");
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
            ui.label("Kind");
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
            ui.separator();
            let approve_all = ui
                .add_enabled(
                    !busy && !pending.is_empty(),
                    egui::Button::new(format!("Approve all shown ({})", pending.len())),
                )
                .on_disabled_hover_text("No pending suggestion in the list.")
                .clicked();
            if approve_all {
                approve_shown = true;
            }
        });
        if self.filter != before {
            self.reload();
        }
        ui.separator();

        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                if self.rows.is_empty() {
                    ui.weak(empty_text(self.filter.status, self.filter.kind));
                } else if let Some(clicked) = suggestion_grid(ui, &self.rows, !busy) {
                    decision = Some(clicked);
                }
            });

        if approve_shown {
            self.confirm = Some(Confirm::ApproveShown(pending));
        }
        if let Some((ids, action)) = decision {
            self.decide(&ctx, ids, action);
        }
    }
}

/// Shows the suggestions in a grid. Returns the decision of the button that was clicked.
fn suggestion_grid(
    ui: &mut egui::Ui,
    rows: &[SuggestionRow],
    enabled: bool,
) -> Option<(Vec<SuggestionId>, Action)> {
    // The content and the evidence share the width that the fixed columns leave. They wrap, so
    // a long line does not push the buttons out of the window.
    let spacing = ui.spacing().item_spacing.x;
    let fixed = ID_WIDTH + KIND_WIDTH + ACTION_WIDTH + 4.0 * spacing;
    let text_width = ((ui.available_width() - fixed) / 2.0).max(TEXT_COLUMN_MIN_WIDTH);
    let mut decision = None;
    egui::Grid::new("suggestions").striped(true).show(ui, |ui| {
        for heading in ["Id", "Kind", "Content", "Evidence", ""] {
            ui.weak(heading);
        }
        ui.end_row();
        for row in rows {
            sized_cell(ui, ID_WIDTH, |ui| ui.monospace(row.id.0.as_str()));
            sized_cell(ui, KIND_WIDTH, |ui| {
                ui.label(suggestion_kind_text(row.kind))
            });
            sized_cell(ui, text_width, |ui| {
                ui.add(egui::Label::new(&row.content).wrap())
            });
            sized_cell(ui, text_width, |ui| {
                ui.add(egui::Label::new(row.evidence.join("\n")).wrap())
            });
            sized_cell(ui, ACTION_WIDTH, |ui| {
                ui.horizontal(|ui| match row.status {
                    Status::Pending => {
                        let approve = ui
                            .add_enabled(enabled && !row.blocked, egui::Button::new("Approve"))
                            .on_disabled_hover_text(
                                "Decide the suggestion that this one refers to first.",
                            );
                        if approve.clicked() {
                            decision = Some((vec![row.id.clone()], Action::Approve));
                        }
                        // The gap keeps a stray click on Approve away from Reject.
                        ui.add_space(12.0);
                        if ui
                            .add_enabled(enabled, egui::Button::new("Reject"))
                            .clicked()
                        {
                            decision = Some((vec![row.id.clone()], Action::Reject));
                        }
                    }
                    Status::Approved | Status::Rejected => {
                        if ui
                            .add_enabled(enabled, egui::Button::new("Revoke"))
                            .clicked()
                        {
                            decision = Some((vec![row.id.clone()], Action::Revoke));
                        }
                    }
                    Status::Invalid => {
                        ui.colored_label(ui.visuals().warn_fg_color, "Invalid")
                            .on_hover_text("It refers to a suggestion that was rejected.");
                    }
                })
                .response
            });
            ui.end_row();
        }
    });
    decision
}

/// Runs `add` in a cell that is exactly `width` wide.
fn sized_cell<R>(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.vertical(|ui| {
        ui.set_width(width);
        add(ui)
    })
    .inner
}

/// What the empty list says. It names the filter, so the user knows how to get rows.
fn empty_text(status: Option<Status>, kind: Option<SuggestionKind>) -> String {
    if status.is_none() && kind.is_none() {
        "No suggestions yet. Make a prompt in the Prompts tab and submit the answer.".to_string()
    } else {
        let status = status.map_or("", |status| status_text(Some(status)));
        let kind = kind.map_or("", suggestion_kind_text);
        let words: Vec<&str> = [status, kind, "suggestions"]
            .into_iter()
            .filter(|word| !word.is_empty())
            .collect();
        format!("No {}. Change the filter to see more.", words.join(" "))
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
        SuggestionKind::LineSpeaker => "speaker",
        SuggestionKind::Fact => "fact",
        SuggestionKind::Scene => "scene",
    }
}
