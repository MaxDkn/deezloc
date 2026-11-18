use crate::{Result, Track};
use tokio::process::Command;

pub(crate) async fn find_audio(track: &Track) -> Result<String> {
    let query = format!("ytsearch1:{} {} audio", track.artist, track.title);
    let out = Command::new("yt-dlp")
        .args(["--ignore-config", "--flat-playlist", "--dump-single-json", "--"])
        .arg(query).output().await?;
    if !out.status.success() {
        return Err(format!("Recherche YouTube : {}", String::from_utf8_lossy(&out.stderr).trim()).into());
    }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let id = json["entries"].as_array().and_then(|entries| entries.first())
        .and_then(|entry| entry["id"].as_str())
        .filter(|id| id.len() == 11 && id.bytes().all(|b|
            b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))
        .ok_or("Aucun résultat YouTube trouvé.")?;
    Ok(format!("https://www.youtube.com/watch?v={id}"))
}

pub(crate) fn youtube_url(input: &str) -> Result<String> {
    let url = input.trim().strip_prefix("https://").ok_or("Un lien YouTube HTTPS est attendu.")?;
    let (host, rest) = url.split_once('/').ok_or("URL incomplète.")?;
    let rest = rest.split('#').next().unwrap_or("");
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let id = match host {
        "youtu.be" => Some(path),
        "youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com" => {
            if path == "watch" { query.split('&').find_map(|v| v.strip_prefix("v=")) }
            else { path.strip_prefix("shorts/") }
        }
        _ => None,
    }.filter(|id| id.len() == 11 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))
        .ok_or("Colle le lien d’une vidéo YouTube précise.")?;
    Ok(format!("https://www.youtube.com/watch?v={id}"))
}
