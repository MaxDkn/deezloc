use super::{
    dialogs::{choose, text_input},
    input::MouseCapture, playlist_add, youtube_import,
    widgets::{panel, AMBER, CASE, INK},
};
use crate::{audio::{Audio, AudioCommand}, track::Track, youtube::youtube_url, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseEventKind};
use ratatui::{
    layout::{Constraint, Layout}, style::{Modifier, Style},
    widgets::{Block, List, ListItem, ListState, Paragraph, Wrap}, DefaultTerminal,
};
use std::{future::Future, path::Path, time::Duration};

// Garder le rendu et la lecture actifs pendant les opérations réseau.
pub(super) async fn waiting<T>(
    terminal: &mut DefaultTerminal, audio: &Audio, title: &str, label: &str,
    work: impl Future<Output = Result<T>>,
) -> Result<T> {
    tokio::pin!(work);
    let mut tick = 0;
    loop {
        terminal.draw(|frame| {
            let text = format!("{} {label}", ["◐", "◓", "◑", "◒"][tick % 4]);
            frame.render_widget(Paragraph::new(text).block(panel(title))
                .style(Style::default().fg(AMBER).bg(CASE)), frame.area());
        })?;
        tokio::select! {
            result = &mut work => return result,
            _ = tokio::time::sleep(Duration::from_millis(120)) => {}
        }
        tick += 1;
        if event::poll(Duration::ZERO)? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press { continue; }
                match key.code {
                    KeyCode::Char(' ') => audio.send(AudioCommand::Toggle)?,
                    KeyCode::Left => audio.send(AudioCommand::Previous)?,
                    KeyCode::Right => audio.send(AudioCommand::Next)?,
                    _ => {}
                }
            }
        }
    }
}

fn duplicate_warning(
    terminal: &mut DefaultTerminal, audio: &Audio, item: &youtube_import::Existing,
) -> Result<Option<bool>> {
    crossterm::execute!(std::io::stdout(), event::EnableMouseCapture)?;
    let _mouse = MouseCapture;
    let locations = if item.playlists.is_empty() {
        "Aucune playlist locale connue".to_owned()
    } else { item.playlists.join("\n") };
    let details = format!("Ce morceau semble déjà téléchargé.\n\n{}\n{}\n\nFichier\n{}\n\nPlaylists\n{}",
        item.label, item.reason, item.path.display(), locations);
    let mut state = ListState::default();
    state.select(Some(0));
    let mut offset = 0u16;
    loop {
        terminal.draw(|frame| {
            let style = Style::default().fg(INK).bg(CASE);
            frame.render_widget(Block::default().style(style), frame.area());
            let [body, actions] = Layout::vertical([Constraint::Min(0), Constraint::Length(5)])
                .areas(frame.area());
            frame.render_widget(Paragraph::new(details.as_str())
                .block(panel(" Doublon possible · Pg↑/Pg↓ ou molette "))
                .style(style).wrap(Wrap { trim: false }).scroll((offset, 0)), body);
            let list = List::new([
                ListItem::new("Utiliser ce fichier dans la playlist"),
                ListItem::new("C’est une autre version · continuer"),
                ListItem::new("Annuler"),
            ]).block(panel(" Ajouter un morceau ")).highlight_symbol("› ").style(style)
                .highlight_style(Style::default().fg(CASE).bg(AMBER).add_modifier(Modifier::BOLD));
            frame.render_stateful_widget(list, actions, &mut state);
        })?;
        if !event::poll(Duration::from_millis(150))? { continue; }
        match event::read()? {
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => offset = offset.saturating_sub(1),
                MouseEventKind::ScrollDown => offset = offset.saturating_add(1),
                _ => {}
            },
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Esc | KeyCode::Char('q') => return Ok(None),
                KeyCode::Up => state.select(Some(state.selected().unwrap_or(0).saturating_sub(1))),
                KeyCode::Down => state.select(Some((state.selected().unwrap_or(0) + 1).min(2))),
                KeyCode::PageUp => offset = offset.saturating_sub(5),
                KeyCode::PageDown => offset = offset.saturating_add(5),
                KeyCode::Enter => return Ok(match state.selected() {
                    Some(0) => Some(true), Some(1) => Some(false), _ => None,
                }),
                KeyCode::Char(' ') => audio.send(AudioCommand::Toggle)?,
                KeyCode::Left => audio.send(AudioCommand::Previous)?,
                KeyCode::Right => audio.send(AudioCommand::Next)?,
                _ => {}
            },
            _ => {}
        }
    }
}

async fn review(terminal: &mut DefaultTerminal, audio: &Audio, mut track: Track) -> Result<Option<Track>> {
    loop {
        let labels = vec![
            format!("Titre · {}", track.title),
            format!("Artiste · {}", track.artist),
            "Vérifier les doublons et ajouter".to_owned(),
        ];
        match choose(terminal, " Informations détectées ", None, &labels,
            "Vérifie le titre et l’artiste · Entrée pour modifier", audio, false).await? {
            Some(0) => {
                if let Some(value) = text_input(terminal, " Titre ", &track.title)? {
                    track.title = value;
                }
            }
            Some(1) => {
                if let Some(value) = text_input(terminal, " Artiste ", &track.artist)? {
                    track.artist = value;
                }
            }
            Some(2) if !track.title.trim().is_empty() && !track.artist.trim().is_empty() => {
                return Ok(Some(track));
            }
            None => return Ok(None),
            _ => {}
        }
    }
}

pub(super) async fn add_song(terminal: &mut DefaultTerminal, csv: &Path, audio: &Audio) -> Result<String> {
    let Some(input) = text_input(terminal, " Ajouter un morceau YouTube ",
        "Colle le lien de la vidéo · Entrée pour continuer · Échap pour revenir")? else {
        return Ok(String::new());
    };
    let url = youtube_url(&input)?;
    let track = waiting(terminal, audio, " Ajouter un morceau ", "Lecture des informations YouTube…",
        youtube_import::identify(&url)).await?;
    let Some(track) = review(terminal, audio, track).await? else { return Ok(String::new()); };

    let track = crate::track_edits::resolve(track, &crate::track_edits::load()?)?;

    // Le verrou couvre la vérification, la décision et la publication.
    let _lock = playlist_add::lock()?;
    let matches = waiting(terminal, audio, " Ajouter un morceau ", "Recherche des morceaux déjà téléchargés…",
        youtube_import::duplicates(csv, &track, &url)).await?;
    for item in matches {
        match duplicate_warning(terminal, audio, &item)? {
            None => return Ok(String::new()),
            Some(false) => continue,
            Some(true) => {
                let selected = item.track.as_ref().unwrap_or(&track);
                return waiting(terminal, audio, " Ajouter un morceau ", "Ajout du fichier existant…",
                    playlist_add::save(csv, selected, &url, Some(&item.path))).await;
            }
        }
    }
    waiting(terminal, audio, " Ajouter un morceau ", "Téléchargement et conversion…",
        playlist_add::save(csv, &track, &url, None)).await
}
