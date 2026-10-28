use anime2ctxdb::{PromptKind, Series, app_data_dir};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Makes prompts and saves answers for a series.
#[derive(Debug, Parser)]
#[command(name = "anime2ctxdb", version)]
struct Cli {
    /// The folder that holds the series. The default is the app data folder.
    #[arg(long)]
    series_root: Option<PathBuf>,

    /// The name of the series folder inside the series root.
    #[arg(long)]
    series: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Works with prompts.
    Prompt {
        #[command(subcommand)]
        command: PromptCommand,
    },
    /// Saves answers to prompts.
    Answer {
        #[command(subcommand)]
        command: AnswerCommand,
    },
    /// Lists the episodes that have a Ja subtitle, and whether each has an answer of each kind.
    Status,
}

#[derive(Debug, Subcommand)]
enum PromptCommand {
    /// Prints the prompt for one or more episodes. The prompt is not saved.
    Show {
        #[arg(long)]
        kind: KindArg,

        /// The episode numbers, such as 1,2,3.
        #[arg(long, value_delimiter = ',', required = true)]
        episodes: Vec<u32>,
    },
}

#[derive(Debug, Subcommand)]
enum AnswerCommand {
    /// Saves the answer to the prompt for one or more episodes. The answer is read from FILE, or
    /// from stdin if FILE is missing.
    Submit {
        #[arg(long)]
        kind: KindArg,

        /// The episode numbers, such as 1,2,3. They must be the ones that the prompt had.
        #[arg(long, value_delimiter = ',', required = true)]
        episodes: Vec<u32>,

        file: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum KindArg {
    Speakers,
    Story,
}

impl From<KindArg> for PromptKind {
    fn from(kind: KindArg) -> PromptKind {
        match kind {
            KindArg::Speakers => PromptKind::Speakers,
            KindArg::Story => PromptKind::Story,
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let root = match cli.series_root {
        Some(root) => root,
        None => app_data_dir()?,
    };
    let series = Series::open(&root.join(&cli.series))?;
    match cli.command {
        Command::Prompt { command } => prompt(&series, command),
        Command::Answer { command } => answer(&series, command),
        Command::Status => status(&series),
    }
}

fn prompt(series: &Series, command: PromptCommand) -> Result<()> {
    match command {
        PromptCommand::Show { kind, episodes } => {
            let prompt = series.prompt(kind.into(), &episodes)?;
            print!("{}", prompt.text);
        }
    }
    Ok(())
}

fn answer(series: &Series, command: AnswerCommand) -> Result<()> {
    match command {
        AnswerCommand::Submit {
            kind,
            episodes,
            file,
        } => {
            let text = read_answer(file.as_deref())?;
            let count = series.submit_answer(kind.into(), &episodes, &text)?;
            println!("saved {count} suggestions");
        }
    }
    Ok(())
}

/// Prints one line for each episode that has a Ja subtitle: the episode, then the state of the
/// speakers answer and of the story answer, separated by tabs.
fn status(series: &Series) -> Result<()> {
    let speakers = series.answered_episodes(PromptKind::Speakers)?;
    let story = series.answered_episodes(PromptKind::Story)?;
    let state = |answered: bool| if answered { "answered" } else { "pending" };
    for item in series.progress()?.iter().filter(|item| item.has_subtitle) {
        println!(
            "{}\tspeakers={}\tstory={}",
            item.episode,
            state(speakers.contains(&item.episode)),
            state(story.contains(&item.episode))
        );
    }
    Ok(())
}

/// Reads the answer text from `file`, or from stdin when there is no file.
fn read_answer(file: Option<&Path>) -> Result<String> {
    if let Some(path) = file {
        return fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()));
    }
    let mut text = String::new();
    io::stdin()
        .read_to_string(&mut text)
        .context("cannot read the answer from stdin")?;
    Ok(text)
}
