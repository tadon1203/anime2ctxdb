use crate::files::{append_jsonl, enum_text, read_jsonl};
use crate::sources::ass::LineId;
use crate::speakers::LabelKind;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::Path;

/// `<answer>-<n>`, where `<answer>` is the number of the answer file, and `n` is the position in the answer, from 1.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SuggestionId(pub String);

impl SuggestionId {
    /// The answer number and the position. Sort by these two numbers, not by the text.
    ///
    /// # Panics
    ///
    /// Panics when the ID does not have the form `<answer>-<n>`.
    pub fn sort_key(&self) -> (u32, u32) {
        let (answer, position) = self
            .0
            .split_once('-')
            .expect("a suggestion ID has the form <answer>-<n>");
        (
            answer.parse().expect("the answer part is a number"),
            position.parse().expect("the position part is a number"),
        )
    }
}

impl fmt::Display for SuggestionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// One item from an answer, waiting for the user's decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suggestion {
    pub id: SuggestionId,
    pub content: Content,
}

/// One or more lines. A stored suggestion with no evidence cannot be read back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<LineId>", into = "Vec<LineId>")]
pub struct Evidence(Vec<LineId>);

impl Evidence {
    /// `None` for an empty list.
    pub fn new(lines: Vec<LineId>) -> Option<Evidence> {
        (!lines.is_empty()).then_some(Evidence(lines))
    }

    pub fn lines(&self) -> &[LineId] {
        &self.0
    }
}

impl TryFrom<Vec<LineId>> for Evidence {
    type Error = String;

    fn try_from(lines: Vec<LineId>) -> Result<Evidence, String> {
        Evidence::new(lines).ok_or_else(|| "evidence has no line".to_string())
    }
}

impl From<Evidence> for Vec<LineId> {
    fn from(evidence: Evidence) -> Vec<LineId> {
        evidence.0
    }
}

