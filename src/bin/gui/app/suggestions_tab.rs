use super::App;
use anime2ctxdb::{Action, Status, SuggestionId, SuggestionKind};
use eframe::egui;

impl App {
    pub(super) fn suggestions_tab(&mut self, ui: &mut egui::Ui) {
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
