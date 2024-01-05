use std::{
    path::PathBuf,
    ffi::OsString,
    io::ErrorKind,
    process::{self, Stdio},
    env,
};

use clap::{Parser, Subcommand};

use tokio::{
    fs::{self, DirEntry},
    process::Command,
    io::{self, AsyncWriteExt},
    sync::Mutex,
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

    let directory = cli.path.unwrap_or_else(|| env::current_dir().unwrap_or_else(entries_error_handler));
    let entries = fs::read_dir(&directory)
        .await
        .unwrap_or_else(entries_error_handler);

    let ignored_directories = &*cli.ignore
        .into_iter()
        .map(|path| {
            if path.is_relative() {
                let mut ignored_directory = directory.clone();
                ignored_directory.push(path);
                fs::canonicalize(ignored_directory)
            } else {
                fs::canonicalize(path)
            }
        })
        .collect::<FuturesUnordered<_>>()
        .flat_map_unordered(None, stream::iter)
        .collect::<Vec<_>>()
        .await;

    let stderr = &Mutex::new(io::stderr());
    let stdout = &Mutex::new(io::stdout());
    ReadDirStream::new(entries).for_each_concurrent(None, |entry| async move {
        let error_message = 'exit: {
            let entry = match entry.as_ref().map(DirEntry::path) {
                Ok(entry)
                    if entry.is_dir()
                    && !stream::iter(ignored_directories).any(|path| {
                        let entry = &entry;
                        async move { fs::canonicalize(entry).await.is_ok_and(|entry| &entry == path) }
                    })
                    .await
                => entry,
                Ok(_) => return,
                Err(error) => break 'exit format!("Error occurred: {}", error),
            };

            match Command::new(command)
                .current_dir(&entry)
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .args(args)
                .spawn()
            {
                Ok(child) => match child.wait_with_output().await {
                    Ok(output) => {
                        let mut stdout = stdout.lock().await;
                        stdout
                            .write_all(format!("Command {:?} at {:?} returned {}\n", command, entry, output.status).as_bytes())
                            .await
                            .expect("failed writing to stdout");
                        stdout
                            .write_all(&output.stderr)
                            .await
                            .expect("failed writing to stdout");
                    },
                    Err(error) => break 'exit format!("Error occurred at {:?}: {}", entry, error),
                }
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    eprintln!("Command {:?} could not be found", command);
                    process::exit(1);
                },
                Err(error) => break 'exit format!("Error occurred at {:?}: {}", entry, error),
            }

            return;
        };

        stderr
            .lock()
            .await
            .write_all(error_message.as_bytes())
            .await
            .expect("failed writing to stderr");
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
