use super::App;
use anime2ctxdb::{EntityRow, FactRow, Position, SceneRow};
use eframe::egui;

impl App {
    /// What the answers gave. The tab only shows it. To change something, answer the episode again.
    pub(super) fn data_tab(&self, ui: &mut egui::Ui) {
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
                            ui.label(entity_text(entity));
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
    }
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
