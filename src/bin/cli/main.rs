use anime2ctxdb::{Prompt, PromptId, PromptKind, PromptRow, Series, app_data_dir};
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
}

#[derive(Debug, Subcommand)]
enum PromptCommand {
    /// Writes a prompt for one or more episodes and prints its ID.
    Create {
        #[arg(long)]
        kind: KindArg,

        /// The episode numbers, such as 1,2,3.
        #[arg(long, value_delimiter = ',', required = true)]
        episodes: Vec<u32>,
    },
    /// Lists the prompts, one per line: ID, kind, episodes, and state.
    List {
        /// Show only the prompts that have no answer.
        #[arg(long)]
        pending: bool,
    },
    /// Prints the text of one prompt.
    Show { id: u32 },
}

#[derive(Debug, Subcommand)]
enum AnswerCommand {
    /// Saves the answer to a prompt. The answer is read from FILE, or from stdin if FILE is missing.
    Submit { id: u32, file: Option<PathBuf> },
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
    }
}

fn prompt(series: &Series, command: PromptCommand) -> Result<()> {
    match command {
        PromptCommand::Create { kind, episodes } => {
            let prompt = series.create_prompt(kind.into(), &episodes)?;
            println!("{}", prompt.id);
        }
        PromptCommand::List { pending } => {
            for row in series.prompts()? {
                if pending && row.answered {
                    continue;
                }
                println!("{}", list_line(&row));
            }
        }
        PromptCommand::Show { id } => {
            let prompt: Prompt = series.prompt(PromptId(id))?;
            print!("{}", prompt.text);
        }
    }
    Ok(())
}

fn answer(series: &Series, command: AnswerCommand) -> Result<()> {
    match command {
        AnswerCommand::Submit { id, file } => {
            let text = read_answer(file.as_deref())?;
            let count = series.submit_answer(PromptId(id), &text)?;
            println!("saved {count} suggestions for prompt {id}");
        }
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

/// One line of `prompt list`: the ID, the kind, the episodes, and the state, separated by tabs.
fn list_line(row: &PromptRow) -> String {
    let kind = match row.prompt.kind {
        PromptKind::Speakers => "speakers",
        PromptKind::Story => "story",
    };
    let episodes = row
        .prompt
        .episodes
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let state = if row.answered { "answered" } else { "pending" };
    format!("{}\t{kind}\t{episodes}\t{state}", row.prompt.id)
}
