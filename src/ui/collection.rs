use super::{playlist_name, CACHE};
use crate::{config::MUSIC_DIR, download::encode_source, track::{read_tracks, Song, Track}, Result};
use fs2::FileExt;
use std::{fs::{self, File}, io, path::{Path, PathBuf}};

fn edit_lock() -> Result<File> {
    let lock = fs::OpenOptions::new().create(true).truncate(false).write(true)
        .open(Path::new(MUSIC_DIR).join(".lock"))?;
    FileExt::try_lock_exclusive(&lock).map_err(|e| format!("Bibliothèque occupée : {e}"))?;
    Ok(lock)
}

pub(super) fn other_playlists(csv: &Path, song: &Song) -> Result<Vec<String>> {
    let mut names = Vec::new();
    let current = fs::canonicalize(csv)?;
    let key = song.track.key();
    for path in local_playlists()? {
        if fs::canonicalize(&path)? != current &&
            read_tracks(&path, None)?.iter().any(|t| t.key() == key) {
            names.push(playlist_name(&path));
        }
    }
    Ok(names)
}

pub(super) fn remove_song(csv: &Path, song: &Song, allow_file_removal: bool) -> Result<()> {
    let _lock = edit_lock()?;
    let shared = !other_playlists(csv, song)?.is_empty();
    if !shared && !allow_file_removal {
        return Err("Les playlists ont changé. Rouvre la fiche pour confirmer la suppression du fichier.".into());
    }
    let key = song.track.key();
    let edits = crate::track_edits::load()?;
    let original = fs::read(csv)?;
    let bytes = original.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&original);
    let parent = csv.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    for delimiter in [b',', b';', b'\t'] {
        let mut reader = csv::ReaderBuilder::new().delimiter(delimiter).from_reader(bytes);
        let Ok(header) = reader.headers().cloned() else { continue };
        let column = |name: &str| header.iter().position(|h| h.trim().eq_ignore_ascii_case(name));
        let (Some(t), Some(a)) = (column("Track name"), column("Artist name")) else { continue };
        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        if bytes.len() != original.len() {
            std::io::Write::write_all(tmp.as_file_mut(), &[0xef, 0xbb, 0xbf])?;
        }
        let mut out = csv::WriterBuilder::new().delimiter(delimiter).from_writer(tmp.as_file_mut());
        out.write_record(&header)?;
        let mut removed = false;
        for row in reader.records() {
            let row = row?;
            let track = Track { title: row[t].trim().into(), artist: row[a].trim().into() };
            let track = crate::track_edits::resolve(track, &edits)?;
            if track.key() == key { removed = true; } else { out.write_record(&row)?; }
        }
        if !removed { return Err("La playlist a changé. Rouvre-la avant de réessayer.".into()); }
        out.flush()?;
        drop(out);
        tmp.persist(csv)?;
        if !shared {
            for ext in ["opus", "mp3"] {
                let path = song.path.with_extension(ext);
                if let Err(e) = fs::remove_file(&path) {
                    if e.kind() != io::ErrorKind::NotFound {
                        return Err(format!("Titre retiré ; fichier conservé : {} ({e})", path.display()).into());
                    }
                }
            }
        }
        return Ok(());
    }
    Err("Colonnes de la playlist introuvables.".into())
}

pub(super) async fn correct_song(song: &Song, url: &str) -> Result<()> {
    let _lock = edit_lock()?;
    let tmp = tempfile::tempdir_in(MUSIC_DIR)?;
    let encoded = encode_source(tmp.path(), url).await?;
    // Conserver aussi le titre et l'artiste corrigés dans le nouveau fichier.
    let tagged = tmp.path().join("corrected.opus");
    let out = tokio::process::Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-i"]).arg(&encoded)
        .args(["-map", "0", "-map_metadata", "0", "-c", "copy", "-metadata"])
        .arg(format!("title={}", song.track.title))
        .arg("-metadata").arg(format!("artist={}", song.track.artist))
        .arg(&tagged).output().await?;
    if !out.status.success() {
        return Err(format!("Métadonnées audio : {}", String::from_utf8_lossy(&out.stderr).trim()).into());
    }
    let mut changes = vec![(song.path.with_extension("opus"), Some(tagged))];
    let mp3 = song.path.with_extension("mp3");
    if mp3.try_exists()? { changes.push((mp3, None)); }
    crate::file_transaction::apply(&changes)?;
    Ok(())
}

