use super::{collection::{correct_song, other_playlists, remove_song}, info_card::{self, Action, Kind, Row}, playlist_name, rename_song};
use crate::{audio::{Audio, AudioCommand}, track::Song, youtube::youtube_url, Result};
use ratatui::{DefaultTerminal, widgets::ListState};
use std::{fs, path::Path};

pub(super) async fn song_info(
    terminal: &mut DefaultTerminal, csv: &Path, song: &Song, audio: &Audio,
    mut background: impl FnMut(&mut ratatui::Frame<'_>),
) -> Result<bool> {
    let mut song = song.clone();
    let mut changed = false;
    let mut selected = ListState::default();
    let mut notice = String::new();
    loop {
        let others = other_playlists(csv, &song)?;
        let size = fs::metadata(&song.path)?.len() as f64 / 1_000_000.0;
        let output = tokio::process::Command::new("ffprobe")
            .args(["-v", "error", "-select_streams", "a:0", "-show_entries",
                "stream=codec_name:stream_tags=source_url:format=duration:format_tags=source_url", "-of", "json"])
            .arg(&song.path).output().await?;
        let data: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_default();
        let seconds = data["format"]["duration"].as_str().and_then(|s| s.parse::<f64>().ok());
        let duration = seconds.map(|s| format!("{}:{:02}", s as u64 / 60, s as u64 % 60))
            .unwrap_or_else(|| "Durée inconnue".into());
        let source = [&data["streams"][0]["tags"], &data["format"]["tags"]].into_iter()
            .filter_map(|tags| tags.as_object()).flat_map(|tags| tags.iter())
            .find(|(name, _)| name.eq_ignore_ascii_case("source_url"))
            .and_then(|(_, value)| value.as_str()).unwrap_or("").to_owned();
        let codec = data["streams"][0]["codec_name"].as_str().unwrap_or("?").to_uppercase();
        let fields = [
            Row::new("Titre", song.track.title.clone(), Kind::Edit),
            Row::new("Artiste", song.track.artist.clone(), Kind::Edit),
            Row::new("URL YouTube", source, Kind::Url),
            Row::new("Durée", duration, Kind::ReadOnly).separated(),
            Row::new("Format", codec, Kind::ReadOnly),
            Row::new("Taille", format!("{size:.1} Mo"), Kind::ReadOnly),
            Row::new("Playlist", playlist_name(csv), Kind::ReadOnly).separated(),
            Row::new("Autres playlists", if others.is_empty() { "aucune".into() } else { others.join(", ") }, Kind::ReadOnly),
            Row::new("Fichier", song.path.display().to_string(), Kind::ReadOnly),
        ];
        let action = info_card::show(terminal, " Infos du morceau ", &fields,
            "Retirer ce morceau ? Le fichier sera supprimé uniquement s’il n’est dans aucune autre playlist.",
            &notice, &mut selected, &mut background)?;
        match action {
            None => return Ok(changed),
            Some(Action::Activate(_)) => {},
            Some(Action::Delete) => match remove_song(csv, &song, others.is_empty()) {
                Ok(()) => {
                    if others.is_empty() { audio.send(AudioCommand::Removed(vec![song.track.key()]))?; }
                    return Ok(true);
                }, Err(e) => notice = e.to_string(),
            },
            Some(Action::Save(index, value)) => {
                if index == 2 {
                    let url = match youtube_url(&value) { Ok(url) => url, Err(e) => { notice = e.to_string(); continue; } };
                    info_card::waiting(terminal, "Téléchargement et vérification…", &mut background)?;
                    match correct_song(&song, &url).await {
                        Ok(()) => {
                            song.path.set_extension("opus");
                            audio.send(AudioCommand::Renamed(song.track.key(), song.clone()))?;
                            changed = true; notice = "Source remplacée. La prochaine lecture utilisera ce fichier.".into();
                        }
                        Err(e) => notice = e.to_string(),
                    }
                } else {
                    let mut track = song.track.clone();
                    if index == 0 { track.title = value; } else { track.artist = value; }
                    info_card::waiting(terminal, "Mise à jour du morceau et des playlists…", &mut background)?;
                    match rename_song::rename(csv, &song, track.clone(), audio).await {
                        Ok(()) => {
                            song.path = song.path.with_file_name(track.filename()).with_extension(song.path.extension().unwrap_or_default());
                            song.label = format!("{} — {}", track.artist, track.title);
                            song.track = track; changed = true; notice = "Modification enregistrée dans toutes les playlists locales.".into();
                        }
                        Err(e) => notice = e.to_string(),
                    }
                }
            }
        }
    }
}
