use crate::files::{append_jsonl, read_jsonl};
use crate::sources::ass::LineId;
use crate::speakers::LabelKind;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::Path;

/// `<PromptId>-<n>`, where `n` is the position in the answer, from 1.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SuggestionId(pub String);

impl SuggestionId {
    /// The prompt number and the position. Sort by these two numbers, not by the text.
    ///
    /// # Panics
    ///
    /// Panics when the ID does not have the form `<prompt>-<n>`.
    pub fn sort_key(&self) -> (u32, u32) {
        let (prompt, position) = self
            .0
            .split_once('-')
            .expect("a suggestion ID has the form <prompt>-<n>");
        (
            prompt.parse().expect("the prompt part is a number"),
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

/// A reference to a character is a `CharacterId`. It can also be the `SuggestionId` of a new
/// character in the same answer. Both hold the text of a `CharacterId`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Content {
    Character {
        name: String,
        reading: Option<String>,
        evidence: Vec<LineId>,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionKind {
    Character,
    Label,
    Color,
    LineSpeaker,
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
            Content::Character { .. } => SuggestionKind::Character,
            Content::Label { .. } => SuggestionKind::Label,
            Content::Color { .. } => SuggestionKind::Color,
            Content::LineSpeaker { .. } => SuggestionKind::LineSpeaker,
        }
    }

    /// The suggestions that this suggestion refers to.
    fn dependencies(&self) -> Vec<SuggestionId> {
        let characters: Vec<&String> = match self {
            Content::Character { .. } => Vec::new(),
            Content::Label { character, .. } => character.iter().collect(),
            Content::Color { character, .. } => vec![character],
            Content::LineSpeaker { characters, .. } => characters.iter().collect(),
        };
        characters
            .into_iter()
            .map(|character| SuggestionId(character.clone()))
            .collect()
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

    fn character(id: &str) -> Suggestion {
        Suggestion {
            id: SuggestionId(id.to_string()),
            content: Content::Character {
                name: "n".to_string(),
                reading: None,
                evidence: vec![LineId("s:1".to_string())],
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
        let suggestions = [character("1-1"), label("1-2", "1-1")];

        let reviews = reviews(&suggestions, &[]);

        assert!(reviews[&SuggestionId("1-2".to_string())].blocked);
        assert_eq!(
            reviews[&SuggestionId("1-2".to_string())].status,
            Status::Pending
        );
    }

    #[test]
    fn a_label_is_invalid_when_its_character_is_rejected() {
        let suggestions = [character("1-1"), label("1-2", "1-1")];
        let decisions = [decide("1-1", Action::Reject)];

        let reviews = reviews(&suggestions, &decisions);

        assert_eq!(
            reviews[&SuggestionId("1-2".to_string())].status,
            Status::Invalid
        );
    }

    #[test]
    fn the_last_decision_wins() {
        let suggestions = [character("1-1")];
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
}