pub(super) fn music_size() -> io::Result<String> {
    let files = match fs::read_dir(MUSIC_DIR) {
        Ok(files) => files,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok("0 Mo".into()),
        Err(e) => return Err(e),
    };
    let mut bytes = 0u64;
    for file in files {
        let file = file?;
        let path = file.path();
        if path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("mp3") || ext.eq_ignore_ascii_case("opus"))
            && file.file_type()?.is_file()
        {
            bytes += file.metadata()?.len();
        }
    }
    if bytes >= 1_000_000_000 {
        Ok(format!("{:.1} Go", bytes as f64 / 1_000_000_000.0))
    } else {
        Ok(format!("{:.0} Mo", bytes as f64 / 1_000_000.0))
    }
}

pub(super) fn local_playlists() -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();

    if Path::new("playlist.csv").is_file() {
        paths.push(PathBuf::from("playlist.csv"));
    }

    if Path::new(CACHE).is_dir() {
        for entry in fs::read_dir(CACHE)? {
            let path = entry?.path();
            if path.extension().and_then(|s| s.to_str()) == Some("csv") {
                paths.push(path);
            }
        }
    }

    paths.sort_by_cached_key(|p| playlist_name(p).to_lowercase());
    Ok(paths)
}

// Recalculer les références sous verrou. Une playlist illisible annule tout.
pub(super) fn remove_playlist(csv: &Path) -> Result<Vec<String>> {
    use std::collections::HashSet;
    let _lock = super::playlist_add::lock()?;
    let current = fs::canonicalize(csv)?;
    let tracks = read_tracks(csv, None)?;
    let mut shared = HashSet::new();
    for other in local_playlists()? {
        if fs::canonicalize(&other)? == current { continue; }
        shared.extend(read_tracks(&other, None)?.into_iter().map(|t| t.key()));
    }
    let exclusive = exclusive_tracks(&tracks, &shared);
    let mut changes = Vec::new();
    for track in &exclusive {
        for extension in ["opus", "mp3"] {
            let path = Path::new(MUSIC_DIR).join(track.filename()).with_extension(extension);
            if path.try_exists()? { changes.push((path, None)); }
        }
    }
    changes.push((csv.to_owned(), None));
    for extension in ["txt", "name"] {
        let path = csv.with_extension(extension);
        if path.try_exists()? { changes.push((path, None)); }
    }
    crate::file_transaction::apply(&changes)?;
    Ok(exclusive.iter().map(|t| t.key()).collect())
}

fn exclusive_tracks<'a>(tracks: &'a [Track], shared: &std::collections::HashSet<String>) -> Vec<&'a Track> {
    tracks.iter().filter(|t| !shared.contains(&t.key())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deletion_preserves_shared_tracks_and_ignores_title_case() {
        let shared = Track { title: "Shared song".into(), artist: "Artist".into() };
        let same = Track { title: " SHARED   SONG ".into(), artist: "ARTIST".into() };
        let unique = Track { title: "Only here".into(), artist: "Artist".into() };
        let tracks = vec![same, unique.clone()];
        let references = [shared.key()].into_iter().collect();
        let removed = exclusive_tracks(&tracks, &references);
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].key(), unique.key());
        let all_shared = tracks.iter().map(|t| t.key()).collect();
        assert!(exclusive_tracks(&tracks, &all_shared).is_empty());
    }
}