/// A reference to an entity is the text of an `EntityId`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Content {
    Entity {
        kind: String,
        name: String,
        reading: Option<String>,
        evidence: Evidence,
    },
    Label {
        label: String,
        kind: LabelKind,
        character: Option<String>,
    },
    Color {
        color: u32,
        character: String,
    },
    LineSpeaker {
        line: LineId,
        characters: Vec<String>,
    },
    Fact {
        entities: Vec<String>,
        text: String,
        evidence: Evidence,
    },
    Scene {
        start: LineId,
        end: LineId,
        place: Option<String>,
        summary: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionKind {
    Entity,
    Label,
    Color,
    LineSpeaker,
    Fact,
    Scene,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Approve,
    Reject,
    Revoke,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pending,
    Approved,
    Rejected,
    Invalid,
}

/// One line of `decisions.jsonl`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub suggestion: SuggestionId,
    pub action: Action,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Review {
    pub status: Status,
    pub blocked: bool,
}

impl Content {
    pub fn kind(&self) -> SuggestionKind {
        match self {
            Content::Entity { .. } => SuggestionKind::Entity,
            Content::Label { .. } => SuggestionKind::Label,
            Content::Color { .. } => SuggestionKind::Color,
            Content::LineSpeaker { .. } => SuggestionKind::LineSpeaker,
            Content::Fact { .. } => SuggestionKind::Fact,
            Content::Scene { .. } => SuggestionKind::Scene,
        }
    }

    /// The suggestions that this suggestion refers to.
    pub fn dependencies(&self) -> Vec<SuggestionId> {
        let entities: Vec<&String> = match self {
            Content::Entity { .. } => Vec::new(),
            Content::Label { character, .. } => character.iter().collect(),
            Content::Color { character, .. } => vec![character],
            Content::LineSpeaker { characters, .. } => characters.iter().collect(),
            Content::Fact { entities, .. } => entities.iter().collect(),
            Content::Scene { place, .. } => place.iter().collect(),
        };
        entities
            .into_iter()
            .map(|entity| SuggestionId(entity.clone()))
            .collect()
    }

    /// The lines that the suggestion is based on. Empty for a label and a color.
    /// For a scene: its start line and its end line.
    pub fn evidence_lines(&self) -> Vec<LineId> {
        match self {
            Content::Entity { evidence, .. } | Content::Fact { evidence, .. } => {
                evidence.lines().to_vec()
            }
            Content::LineSpeaker { line, .. } => vec![line.clone()],
            Content::Scene { start, end, .. } => vec![start.clone(), end.clone()],
            Content::Label { .. } | Content::Color { .. } => Vec::new(),
        }
    }

    /// For example `fact [明智あんな, 坂本みくる]: ...`. `names` maps an ID to an entity name.
    pub fn describe(&self, names: &HashMap<String, String>) -> String {
        let name_of = |id: &String| names.get(id).cloned().unwrap_or_else(|| id.clone());
        match self {
            Content::Entity {
                kind,
                name,
                reading: Some(reading),
                ..
            } => format!("{kind} \"{name}\" ({reading})"),
            Content::Entity { kind, name, .. } => format!("{kind} \"{name}\""),
            Content::Label {
                label,
                kind,
                character,
            } => {
                let target = match character {
                    Some(id) => name_of(id),
                    None => enum_text(kind),
                };
                format!("label \"{label}\" -> {target}")
            }
            Content::Color { color, character } => {
                format!("color #{color:06X} -> {}", name_of(character))
            }
            Content::LineSpeaker { characters, .. } if characters.is_empty() => {
                "nobody".to_string()
            }
            Content::LineSpeaker { characters, .. } => {
                let names: Vec<String> = characters.iter().map(name_of).collect();
                format!("speaker {}", names.join(", "))
            }
            Content::Fact { entities, text, .. } => {
                let names: Vec<String> = entities.iter().map(name_of).collect();
                format!("fact [{}]: {text}", names.join(", "))
            }
            Content::Scene { place, summary, .. } => match place {
                Some(id) => format!("scene [{}]: {summary}", name_of(id)),
                None => format!("scene: {summary}"),
            },
        }
    }

    /// The text that only one approved suggestion can hold: a label or a color.
    pub fn unique_key(&self) -> Option<String> {
        match self {
            Content::Label { label, .. } => Some(format!("label {label}")),
            Content::Color { color, .. } => Some(format!("color {color}")),
            Content::Entity { .. }
            | Content::LineSpeaker { .. }
            | Content::Fact { .. }
            | Content::Scene { .. } => None,
        }
    }
}

pub fn read_decisions(series_dir: &Path) -> Result<Vec<Decision>> {
    read_jsonl(&series_dir.join("decisions.jsonl"))
}

pub fn append_decisions(series_dir: &Path, decisions: &[Decision]) -> Result<()> {
    append_jsonl(&series_dir.join("decisions.jsonl"), decisions)
}

/// The status of each suggestion. The last decision of a suggestion is its own status.
///
/// A suggestion is `Invalid` when one of the suggestions it refers to is `Rejected` or `Invalid`.
/// It is `blocked` when one of them is `Pending`.
pub fn reviews(
    suggestions: &[Suggestion],
    decisions: &[Decision],
) -> HashMap<SuggestionId, Review> {
    let mut own: HashMap<&SuggestionId, Status> = HashMap::new();
    for decision in decisions {
        let status = match decision.action {
            Action::Approve => Status::Approved,
            Action::Reject => Status::Rejected,
            Action::Revoke => Status::Pending,
        };
        own.insert(&decision.suggestion, status);
    }

    let mut sorted: Vec<&Suggestion> = suggestions.iter().collect();
    sorted.sort_by_key(|suggestion| suggestion.id.sort_key());

    let mut reviews: HashMap<SuggestionId, Review> = HashMap::new();
    for suggestion in sorted {
        let own_status = own.get(&suggestion.id).copied().unwrap_or(Status::Pending);
        let dependencies: Vec<Review> = suggestion
            .content
            .dependencies()
            .iter()
            .map(|dependency| {
                reviews.get(dependency).copied().unwrap_or(Review {
                    status: Status::Invalid,
                    blocked: false,
                })
            })
            .collect();
        let refused = dependencies
            .iter()
            .any(|dependency| matches!(dependency.status, Status::Rejected | Status::Invalid));
        let status = if refused { Status::Invalid } else { own_status };
        let blocked = dependencies
            .iter()
            .any(|dependency| dependency.status == Status::Pending);
        reviews.insert(suggestion.id.clone(), Review { status, blocked });
    }
    reviews
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CHARACTER;

    fn entity(id: &str) -> Suggestion {
        Suggestion {
            id: SuggestionId(id.to_string()),
            content: Content::Entity {
                kind: CHARACTER.to_string(),
                name: "n".to_string(),
                reading: None,
                evidence: Evidence::new(vec![LineId("s:1".to_string())]).expect("one line"),
            },
        }
    }

    fn label(id: &str, character: &str) -> Suggestion {
        Suggestion {
            id: SuggestionId(id.to_string()),
            content: Content::Label {
                label: "l".to_string(),
                kind: LabelKind::Character,
                character: Some(character.to_string()),
            },
        }
    }

    fn decide(id: &str, action: Action) -> Decision {
        Decision {
            suggestion: SuggestionId(id.to_string()),
            action,
        }
    }

    #[test]
    fn a_label_is_blocked_while_its_character_is_pending() {
        let suggestions = [entity("1-1"), label("1-2", "1-1")];

        let reviews = reviews(&suggestions, &[]);

        assert!(reviews[&SuggestionId("1-2".to_string())].blocked);
        assert_eq!(
            reviews[&SuggestionId("1-2".to_string())].status,
            Status::Pending
        );
    }

    #[test]
    fn a_label_is_invalid_when_its_character_is_rejected() {
        let suggestions = [entity("1-1"), label("1-2", "1-1")];
        let decisions = [decide("1-1", Action::Reject)];

        let reviews = reviews(&suggestions, &decisions);

        assert_eq!(
            reviews[&SuggestionId("1-2".to_string())].status,
            Status::Invalid
        );
    }

    #[test]
    fn the_last_decision_wins() {
        let suggestions = [entity("1-1")];
        let decisions = [
            decide("1-1", Action::Approve),
            decide("1-1", Action::Reject),
            decide("1-1", Action::Revoke),
        ];

        let reviews = reviews(&suggestions, &decisions);

        assert_eq!(
            reviews[&SuggestionId("1-1".to_string())].status,
            Status::Pending
        );
    }

    #[test]
    fn evidence_cannot_be_empty() {
        assert!(Evidence::new(Vec::new()).is_none());
        assert!(Evidence::try_from(Vec::<LineId>::new()).is_err());
    }
}
