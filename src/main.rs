mod audio;
mod file_transaction;
mod track_edits;
mod cli;
mod config;
mod download;
mod menu;
mod track;
mod youtube;

use std::process::ExitCode;
use track::{read_tracks, Track};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match cli::run().await {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => { eprintln!("{e}"); ExitCode::FAILURE }
    }
}
