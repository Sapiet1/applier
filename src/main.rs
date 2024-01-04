use std::{
    path::PathBuf,
    ffi::OsString,
    io::{self, ErrorKind},
    process,
    env,
};

use clap::{Parser, Subcommand};

use tokio::{
    fs::{self, DirEntry},
    process::Command,
};

use tokio_stream::wrappers::ReadDirStream;

use futures::{
    StreamExt,
    stream::{self, FuturesUnordered},
};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let External::Command(command) = cli.command;
    let [command, args @ ..] = command.as_slice() else { unreachable!() };

    fn entries_error_handler<T>(error: io::Error) -> T {
        eprintln!("Failed while reading the parent directory: {}", error);
        process::exit(1)
    }

    let entries = fs::read_dir(cli.path.unwrap_or_else(|| env::current_dir().unwrap_or_else(entries_error_handler)))
        .await
        .unwrap_or_else(entries_error_handler);

    let ignored_directories = &*cli.ignore
        .into_iter()
        .map(fs::canonicalize)
        .collect::<FuturesUnordered<_>>()
        .flat_map_unordered(None, stream::iter)
        .collect::<Vec<_>>()
        .await;

    ReadDirStream::new(entries).for_each_concurrent(None, |entry| {
        let entry = entry;
        async move {
            match entry.as_ref().map(DirEntry::path) {
                Ok(entry)
                    if entry.is_dir() 
                    && !stream::iter(ignored_directories).any(|path| {
                        let entry = &entry;
                        async move { fs::canonicalize(entry).await.is_ok_and(|entry| &entry == path) }
                    })
                    .await
                => { 
                    match Command::new(command)
                        .current_dir(&entry)
                        .args(args)
                        .spawn()
                    {
                        Err(error) if error.kind() == ErrorKind::NotFound => {
                            eprintln!("Command {:?} could not be found", command);
                            process::exit(1);
                        },
                        Err(error) => eprintln!("Error occurred at {:?}: {}", entry, error),
                        Ok(mut child) => {
                            match child.wait().await {
                                Ok(exit_status) => println!("Command {:?} at {:?} exited with: {}", command, entry, exit_status),
                                Err(error) => eprintln!("Error occurred at {:?}: {}", entry, error),
                            }
                        },
                    }
                },
                Err(error) => eprintln!("Error occurred at {:?}: {}", entry, error),
                _ => (),
            }
        }
    })
    .await;
}

#[derive(Parser)]
#[command(name = "applier", version, about = "A CLI for applying a command to directories within a directory", long_about = None)]
struct Cli {
    /// A path to specify for the parent directory
    #[arg(long, require_equals = true)]
    path: Option<PathBuf>,
    /// A path of the children directories to ignore
    #[arg(short, long, require_equals = true, value_name = "PATH")]
    ignore: Vec<PathBuf>,
    /// The command to execute
    #[command(subcommand)]
    command: External,
}

#[derive(Subcommand)]
enum External {
    #[command(external_subcommand)]
    Command(Vec<OsString>),
}
