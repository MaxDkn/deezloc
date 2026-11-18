use super::{playlist_add, CACHE};
use crate::{read_tracks, track::Track, Result};
use serde_json::Value;
use std::{
    collections::HashSet,
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Output, Stdio},
};
use tokio::process::Command;

struct Source {
    url: String,
    kind: String,
    id: String,
}

fn source(input: &str) -> Result<Source> {
    let path = input.trim().strip_prefix("https://open.spotify.com/")
        .ok_or("Colle le lien https://open.spotify.com/playlist/… ou /album/….")?;
    let path = path.split(['?', '#']).next().unwrap_or("").trim_end_matches('/');
    let mut parts = path.split('/');
    let first = parts.next().unwrap_or("");
    let kind = if first.starts_with("intl-") { parts.next().unwrap_or("") } else { first };
    let id = parts.next().unwrap_or("");
    if !matches!(kind, "album" | "playlist") || id.len() != 22 ||
        !id.bytes().all(|c| c.is_ascii_alphanumeric()) || parts.next().is_some()
    {
        return Err("Un lien Spotify complet de playlist ou d’album est attendu.".into());
    }
    Ok(Source {
        url: format!("https://open.spotify.com/{kind}/{id}"),
        kind: kind.to_owned(),
        id: id.to_owned(),
    })
}

async fn export(executable: &Path, url: &str, file: &Path) -> io::Result<Output> {
    Command::new(executable).arg("save").arg(url).arg("--save-file").arg(file)
        .stdin(Stdio::null()).kill_on_drop(true).output().await
}

async fn run_spotdl(url: &str, file: &Path) -> Result<()> {
    let output = match export(Path::new("spotdl"), url, file).await {
        Ok(output) => output,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            // pipx peut être disponible dans le shell, mais absent du PATH du Dock.
            let home = env::var_os("HOME").ok_or("spotDL introuvable. Installe-le avec pipx install spotdl.")?;
            let executable = PathBuf::from(home).join(".local/bin/spotdl");
            export(&executable, url, file).await.map_err(|error|
                format!("spotDL introuvable ou impossible à lancer : {error}. Vérifie pipx ensurepath."))?
        }
        Err(error) => return Err(error.into()),
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let details = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };
        let message = details.lines().rev().take(6).collect::<Vec<_>>()
            .into_iter().rev().collect::<Vec<_>>().join("\n");
        return Err(format!("Import Spotify échoué ({}) : {message}", output.status).into());
    }
    Ok(())
}

