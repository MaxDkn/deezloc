use crate::{config::MUSIC_DIR, download::encode_source, track::Track, Result};
use fs2::FileExt;
use std::{fs::{self, File}, io::Write, path::Path};

pub(super) fn lock() -> Result<File> {
    fs::create_dir_all(MUSIC_DIR)?;
    let file = fs::OpenOptions::new().create(true).truncate(false).write(true)
        .open(Path::new(MUSIC_DIR).join(".lock"))?;
    FileExt::try_lock_exclusive(&file).map_err(|e| format!("Bibliothèque occupée : {e}"))?;
    Ok(file)
}

// Préparer le CSV complet avant de publier un nouveau fichier audio.
fn append(csv: &Path, track: &Track) -> Result<Option<tempfile::NamedTempFile>> {
    let edits = crate::track_edits::load()?;
    let original = fs::read(csv)?;
    let bytes = original.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&original);
    let parent = csv.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    for delimiter in [b',', b';', b'\t'] {
        let mut reader = csv::ReaderBuilder::new().delimiter(delimiter).from_reader(bytes);
        let Ok(header) = reader.headers().cloned() else { continue };
        let column = |name: &str| header.iter().position(|h| h.trim().eq_ignore_ascii_case(name));
        let (Some(title), Some(artist)) = (column("Track name"), column("Artist name")) else { continue };
        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        if bytes.len() != original.len() { tmp.write_all(&[0xef, 0xbb, 0xbf])?; }
        let mut writer = csv::WriterBuilder::new().delimiter(delimiter).from_writer(tmp.as_file_mut());
        writer.write_record(&header)?;
        let mut present = false;
        for record in reader.records() {
            let record = record?;
            let existing = Track { title: record[title].trim().to_owned(), artist: record[artist].trim().to_owned() };
            let existing = crate::track_edits::resolve(existing, &edits)?;
            present |= existing.key() == track.key();
            writer.write_record(&record)?;
        }
        if present { return Ok(None); }
        let mut row = vec![String::new(); header.len()];
        row[title] = track.title.clone();
        row[artist] = track.artist.clone();
        writer.write_record(row)?;
        writer.flush()?;
        drop(writer);
        return Ok(Some(tmp));
    }
    Err("Colonnes Track name et Artist name introuvables.".into())
}

pub(super) async fn save(csv: &Path, track: &Track, url: &str, existing: Option<&Path>) -> Result<String> {
    let resolved = crate::track_edits::resolve(track.clone(), &crate::track_edits::load()?)?;
    let track = &resolved;
    let original = fs::read(csv)?;
    let pending = append(csv, track)?;
    let destination = Path::new(MUSIC_DIR).join(track.filename());
    if let Some(path) = existing {
        let out = tokio::process::Command::new("ffprobe")
            .args(["-v", "error", "-select_streams", "a:0", "-show_entries",
                "stream=codec_name", "-of", "default=noprint_wrappers=1:nokey=1"])
            .arg(path).output().await?;
        let codec = String::from_utf8_lossy(&out.stdout);
        if !out.status.success() || !matches!(codec.trim(), "opus" | "mp3") {
            return Err("Le fichier existant est illisible.".into());
        }
        let target = destination.with_extension(codec.trim());
        if fs::canonicalize(&target).ok().as_deref() != Some(path) {
            // Un fichier isolé peut être réutilisé sous le nom attendu par la playlist.
            fs::hard_link(path, &target)?;
        }
    } else {
        if destination.try_exists()? || destination.with_extension("mp3").try_exists()? {
            return Err("Ce titre possède déjà un fichier. Relance l’ajout pour le réutiliser.".into());
        }
        let tmp = tempfile::tempdir_in(MUSIC_DIR)?;
        let encoded = encode_source(tmp.path(), url).await?;
        fs::hard_link(encoded, &destination)?;
    }
    match pending {
        Some(tmp) => {
            if fs::read(csv)? != original {
                return Err("Le fichier audio est prêt, mais la playlist a changé. Relance l’ajout pour l’associer.".into());
            }
            tmp.persist(csv).map_err(|e| format!("Audio prêt ; ajout à la playlist impossible : {e}. Relance l’ajout."))?;
            Ok(format!("Ajouté : {} — {}", track.artist, track.title))
        }
        None => Ok(format!("Déjà dans cette playlist : {} — {}", track.artist, track.title)),
    }
}
