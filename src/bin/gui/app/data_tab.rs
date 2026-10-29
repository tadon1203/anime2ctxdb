use super::{App, JobOutput};
use anime2ctxdb::{EntityRow, FactRow, Importance, Position, SceneRow};
use eframe::egui;

impl App {
    /// What the answers gave. The tab only shows it. To change a fact or a scene, answer the
    /// episode again. The importance of an entity can be set here.
    pub(super) fn data_tab(&mut self, ui: &mut egui::Ui) {
        let busy = self.busy();
        let mut change: Option<(String, Choice)> = None;
        let Some(data) = &self.data else {
            ui.weak("No data yet.");
            return;
        };
        if data.entities.is_empty() && data.facts.is_empty() && data.scenes.is_empty() {
            ui.weak("No data yet. Answer a prompt in the Prompts tab.");
            return;
        }
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(12, 0))
                    .show(ui, |ui| {
                        ui.strong(format!("Entities ({})", data.entities.len()));
                        for entity in &data.entities {
                            ui.horizontal(|ui| {
                                if let Some(value) = importance_box(ui, entity, busy) {
                                    change = Some((entity.name.clone(), value));
                                }
                                ui.label(entity_text(entity));
                            });
                        }
                        ui.add_space(16.0);
                        ui.strong(format!("Facts ({})", data.facts.len()));
                        for fact in &data.facts {
                            fact_view(ui, fact);
                        }
                        ui.add_space(16.0);
                        ui.strong(format!("Scenes ({})", data.scenes.len()));
                        for scene in &data.scenes {
                            ui.label(scene_text(scene));
                        }
                    });
            });
        if let Some((name, choice)) = change {
            self.change_importance(ui.ctx(), name, choice);
        }
    }

    /// Saves the importance of an entity on a worker thread.
    fn change_importance(&mut self, ctx: &egui::Context, name: String, choice: Choice) {
        let Some(series) = self.series.clone() else {
            return;
        };
        let importance = match choice {
            Choice::Set(importance) => Some(importance),
            Choice::Auto => None,
        };
        self.start(ctx, move || {
            series.set_importance(&name, importance)?;
            Ok(JobOutput::Status(format!(
                "Saved the importance of {name}."
            )))
        });
    }
}

/// What the user picks in the drop-down of an entity.
#[derive(Clone, Copy)]
enum Choice {
    Set(Importance),
    /// Removes the hand setting.
    Auto,
}

/// A drop-down with the importance of an entity and `auto`, which removes the hand setting.
/// Returns the new value when the user picks one.
fn importance_box(ui: &mut egui::Ui, entity: &EntityRow, busy: bool) -> Option<Choice> {
    const CHOICES: [(&str, Choice); 4] = [
        ("main", Choice::Set(Importance::Main)),
        ("recurring", Choice::Set(Importance::Recurring)),
        ("minor", Choice::Set(Importance::Minor)),
        ("auto", Choice::Auto),
    ];
    let shown = match entity.importance {
        Importance::Main => "main",
        Importance::Recurring => "recurring",
        Importance::Minor => "minor",
    };
    let text = if entity.manual {
        format!("{shown} (set)")
    } else {
        shown.to_string()
    };
    let mut picked = None;
    ui.add_enabled_ui(!busy, |ui| {
        egui::ComboBox::from_id_salt(("importance", &entity.name))
            .selected_text(text)
            .width(140.0)
            .show_ui(ui, |ui| {
                for (label, value) in CHOICES {
                    if ui.selectable_label(false, label).clicked() {
                        picked = Some(value);
                    }
                }
            });
    });
    picked
}

fn at_text(at: Position) -> String {
    format!("E{} {}", at.episode, clock(at.time_ms))
}

fn clock(ms: u32) -> String {
    let seconds = ms / 1000;
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

fn entity_text(entity: &EntityRow) -> String {
    let reading = entity
        .reading
        .as_ref()
        .map(|reading| format!(" ({reading})"))
        .unwrap_or_default();
    format!(
        "{}{reading}  {}  from {}",
        entity.name,
        entity.kind,
        at_text(entity.since)
    )
}

fn fact_view(ui: &mut egui::Ui, fact: &FactRow) {
    ui.label(format!(
        "{} [{}] {}",
        at_text(fact.since),
        fact.entities.join(", "),
        fact.text
    ));
    for line in &fact.evidence {
        ui.label(egui::RichText::new(format!("    {line}")).small().weak());
    }
}

fn scene_text(scene: &SceneRow) -> String {
    let place = scene
        .place
        .as_ref()
        .map(|place| format!(" [{place}]"))
        .unwrap_or_default();
    format!(
        "E{} {}-{}{place} {}",
        scene.episode,
        clock(scene.start_ms),
        clock(scene.end_ms),
        scene.summary
    )
}
