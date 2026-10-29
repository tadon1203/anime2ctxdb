mod answers;
mod files;
mod model;
mod package;
mod series;
mod sources;
mod speakers;

use anyhow::{Result, bail};
use std::path::PathBuf;

pub use answers::prompt::{Prompt, PromptKind};
pub use model::{Position, parse_clock};
pub use series::{DataView, EntityRow, EpisodeProgress, FactRow, Removed, SceneRow, Series};
pub use sources::{
    Detected, EpisodeOrigin, Failure, Language, Problem, Scan, Severity, Source, SourceId,
    SourceKind,
};

/// The app data folder. The series folders live in it, and so does the app lock.
/// The GUI and the CLI both use it. This does not create the folder.
pub fn app_data_dir() -> Result<PathBuf> {
    let Some(dir) = dirs::data_local_dir() else {
        bail!("no local data directory on this system");
    };
    Ok(dir.join("anime2ctxdb"))
}
