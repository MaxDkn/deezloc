#[path = "ui/info_card.rs"]
mod info_card;
#[path = "ui/playlist_info.rs"]
mod playlist_info;
#[path = "ui/rename_song.rs"]
mod rename_song;
#[path = "spotify.rs"]
mod spotify;
#[path = "ui/add_song.rs"]
mod add_song;
#[path = "ui/youtube_import.rs"]
mod youtube_import;
#[path = "ui/playlist_add.rs"]
mod playlist_add;
#[path = "ui/collection.rs"]
mod collection;
#[path = "ui/dialogs.rs"]
mod dialogs;
#[path = "ui/downloads.rs"]
mod downloads;
#[path = "ui/info.rs"]
mod info;
#[path = "ui/input.rs"]
mod input;
#[path = "ui/tracks.rs"]
mod tracks;
#[path = "ui/widgets.rs"]
mod widgets;

use super::{import_deezer, playlist_name as original_playlist_name, CACHE};

fn playlist_name(path: &Path) -> String {
    std::fs::read_to_string(path.with_extension("name")).ok()
        .filter(|name| !name.trim().is_empty()).unwrap_or_else(|| original_playlist_name(path))
}
use crate::{audio::Audio, Result};
use collection::{local_playlists, music_size};
use dialogs::{choose, text_input};
use tracks::listen;
use ratatui::DefaultTerminal;
use std::path::{Path, PathBuf};

// Une seule session de terminal pour toute la navigation.
struct TerminalSession;

impl Drop for TerminalSession {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

async fn add_playlist(terminal: &mut DefaultTerminal, audio: &Audio) -> Result<Option<PathBuf>> {
    let services = vec!["Deezer".to_owned(), "Spotify".to_owned()];
    let mut status = String::new();

    loop {
        let Some(service) = choose(
            terminal, " Ajouter une playlist ", None, &services, &status, audio, false,
        ).await? else { return Ok(None); };

        let (title, hint) = if service == 0 {
            (" URL Deezer ", "Colle le lien de la playlist publique Deezer.")
        } else {
            (" URL Spotify ", "Colle le lien public d’une playlist ou d’un album Spotify.")
        };
        let Some(url) = text_input(terminal, title, hint)? else { continue; };

        let result = if service == 0 {
            import_deezer(&url)
        } else {
            add_song::waiting(
                terminal, audio, " Import Spotify ",
                "Récupération des titres et artistes…", spotify::import(&url),
            ).await
        };
        match result {
            Ok(path) => return Ok(Some(path)),
            Err(error) => status = error.to_string(),
        }
    }
}

pub(super) async fn launch() -> Result<()> {
    let mut terminal = ratatui::init();
    let _session = TerminalSession;
    crossterm::execute!(std::io::stdout(), crossterm::terminal::SetTitle(""))?;
    let audio = Audio::new();
    let mut status = String::new();
    let mut menu_state = ratatui::widgets::ListState::default();
    let mut selected_path: Option<PathBuf> = None;

    loop {
        let paths = local_playlists()?;
        if let Some(index) = selected_path.as_ref().and_then(|selected| paths.iter().position(|path| path == selected)) {
            menu_state.select(Some(index));
        }
        let mut labels: Vec<String> = paths.iter().map(|p| playlist_name(p)).collect();
        labels.push("+ Ajouter une playlist".to_owned());

        let size = music_size().unwrap_or_else(|_| "Taille indisponible".into());
        let Some(index) = dialogs::choose_playlists(&mut terminal, " DeezLoc ", Some(&size), &labels, &status, &audio, true, &paths, &mut menu_state).await? else {
            return Ok(());
        };

        if index == usize::MAX { selected_path = None; continue; }

        selected_path = paths.get(index).cloned();
        let path = if let Some(path) = paths.get(index) {
            path.clone()
        } else {
            let Some(path) = add_playlist(&mut terminal, &audio).await? else { continue; };
            path
        };

        selected_path = Some(path.clone());
        status.clear();

        if let Err(e) = listen(&mut terminal, &path, &audio).await {
            status = e.to_string();
        }
    }
}
