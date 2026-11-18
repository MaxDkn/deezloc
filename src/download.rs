use crate::{config::{MAX_DOWNLOADS, MUSIC_DIR, OPUS_BITRATE}, read_tracks, youtube::find_audio, Result, Track};
use fs2::FileExt;
use futures::{stream, StreamExt};
use std::{fs, path::{Path, PathBuf}};
use tokio::process::Command;

pub(crate) enum Status { Downloaded, AlreadyPresent }

async fn has_codec(path: &Path, codec: &str) -> Result<bool> {
    if !path.try_exists()? { return Ok(false); }
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "a:0", "-show_entries", "stream=codec_name",
               "-of", "default=noprint_wrappers=1:nokey=1"])
        .arg(path).output().await?;
    Ok(out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == codec)
}

pub(crate) async fn download(track: &Track, dir: &Path) -> Result<Status> {
    let dest = dir.join(track.filename());
    if has_codec(&dest, "opus").await? || has_codec(&dest.with_extension("mp3"), "mp3").await? {
        return Ok(Status::AlreadyPresent);
    }

    if dest.try_exists()? {
        return Err("Le fichier Opus existe mais sa vérification a échoué. Fichier conservé.".into());
    }

    let tmp = tempfile::tempdir_in(dir)?;
    let url = find_audio(track).await?;
    let encoded = encode_source(tmp.path(), &url).await?;
    // Publier le fichier complet sans écraser une destination apparue entre-temps.
    // Le fichier temporaire est sur le même volume et sera supprimé avec le dossier.
    fs::hard_link(&encoded, &dest)?;
    Ok(Status::Downloaded)
}

pub(crate) async fn encode_source(dir: &Path, url: &str) -> Result<PathBuf> {
    let out = Command::new("yt-dlp").current_dir(dir)
        .args(["--ignore-config", "--no-progress", "--no-playlist", "-f", "bestaudio/best",
               "-o", "source.media", "--"])
        .arg(url).output().await?;
    if !out.status.success() {
        return Err(format!("yt-dlp : {}", String::from_utf8_lossy(&out.stderr).trim()).into());
    }

    let encoded = dir.join("track.opus");
    let out = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-i"]).arg(dir.join("source.media"))
        .args(["-map", "0:a:0", "-vn", "-c:a", "libopus", "-b:a", OPUS_BITRATE, "-vbr", "on"])
        .arg("-metadata").arg(format!("source_url={url}"))
        .arg(&encoded).output().await?;
    if !out.status.success() {
        return Err(format!("FFmpeg : {}", String::from_utf8_lossy(&out.stderr).trim()).into());
    }
    if !has_codec(&encoded, "opus").await? { return Err("Fichier Opus illisible.".into()); }
    Ok(encoded)
}

pub(crate) enum BatchEvent {
    Started(usize),
    Downloaded,
    Present,
    Failed(String),
}

pub(crate) async fn run_batch(
    csv: &Path,
    limit: Option<usize>,
    mut notify: impl FnMut(BatchEvent),
) -> Result<()> {
    let tracks = read_tracks(csv, limit)?;
    notify(BatchEvent::Started(tracks.len()));
    if tracks.is_empty() { return Ok(()); }

    let dir = Path::new(MUSIC_DIR);
    fs::create_dir_all(dir)?;
    let lock = fs::OpenOptions::new().create(true).truncate(false).write(true)
        .open(dir.join(".lock"))?;
    FileExt::try_lock_exclusive(&lock)
        .map_err(|e| format!("Dossier de téléchargement déjà utilisé ou indisponible : {e}"))?;

    let mut jobs = stream::iter(tracks).map(|track| async move {
        let result = download(&track, dir).await;
        (track, result)
    }).buffer_unordered(MAX_DOWNLOADS);

    while let Some((track, result)) = jobs.next().await {
        notify(match result {
            Ok(Status::Downloaded) => BatchEvent::Downloaded,
            Ok(Status::AlreadyPresent) => BatchEvent::Present,
            Err(error) => BatchEvent::Failed(format!("{} — {}\n\n{}", track.artist, track.title, error)),
        });
    }
    Ok(())
}