fn decode(bytes: &[u8], source: &Source) -> Result<(String, Vec<Track>)> {
    let data: Value = serde_json::from_slice(bytes)?;
    let rows = data.as_array().ok_or("Le fichier spotDL doit contenir une liste de morceaux.")?;
    if rows.is_empty() {
        return Err("spotDL a renvoyé une liste vide. Vérifie le lien et l’accès à la playlist.".into());
    }
    // Un export partiel ne doit pas devenir silencieusement la liste locale.
    for row in rows {
        if let Some(total) = row["list_length"].as_u64().filter(|&n| n > 0) {
            if total != rows.len() as u64 {
                return Err(format!("Export Spotify incomplet : {} titres reçus, {total} annoncés.", rows.len()).into());
            }
        }
    }
    let fallback = format!("Spotify {} {}", source.kind, source.id);
    let name = rows.iter().find_map(|row| {
        let matches = row["list_url"].as_str().and_then(|url| self::source(url).ok())
            .is_some_and(|other| other.url == source.url);
        matches.then(|| row["list_name"].as_str()).flatten().filter(|name| !name.trim().is_empty())
    }).unwrap_or(&fallback).to_owned();
    let mut ordered: Vec<&Value> = rows.iter().collect();
    if source.kind == "album" && rows.iter().all(|row| row["track_number"].as_u64().is_some()) {
        ordered.sort_by_key(|row| (
            row["disc_number"].as_u64().unwrap_or(1),
            row["track_number"].as_u64().unwrap_or(0),
        ));
    } else if rows.iter().all(|row| row["list_position"].as_u64().is_some()) {
        ordered.sort_by_key(|row| row["list_position"].as_u64().unwrap_or(0));
    }
    let mut tracks = Vec::new();
    for (index, row) in ordered.iter().enumerate() {
        let title = row["name"].as_str().unwrap_or("").trim();
        let artists = row["artists"].as_array().map(|values| values.iter()
            .filter_map(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
            .collect::<Vec<_>>().join(", ")).unwrap_or_default();
        let artist = if artists.is_empty() {
            row["artist"].as_str().unwrap_or("").trim().to_owned()
        } else { artists };
        if title.is_empty() || artist.is_empty() {
            return Err(format!("Morceau {} : titre ou artiste manquant dans l’export Spotify.", index + 1).into());
        }
        tracks.push(Track { title: title.to_owned(), artist });
    }
    Ok((name, tracks))
}

pub(super) async fn import(input: &str) -> Result<PathBuf> {
    let source = source(input)?;
    let temporary = tempfile::tempdir()?;
    let exported = temporary.path().join("playlist.spotdl");
    run_spotdl(&source.url, &exported).await?;
    let bytes = fs::read(&exported).map_err(|e| format!("Export spotDL inaccessible : {e}"))?;
    let (name, tracks) = decode(&bytes, &source)?;

    let _lock = playlist_add::lock()?;
    let edits = crate::track_edits::load()?;
    let mut tracks = tracks.into_iter().map(|track| crate::track_edits::resolve(track, &edits))
        .collect::<Result<Vec<_>>>()?;
    fs::create_dir_all(CACHE)?;
    let destination = Path::new(CACHE).join(format!("spotify-{}-{}.csv", source.kind, source.id));
    let mut seen: HashSet<String> = tracks.iter().map(Track::key).collect();
    // Conserver les ajouts locaux lors d’un nouvel import du même lien.
    if destination.try_exists()? {
        for track in read_tracks(&destination, None)? {
            if seen.insert(track.key()) { tracks.push(track); }
        }
    }
    let mut csv_file = tempfile::NamedTempFile::new_in(CACHE)?;
    let mut writer = csv::Writer::from_writer(csv_file.as_file_mut());
    writer.write_record(["Track name", "Artist name"])?;
    for track in tracks { writer.write_record([track.title, track.artist])?; }
    writer.flush()?;
    drop(writer);
    let mut name_file = tempfile::NamedTempFile::new_in(CACHE)?;
    name_file.write_all(format!("{name} · Spotify").as_bytes())?;
    name_file.flush()?;
    name_file.persist(destination.with_extension("txt"))?;
    csv_file.persist(&destination)?;
    Ok(destination)
}


#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ALBUM: &str = "https://open.spotify.com/album/26bQ8ka40C7SjHhqyNrqoj";

    #[test]
    fn accepts_shared_and_localized_urls() {
        let link = source("https://open.spotify.com/intl-fr/album/26bQ8ka40C7SjHhqyNrqoj?si=123").unwrap();
        assert_eq!(link.url, ALBUM);
        assert!(source("https://open.spotify.com.evil/album/26bQ8ka40C7SjHhqyNrqoj").is_err());
        assert!(source("https://open.spotify.com/track/26bQ8ka40C7SjHhqyNrqoj").is_err());
    }

    #[test]
    fn keeps_all_artists_and_restores_album_order() {
        let data = json!([
            {"name": "Morning Mood", "artists": ["Edvard Grieg", "Cyprien Katsaris"],
             "list_name": "Piano", "list_url": ALBUM, "list_length": 2, "track_number": 2},
            {"name": "Moonlight", "artist": "Beethoven", "list_length": 2, "track_number": 1}
        ]);
        let (name, tracks) = decode(&serde_json::to_vec(&data).unwrap(), &source(ALBUM).unwrap()).unwrap();
        assert_eq!(name, "Piano");
        assert_eq!(tracks[0].title, "Moonlight");
        assert_eq!(tracks[1].artist, "Edvard Grieg, Cyprien Katsaris");
    }

    #[test]
    fn rejects_partial_or_invalid_exports() {
        for data in [
            json!([]),
            json!([{"name": "Title", "artist": "Artist", "list_length": 500}]),
            json!([{"name": "Title"}]),
        ] {
            assert!(decode(&serde_json::to_vec(&data).unwrap(), &source(ALBUM).unwrap()).is_err());
        }
    }
}
