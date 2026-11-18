use super::{info_card::{self, Action, Kind, Row}, playlist_name};
use crate::{audio::{Audio, AudioCommand}, config::MUSIC_DIR, track::read_tracks, Result};
use ratatui::{DefaultTerminal, widgets::ListState};
use std::{fs, io::Write, path::Path};

pub(super) async fn show(terminal: &mut DefaultTerminal, csv: &Path, audio: &Audio,
    mut background: impl FnMut(&mut ratatui::Frame<'_>)) -> Result<bool> {
    let mut state = ListState::default();
    let mut notice = String::new();
    let mut changed = false;
    loop {
        let tracks = read_tracks(csv, None)?;
        let mut present = 0;
        let mut bytes = 0;
        for track in &tracks {
            let mut found = false;
            for ext in ["opus", "mp3"] {
                if let Ok(meta) = fs::metadata(Path::new(MUSIC_DIR).join(track.filename()).with_extension(ext)) {
                    bytes += meta.len(); found = true;
                }
            }
            present += usize::from(found);
        }
        let source = if csv.file_name().unwrap_or_default().to_string_lossy().starts_with("spotify-") { "Spotify" }
            else if csv.file_stem().unwrap_or_default().to_string_lossy().parse::<u64>().is_ok() { "Deezer" } else { "CSV local" };
        let fields = [
            Row::new("Nom", playlist_name(csv), Kind::Edit),
            Row::new("Source", source, Kind::ReadOnly),
            Row::new("Morceaux", tracks.len().to_string(), Kind::ReadOnly).separated(),
            Row::new("Disponibles", present.to_string(), Kind::ReadOnly),
            Row::new("Taille audio", format!("{:.1} Mo", bytes as f64 / 1_000_000.0), Kind::ReadOnly),
            Row::new("Fichier", csv.display().to_string(), Kind::ReadOnly).separated(),
            Row::new("Télécharger les morceaux manquants", "", Kind::Action).separated(),
            Row::new("Ajouter un morceau YouTube", "", Kind::Action),
        ];
        match info_card::show(terminal, " Infos de la playlist ", &fields,
            "Supprimer cette playlist locale ? Seuls les fichiers audio absents des autres playlists seront supprimés.",
            &notice, &mut state, &mut background)? {
            Some(Action::Save(0, name)) => {
                if name.is_empty() || name.chars().any(char::is_control) {
                    notice = "Entre un nom non vide, sans retour à la ligne.".into(); continue;
                }
                let save = (|| -> Result<()> {
                    let _lock = super::playlist_add::lock()?;
                    let parent = csv.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
                    let mut file = tempfile::NamedTempFile::new_in(parent)?;
                    file.write_all(name.as_bytes())?;
                    file.as_file().sync_all()?;
                    file.persist(csv.with_extension("name"))?;
                    Ok(())
                })();
                match save { Ok(()) => { changed = true; notice.clear(); }, Err(e) => notice = e.to_string() }
            }
            Some(Action::Activate(6)) => {
                match super::downloads::download_screen(terminal, csv, None, audio).await {
                    Ok(()) => { changed = true; notice.clear(); }, Err(e) => notice = e.to_string(),
                }
            }
            Some(Action::Activate(7)) => {
                // Rompre le cycle de futures via le menu de vérification du morceau.
                match Box::pin(super::add_song::add_song(terminal, csv, audio)).await {
                    Ok(message) => { changed |= !message.is_empty(); notice = message; },
                    Err(e) => notice = e.to_string(),
                }
                crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture)?;
            }
            Some(Action::Delete) => match super::collection::remove_playlist(csv) {
                Ok(keys) => { audio.send(AudioCommand::Removed(keys))?; return Ok(true); }
                Err(e) => notice = e.to_string(),
            },
            None => return Ok(changed),
            _ => {}
        }
    }
}
