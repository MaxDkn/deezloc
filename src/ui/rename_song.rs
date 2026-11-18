use super::{collection::local_playlists, playlist_add};
use crate::{audio::{Audio, AudioCommand}, file_transaction, track::{Song, Track}, track_edits, Result};
use std::{collections::HashSet, fs, io::Write, path::{Path, PathBuf}};

// Préserver les autres colonnes, l'ordre, le séparateur et le BOM de chaque CSV.
fn rewrite(csv: &Path, old: &Track, new: &Track) -> Result<Option<tempfile::NamedTempFile>> {
    let original = fs::read(csv)?;
    let bytes = original.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&original);
    let edits = track_edits::load()?;
    for delimiter in [b',', b';', b'\t'] {
        let mut reader = csv::ReaderBuilder::new().delimiter(delimiter).from_reader(bytes);
        let Ok(header) = reader.headers().cloned() else { continue; };
        let column = |name: &str| header.iter().position(|h| h.trim().eq_ignore_ascii_case(name));
        let (Some(title), Some(artist)) = (column("Track name"), column("Artist name")) else { continue; };
        let mut file = tempfile::NamedTempFile::new_in(csv.parent().unwrap_or(Path::new(".")))?;
        if bytes.len() != original.len() { file.write_all(&[0xef, 0xbb, 0xbf])?; }
        let mut writer = csv::WriterBuilder::new().delimiter(delimiter).from_writer(file.as_file_mut());
        writer.write_record(&header)?;
        let mut changed = false;
        for record in reader.records() {
            let record = record?;
            let item = track_edits::resolve(Track {
                title: record[title].trim().to_owned(), artist: record[artist].trim().to_owned(),
            }, &edits)?;
            let mut cells: Vec<String> = record.iter().map(str::to_owned).collect();
            if item.key() == old.key() {
                cells[title] = new.title.clone();
                cells[artist] = new.artist.clone();
                changed = true;
            } else if item.key() == new.key() {
                return Err(format!("Un autre morceau porte déjà ce titre et cet artiste dans {}.", csv.display()).into());
            }
            writer.write_record(cells)?;
        }
        writer.flush()?;
        drop(writer);
        return Ok(changed.then_some(file));
    }
    Err(format!("Colonnes de playlist introuvables : {}", csv.display()).into())
}

pub(super) async fn rename(
    csv: &Path, song: &Song, new: Track, audio: &Audio,
) -> Result<()> {
    if new.title.trim().is_empty() || new.artist.trim().is_empty() ||
        new.title.chars().chain(new.artist.chars()).any(char::is_control)
    {
        return Err("Renseigne un titre et un artiste, sans caractères de contrôle.".into());
    }
    if new.title == song.track.title && new.artist == song.track.artist { return Ok(()); }
    let _lock = playlist_add::lock()?;
    let mut playlists = local_playlists()?;
    playlists.push(csv.to_owned());
    let mut seen = HashSet::new();
    let mut prepared_csv = Vec::new();
    let mut originals = Vec::new();
    let mut changes: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
    for playlist in playlists {
        let path = fs::canonicalize(playlist)?;
        if seen.insert(path.clone()) {
            let original = fs::read(&path)?;
            if let Some(file) = rewrite(&path, &song.track, &new)? {
                originals.push((path.clone(), original));
                changes.push((path, Some(file.path().to_owned())));
                prepared_csv.push(file);
            }
        }
    }
    if prepared_csv.is_empty() { return Err("Le morceau a changé. Rouvre la playlist.".into()); }

    let directory = song.path.parent().ok_or("Dossier audio introuvable.")?;
    let temporary = tempfile::tempdir_in(directory)?;
    let mut deletions = Vec::new();
    for extension in ["opus", "mp3"] {
        let old = song.path.with_extension(extension);
        if !old.try_exists()? { continue; }
        let target = directory.join(new.filename()).with_extension(extension);
        let same_file = target.try_exists()? && fs::canonicalize(&target)? == fs::canonicalize(&old)?;
        if target.try_exists()? && !same_file {
            return Err(format!("Un fichier utilise déjà ce nom : {}", target.display()).into());
        }
        let output = temporary.path().join(format!("renamed.{extension}"));
        let result = tokio::process::Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-i"]).arg(&old)
            .args(["-map", "0", "-map_metadata", "0", "-c", "copy",
                   "-metadata"]).arg(format!("title={}", new.title))
            .arg("-metadata").arg(format!("artist={}", new.artist))
            .arg(&output).output().await?;
        if !result.status.success() {
            return Err(format!("Métadonnées audio : {}", String::from_utf8_lossy(&result.stderr).trim()).into());
        }
        changes.insert(0, (target, Some(output)));
        if old != directory.join(new.filename()).with_extension(extension) && !same_file {
            deletions.push((old, None));
        }
    }
    let mut edits = track_edits::load()?;
    track_edits::record(&mut edits, &song.track, &new)?;
    let edits_file = temporary.path().join("edits.json");
    fs::write(&edits_file, serde_json::to_vec_pretty(&edits)?)?;
    changes.push((track_edits::path(), Some(edits_file)));
    changes.extend(deletions);
    for (path, original) in originals {
        if fs::read(path)? != original {
            return Err("Une playlist a changé pendant la modification. Réessaie.".into());
        }
    }
    file_transaction::apply(&changes)?;

    let path = directory.join(new.filename())
        .with_extension(song.path.extension().unwrap_or_default());
    audio.send(AudioCommand::Renamed(song.track.key(), Song {
        label: format!("{} — {}", new.artist, new.title), track: new, path,
    }))?;
    Ok(())
}
