use super::Result;
use serde_json::Value;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const CACHE: &str = ".playlists";

fn curl(url: &str, extra_args: &[&str]) -> Result<String> {
    let out = Command::new("curl")
        .args([
            "-fsSL",
            "--max-time", "30",
            "--proto", "=https",
            "--proto-redir", "=https",
        ])
        .args(extra_args)
        .arg("--")
        .arg(url)
        .output()?;

    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned().into());
    }

    Ok(String::from_utf8(out.stdout)?)
}

fn deezer_json(url: &str) -> Result<Value> {
    let json: Value = serde_json::from_str(&curl(url, &[])?)?;

    if let Some(error) = json.get("error") {
        let message = error["message"].as_str().unwrap_or("Playlist inaccessible");
        return Err(format!("Deezer : {message}").into());
    }

    Ok(json)
}

pub(super) fn import_deezer(input: &str) -> Result<PathBuf> {
    let mut url = input.trim().to_owned();

    if url.starts_with("https://link.deezer.com/")
        || url.starts_with("https://deezer.page.link/")
    {
        let null_device = if cfg!(windows) { "NUL" } else { "/dev/null" };
        url = curl(&url, &["-o", null_device, "-w", "%{url_effective}"])?;
    }

    let path = [
        "https://www.deezer.com/",
        "https://deezer.com/",
        "https://m.deezer.com/",
    ]
    .iter()
    .find_map(|prefix| url.strip_prefix(*prefix))
    .ok_or("Colle le lien HTTPS d’une playlist publique Deezer.")?;

    let mut parts = path.split(['?', '#']).next().unwrap_or("").split('/');
    let id: u64 = parts
        .find(|part| *part == "playlist")
        .and_then(|_| parts.next())
        .ok_or("Le lien doit contenir /playlist/ suivi de son numéro.")?
        .parse()?;

    let base = format!("https://api.deezer.com/playlist/{id}");
    let info = deezer_json(&base)?;
    let title = info["title"].as_str().ok_or("Nom de playlist introuvable.")?;

    fs::create_dir_all(CACHE)?;
    let destination = Path::new(CACHE).join(format!("{id}.csv"));
    let mut tmp = tempfile::NamedTempFile::new_in(CACHE)?;
    let mut writer = csv::Writer::from_writer(tmp.as_file_mut());
    writer.write_record(["Track name", "Artist name"])?;

    let mut next = Some(format!("{base}/tracks?limit=100"));
    let mut seen_pages = HashSet::new();

    while let Some(url) = next {
        if !url.starts_with("https://api.deezer.com/") || !seen_pages.insert(url.clone()) {
            return Err("Pagination Deezer inattendue.".into());
        }

        let page = deezer_json(&url)?;
        let tracks = page["data"].as_array().ok_or("Liste de morceaux inaccessible.")?;

        for track in tracks {
            let title = track["title"].as_str().ok_or("Titre manquant.")?;
            let artist = track["artist"]["name"].as_str().ok_or("Artiste manquant.")?;
            writer.write_record([title, artist])?;
        }

        next = page["next"].as_str().map(|url| {
            url.replacen("http://api.deezer.com/", "https://api.deezer.com/", 1)
        });
    }

    writer.flush()?;
    drop(writer);
    tmp.persist(&destination)?;
    fs::write(destination.with_extension("txt"), title)?;

    Ok(destination)
}

fn playlist_name(path: &Path) -> String {
    if path == Path::new("playlist.csv") {
        return "Ma playlist (playlist.csv)".to_owned();
    }

    fs::read_to_string(path.with_extension("txt"))
        .unwrap_or_else(|_| path.display().to_string())
}

#[path = "tui.rs"]
mod tui;

pub(super) async fn launch() -> Result<()> {
    tui::launch().await
}