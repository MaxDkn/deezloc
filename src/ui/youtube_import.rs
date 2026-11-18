use super::{collection::local_playlists, playlist_name};
use crate::{config::MUSIC_DIR, read_tracks, track::Track, youtube::youtube_url, Result};
use futures::{stream, StreamExt};
use serde_json::Value;
use std::{collections::HashMap, fs, path::{Path, PathBuf}};
use tokio::process::Command;

pub(super) async fn identify(url: &str) -> Result<Track> {
    let out = Command::new("yt-dlp")
        .args(["--ignore-config", "--no-playlist", "--skip-download", "--dump-single-json", "--"])
        .arg(url).output().await?;
    if !out.status.success() {
        return Err(format!("YouTube : {}", String::from_utf8_lossy(&out.stderr).trim()).into());
    }
    let data: Value = serde_json::from_slice(&out.stdout)?;
    let text = |key: &str| data[key].as_str().filter(|s| !s.trim().is_empty());
    let video_title = text("title").unwrap_or("");
    let pair = [" - ", " — ", " – "].iter().find_map(|separator| video_title.split_once(*separator));
    let title = text("track").unwrap_or_else(|| pair.map_or(video_title, |(_, title)| title));
    let artist = text("artist").map(str::to_owned).or_else(|| {
        data["artists"].as_array().map(|artists| artists.iter()
            .filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).filter(|s| !s.is_empty())
    }).unwrap_or_else(|| pair.map(|(artist, _)| artist).unwrap_or(
        text("channel").or_else(|| text("uploader")).unwrap_or("")
    ).trim_end_matches(" - Topic").to_owned());
    Ok(Track { title: title.trim().to_owned(), artist: artist.trim().to_owned() })
}

pub(super) struct Existing {
    pub track: Option<Track>,
    pub label: String,
    pub path: PathBuf,
    pub playlists: Vec<String>,
    pub reason: &'static str,
}

fn normalized(text: &str) -> String {
    text.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

fn similar(first: &str, second: &str) -> bool {
    let (first, second) = (normalized(first), normalized(second));
    !first.is_empty() && (first == second ||
        (first.len().min(second.len()) >= 5 &&
            (first.contains(&second) || second.contains(&first))))
}

pub(super) async fn duplicates(csv: &Path, track: &Track, url: &str) -> Result<Vec<Existing>> {
    let mut known: HashMap<PathBuf, (Track, Vec<String>)> = HashMap::new();
    let mut playlists = local_playlists()?;
    playlists.push(csv.to_owned());
    let mut visited = std::collections::HashSet::new();
    for playlist in playlists {
        if !visited.insert(fs::canonicalize(&playlist)?) { continue; }
        for item in read_tracks(&playlist, None)? {
            for extension in ["opus", "mp3"] {
                let path = Path::new(MUSIC_DIR).join(item.filename()).with_extension(extension);
                known.entry(path).or_insert_with(|| (item.clone(), Vec::new()))
                    .1.push(playlist_name(&playlist));
            }
        }
    }
    let mut paths = Vec::new();
    for entry in fs::read_dir(MUSIC_DIR)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_file() &&
            path.extension().and_then(|s| s.to_str()).is_some_and(
                |s| s.eq_ignore_ascii_case("opus") || s.eq_ignore_ascii_case("mp3"))
        {
            paths.push(path);
        }
    }
    let mut jobs = stream::iter(paths).map(|path| {
        let known = &known;
        async move {
            let out = Command::new("ffprobe").args([
                "-v", "error", "-show_entries", "format_tags:stream_tags", "-of", "json"
            ]).arg(&path).output().await?;
            let data: Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
            let tags = std::iter::once(&data["format"]["tags"])
                .chain(data["streams"].as_array().into_iter().flatten().map(|s| &s["tags"]));
            let same_url = tags.filter_map(Value::as_object).flat_map(|t| t.iter())
                .filter(|(key, _)| key.eq_ignore_ascii_case("source_url"))
                .filter_map(|(_, value)| value.as_str())
                .any(|source| youtube_url(source).is_ok_and(|source| source == url));
            let expected = Path::new(MUSIC_DIR).join(track.filename());
            let exact = path == expected || path == expected.with_extension("mp3");
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let label = stem.rsplit_once(" [").map_or(stem, |(label, _)| label);
            let probable = known.get(&path).map_or_else(
                || similar(label, &format!("{} {}", track.artist, track.title)),
                |(item, _)| similar(&item.artist, &track.artist) && similar(&item.title, &track.title)
            );
            if !same_url && !exact && !probable { return Ok(None); }
            let item = known.get(&path);
            Ok(Some(Existing {
                track: item.map(|(item, _)| item.clone()).or_else(|| exact.then(|| track.clone())),
                label: item.map(|(item, _)| format!("{} — {}", item.artist, item.title))
                    .unwrap_or_else(|| label.to_owned()),
                playlists: item.map(|(_, names)| names.clone()).unwrap_or_default(),
                path: fs::canonicalize(&path)?,
                reason: if same_url { "Même lien YouTube" }
                    else if exact { "Même artiste et même titre" } else { "Titre ressemblant" },
            }))
        }
    }).buffer_unordered(8);
    let mut matches = Vec::new();
    while let Some(result) = jobs.next().await {
        let result: Result<Option<Existing>> = result;
        if let Some(item) = result? { matches.push(item); }
    }
    matches.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(matches)
}
