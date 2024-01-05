use std::{
    env,
    ffi::OsString,
    io::ErrorKind,
    path::PathBuf,
    process::{self, Stdio},
};

use clap::{Parser, Subcommand};

use tokio::{
    fs::{self, DirEntry},
    io::{self, AsyncWriteExt},
    process::Command,
    sync::Mutex,
};

use tokio_stream::wrappers::ReadDirStream;

use futures::{future, stream, StreamExt};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let External::Command(command) = cli.command;
    let [command, args @ ..] = command.as_slice() else {
        unreachable!()
    };

    let parent_error = |error: io::Error| -> ! {
        handler::exit!("Failed while reading the parent directory: {}", error)
    };

    let directory = cli
        .path
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|error| parent_error(error)));

    let entries = fs::read_dir(&directory)
        .await
        .unwrap_or_else(|error| parent_error(error));

    let ignored_directories = &*future::try_join_all(cli.ignore.into_iter().map(|path| {
        if path.is_relative() {
            let mut ignored_directory = directory.clone();
            ignored_directory.push(path);
            fs::canonicalize(ignored_directory)
        } else {
            fs::canonicalize(path)
        }
    }))
    .await
    .unwrap_or_else(|error| handler::exit!("Failed while reading ignored directories: {}", error));

    let stderr = &Mutex::new(io::stderr());
    let stdout = &Mutex::new(io::stdout());
    ReadDirStream::new(entries)
        .for_each_concurrent(None, |entry| async move {
            let result = async move {
                let entry = entry
                    .as_ref()
                    .map(DirEntry::path)
                    .map_err(|error| format!("Error occurred: {}", error))?;

                let ignore = !entry.is_dir()
                    || stream::iter(ignored_directories)
                        .any(|path| {
                            let entry = &entry;
                            async move {
                                fs::canonicalize(entry)
                                    .await
                                    .is_ok_and(|entry| &entry == path)
                            }
                        })
                        .await;

                if ignore {
                    return Ok(());
                }

                let output = Command::new(command)
                    .current_dir(&entry)
                    .stdout(Stdio::null())
                    .stderr(Stdio::piped())
                    .args(args)
                    .spawn()
                    .map_err(|error| {
                        if error.kind() == ErrorKind::NotFound {
                            handler::exit!("Command {:?} could not be found", command)
                        } else {
                            format!("Error occurred at {:?}: {}", entry, error)
                        }
                    })?
                    .wait_with_output()
                    .await
                    .map_err(|error| format!("Error occurred at {:?}: {}", entry, error))?;

                let mut stdout = stdout.lock().await;
                stdout
                    .write_all(
                        format!(
                            "Command {:?} at {:?} returned {}\n",
                            command, entry, output.status
                        )
                        .as_bytes(),
                    )
                    .await
                    .expect("failed writing to stdout");
                stdout
                    .write_all(&output.stderr)
                    .await
                    .expect("failed writing to stdout");

                Ok::<(), String>(())
            }
            .await;

            if let Err(error_message) = result {
                stderr
                    .lock()
                    .await
                    .write_all(error_message.as_bytes())
                    .await
                    .expect("failed writing to stderr");
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

mod handler {
    macro_rules! exit {
        ($( $tokens:tt )*) => {
            {
                eprintln!($( $tokens )*);
                process::exit(1);
            }
        };
    }

    pub(crate) use exit;
}
