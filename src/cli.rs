use crate::{config::{MUSIC_DIR, MAX_DOWNLOADS}, download::{download, Status}, menu, read_tracks, Result};
use fs2::FileExt;
use futures::{stream, StreamExt};
use std::{env, fs, path::{Path, PathBuf}};

async fn download_playlist(csv: &Path, limit: Option<usize>) -> Result<bool> {
    use std::io::{self, Write};
    let tracks = read_tracks(csv, limit)?;
    let total = tracks.len();
    if total == 0 { println!("Aucun morceau sélectionné."); return Ok(true); }
    let dir = Path::new(MUSIC_DIR);
    fs::create_dir_all(dir)?;
    let lock = fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(".lock"))?;
    FileExt::try_lock_exclusive(&lock)?;
    let mut jobs = stream::iter(tracks).map(|track| async move {
        let result = download(&track, dir).await;
        (track, result)
    }).buffer_unordered(MAX_DOWNLOADS);
    let (mut saved, mut present) = (0, 0);
    let mut failures = Vec::new();
    while let Some((track, result)) = jobs.next().await {
        match result {
            Ok(Status::Downloaded) => saved += 1,
            Ok(Status::AlreadyPresent) => present += 1,
            Err(_) => failures.push(format!("{} — {}", track.artist, track.title)),
        }
        let done = saved + present + failures.len();
        let filled = 30 * done / total;
        eprint!("\r[{}{}] {done}/{total}  {:3} %", "█".repeat(filled), "░".repeat(30-filled), 100*done/total);
        io::stderr().flush()?;
    }
    eprintln!();
    println!("Téléchargés : {saved} | Déjà présents : {present} | Échecs : {}", failures.len());
    failures.sort();
    for name in &failures { println!("- {name}"); }
    Ok(failures.is_empty())
}

pub(crate) async fn run() -> Result<bool> {
    crate::file_transaction::recover()?;
    let mut args = env::args_os().skip(1);
    let Some(source) = args.next() else { menu::launch().await?; return Ok(true) };
    let n = args.next().map(|s| s.to_string_lossy().parse::<usize>()).transpose()
        .map_err(|_| "N doit être un entier positif ou nul.")?;
    if args.next().is_some() { return Err("Usage : deezloc [playlist.csv ou URL Deezer] [N]".into()); }
    let path = if source.to_string_lossy().starts_with("https://") {
        menu::import_deezer(&source.to_string_lossy())?
    } else { PathBuf::from(source) };
    download_playlist(&path, n).await
}
