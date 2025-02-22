#[cfg(feature = "json")]
pub mod json;

#[macro_use]
mod macros;

use std::{
    collections::HashSet,
    env,
    ffi::OsString,
    io,
    path::PathBuf,
    process::{Output, Stdio},
};

use clap::{Parser, Subcommand};
use futures::stream::{self, Stream, StreamExt, TryStreamExt};
use thiserror::Error;

use tokio::{
    fs::{self, DirEntry, ReadDir},
    process::Command,
    time::{self, Duration},
};

use tokio_stream::wrappers::ReadDirStream;

#[derive(Parser)]
#[command(name = "subdo", version, about = "A CLI for applying a command to directories within a directory", long_about = None)]
pub struct Cli {
    /// A path to specify for the parent directory
    #[arg(long)]
    path: Option<PathBuf>,
    /// The paths of the children directories to ignore
    #[arg(short, long, value_name = "PATH", value_delimiter = ' ', num_args = 1..)]
    ignore: Vec<PathBuf>,
    /// Max number of concurrent tasks
    #[arg(short, long, default_value_t = num_cpus::get() as u16, value_parser = clap::value_parser!(u16).range(1..))]
    jobs: u16,
    /// Max duration for any given process
    #[arg(short, long, value_parser = humantime::parse_duration)]
    timeout: Option<Duration>,
    #[cfg(feature = "json")]
    /// Optional JSON representation
    #[arg(short, long, value_enum, default_value_t = json::Mode::Standard)]
    mode: json::Mode,
    /// The command to execute
    #[command(subcommand)]
    command: External,
}

#[derive(Subcommand)]
enum External {
    #[command(external_subcommand)]
    Command(Vec<OsString>),
}

pub struct CliParsed {
    pub command: (OsString, Vec<OsString>),
    pub directory: PathBuf,
    pub ignored_subdirectories: HashSet<PathBuf>,
    pub jobs: usize,
    pub timeout: Option<Duration>,
    #[cfg(feature = "json")]
    pub mode: json::Mode,
}

#[derive(Debug, Error)]
pub enum CliError {
    #[error("vacant specified command")]
    Command,
    #[error("current directory is unavailable as: {0}")]
    CurrentDirectory(io::Error),
    #[error("subdirectories are unavailable as: {0}")]
    SubDirectories(io::Error),
    #[error("an ignored directory ({entry}) is invalid as: {origin}", entry = .0.display(), origin = .1)]
    IgnoredDirectories(PathBuf, io::Error),
}

#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("invalid directory entry from likely modification")]
    ModifiedEntry,
    #[error("process {process} for {entry} made unavailable as: {origin}", process = .process.to_string_lossy(), entry = .entry.display())]
    ProcessSpawn { process: OsString, entry: PathBuf, origin: io::Error },
    #[error("process {process} for {entry} has unavailable output as: {origin}", process = .process.to_string_lossy(), entry = .entry.display())]
    ProcessOutput { process: OsString, entry: PathBuf, origin: io::Error },
    #[error("process {process} for {entry} could not complete in {duration}", process = .process.to_string_lossy(), entry = .entry.display())]
    Timeout { process: OsString, entry: PathBuf, duration: String },
}

impl Cli {
    pub async fn parse() -> Result<(ReadDir, CliParsed), CliError> {
        let cli: Cli = Parser::parse();

        #[cfg(feature = "json")]
        let mode = cli.mode;
        let jobs = usize::from(cli.jobs);
        let timeout = cli.timeout;

        let External::Command(command) = cli.command;
        let mut command = command.into_iter();

        let command = command
            .next()
            .map(|command_standalone| (command_standalone, command.collect::<Vec<_>>()))
            .ok_or(CliError::Command)?;

        let directory = cli
            .path
            .map(Ok)
            .unwrap_or_else(env::current_dir)
            .map_err(CliError::CurrentDirectory)?;

        let entries = fs::read_dir(&directory)
            .await
            .map_err(CliError::SubDirectories)?;

        let ignored_subdirectories = stream::iter(cli.ignore.into_iter())
            .map(|mut path| async {
                if path.is_relative() {
                    let mut ignored_directory = directory.clone();
                    ignored_directory.push(path);
                    path = ignored_directory;
                }

                fs::canonicalize(&path)
                    .await
                    .map_err(|error| CliError::IgnoredDirectories(path, error))
            })
            .buffer_unordered(jobs)
            .try_collect::<HashSet<_>>()
            .await?;

        Ok((entries, CliParsed {
            command,
            directory,
            ignored_subdirectories,
            jobs,
            timeout,
            #[cfg(feature = "json")]
            mode,
        }))
    }
}

impl CliParsed {
    pub fn process(&self, entries: ReadDir) -> impl Stream<Item = Result<(PathBuf, Output), ProcessError>> + use<'_> {
        ReadDirStream::new(entries)
            .map(|entry| self.process_initial(entry))
            .buffer_unordered(self.jobs)
            .filter_map(|processed| async { processed })
    }

    async fn process_initial(&self, entry: Result<DirEntry, io::Error>) -> Option<Result<(PathBuf, Output), ProcessError>> {
        let (entry_canonicalized, entry) = match entry.as_ref().map(DirEntry::path) {
            Ok(entry) => match fs::canonicalize(&entry).await {
                Ok(entry_canonicalized) => (entry_canonicalized, entry),
                Err(_) => return Some(Err(ProcessError::ModifiedEntry)),
            },
            Err(_) => return Some(Err(ProcessError::ModifiedEntry)),
        };

        if !entry.is_dir()
        || self.ignored_subdirectories.contains(&entry_canonicalized)
        {
            return None;
        }

        let child = match Command::new(&self.command.0)
            .current_dir(&entry)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .args(&self.command.1)
            .spawn()
        {
            Ok(child) => child,
            Err(error) => return Some(Err(ProcessError::ProcessSpawn {
                process: self.command.0.clone(),
                entry,
                origin: error,
            })),
        };

        let output = match self.timeout {
            Some(duration) => match time::timeout(duration, child.wait_with_output()).await {
                Ok(output) => output,
                Err(_) => return Some(Err(ProcessError::Timeout {
                    process: self.command.0.clone(),
                    entry,
                    duration: humantime::format_duration(duration).to_string(),
                })),
            },
            None => child.wait_with_output().await,
        };

        match output {
            Ok(output) => Some(Ok((entry, output))),
            Err(error) => Some(Err(ProcessError::ProcessOutput {
                process: self.command.0.clone(),
                entry,
                origin: error,
            })),
        }
    }
}
