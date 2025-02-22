use std::{
    path::PathBuf,
    pin,
    process::Output
};

use applier::{Cli, CliParsed, ProcessError};
use anyhow::{Context, Error};
use futures::stream::StreamExt;

use tokio::{
    fs::ReadDir,
    io::{self, AsyncWriteExt, Stderr, Stdout},
    sync::{Mutex, MutexGuard}
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let (entries, cli) = Cli::parse()
        .await
        .context("Failed to parse CLI")?;

    #[cfg(feature = "json")]
    match cli.mode {
        applier::json::Mode::Standard => (),
        applier::json::Mode::Json => return execute_json(&cli, entries, serde_json::to_string).await,
        applier::json::Mode::JsonPretty => return execute_json(&cli, entries, serde_json::to_string_pretty).await,
    }

    execute_standard(&cli, entries).await;
    Ok(())
}

async fn execute_standard(cli: &CliParsed, entries: ReadDir) {
    let execute = async |
        processed: Result<(PathBuf, Output), ProcessError>,
        stdout: &mut MutexGuard<'_, Stdout>,
        stderr: &mut MutexGuard<'_, Stderr>,
    | {
        let (entry, output) = match processed {
            Ok(processed) => processed,
            Err(error) => {
                let error = Error::from(error).context("Failed to execute command");
                applier::async_write!(stderr, "{:?}\n", error);
                return;
            },
        };

        applier::async_write!(stdout, "{}:\n", entry.display());
        applier::async_write!(as [u8] => stdout, &output.stdout);

        if output.stderr.is_empty() {
            return;
        }

        applier::async_write!(stderr, "\nWarning:\n");
        applier::async_write!(as [u8] => stderr, &output.stderr);
    };

    let stdout = &Mutex::new(io::stdout());
    let stderr = &Mutex::new(io::stderr());

    let mut processed_entries = pin::pin!(cli.process(entries));

    if let Some(processed) = processed_entries.next().await {
        let mut stdout = stdout.lock().await;
        let mut stderr = stderr.lock().await;

        execute(processed, &mut stdout, &mut stderr).await;

        while let Some(processed) = processed_entries.next().await {
            applier::async_write!(stdout, "\n");
            execute(processed, &mut stdout, &mut stderr).await;
        }
    }

    applier::async_write!(flush => stdout.lock().await);
    applier::async_write!(flush => stderr.lock().await);
}

#[cfg(feature = "json")]
async fn execute_json<
    F: FnOnce(&applier::json::ProcessedEntries) -> Result<String, serde_json::Error>,
>(cli: &CliParsed, entries: ReadDir, formatter: F) -> anyhow::Result<()>
{
    let processed_entries = cli
        .process(entries)
        .collect::<applier::json::ProcessedEntries>()
        .await;

    let json = formatter(&processed_entries).context("Failed to JSONify outputs")?;

    let mut stdout = io::stdout();
    applier::async_write!(stdout, "{}\n", json);
    applier::async_write!(flush => stdout);

    Ok(())
}
