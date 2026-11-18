use super::{info::song_info, input::{move_selection, read_menu_key, MouseCapture}, playlist_name, widgets::{render_list, Marquee}};
use crate::{audio::{Audio, AudioCommand}, config::MUSIC_DIR, track::{read_tracks, Song}, Result};
use crossterm::event::{self, KeyCode, KeyModifiers};
use ratatui::{DefaultTerminal, widgets::ListState};
use std::{io, path::Path, sync::Arc};

pub(super) async fn listen(terminal: &mut DefaultTerminal, csv: &Path, audio: &Audio) -> Result<()> {
    let mut keep_index = 0;
    'reload: loop {
        let songs: Vec<Song> = read_tracks(csv, None)?
            .into_iter()
            .rev()
            .filter_map(|track| {
                let opus = Path::new(MUSIC_DIR).join(track.filename());
                let path = if opus.is_file() { opus } else { opus.with_extension("mp3") };

                path.is_file().then(|| Song {
                    label: format!("{} — {}", track.artist, track.title),
                    track,
                    path,
                })
            })
            .collect();

        let names: Vec<String> = songs.iter().map(|s| s.label.to_lowercase()).collect();
        let title = format!(" {} · {} morceaux disponibles ", playlist_name(csv), songs.len());
        let songs = Arc::new(songs);

        crossterm::execute!(io::stdout(), event::EnableMouseCapture)?;
        let _mouse = MouseCapture;
        let mut state = ListState::default();
        let mut scroll = [Marquee::default(), Marquee::default()];
        state.select((!songs.is_empty()).then_some(keep_index.min(songs.len().saturating_sub(1))));
        let mut previous = None;
        let mut query: Option<String> = None;
        let mut original_selection = 0;
        let mut matches: Vec<usize> = (0..songs.len()).collect();
        let mut labels: Vec<String> = songs.iter().map(|s| s.label.clone()).collect();

        loop {
            let playback = audio.snapshot();
            if previous != playback.current {
                if let Some(path) = playback.current.as_ref().filter(|_| query.is_none()) {
                    if let Some(i) = songs.iter().position(|s| &s.path == path) {
                        state.select(Some(i));
                    }
                }
                previous = playback.current.clone();
            }

            let draw_playlist = |labels: &[String], query: Option<&str>, frame: &mut ratatui::Frame<'_>, state: &mut ListState,
                                 scroll: &mut [Marquee; 2]| {
                let playback = audio.snapshot();
                let button = if playback.current.is_none() { "▶ Lire" }
                    else if playback.paused { "▶ Reprendre" } else { "⏸ Pause" };
                let footer = if songs.is_empty() { "Aucun morceau disponible." } else { playback.status.as_str() };
                render_list(frame, &title, None, labels, state, footer,
                    Some((button, playback.shuffled)), true, query, false, scroll, false);
            };
            terminal.draw(|frame| draw_playlist(&labels, query.as_deref(), frame, &mut state, &mut scroll))?;

            let Some((k, mouse)) = read_menu_key(
                terminal, true, &mut state, labels.len(), query.is_some(), false,
            )? else { continue };
            let selected = state.selected().and_then(|i| matches.get(i)).copied();

            if !mouse {
                if let Some(text) = query.as_mut() {
                    match k.code {
                        KeyCode::Esc | KeyCode::Enter => {
                            if k.code == KeyCode::Enter {
                                let Some(i) = selected else { continue };
                                audio.send(AudioCommand::Start(Arc::clone(&songs), i, false))?;
                            }
                            let restore = if k.code == KeyCode::Enter { selected.unwrap() }
                                          else { original_selection };
                            query = None;
                            matches = (0..songs.len()).collect();
                            labels = songs.iter().map(|s| s.label.clone()).collect();
                            state.select((!songs.is_empty()).then_some(restore.min(songs.len().saturating_sub(1))));
                            continue;
                        }
                        KeyCode::Backspace => { text.pop(); }
                        KeyCode::Char(c) if !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                            text.push(c);
                        }
                        code => {
                            move_selection(&mut state, matches.len(), code);
                            continue;
                        }
                    }
                    let folded = text.to_lowercase();
                    let terms: Vec<_> = folded.split_whitespace().collect();
                    matches = names.iter().enumerate()
                        .filter(|(_, name)| terms.iter().all(|word| name.contains(*word)))
                        .map(|(i, _)| i).collect();
                    labels = matches.iter().map(|&i| songs[i].label.clone()).collect();
                    state.select((!matches.is_empty()).then_some(0));
                    *state.offset_mut() = 0;
                    continue;
                }
            }

            match k.code {
                KeyCode::Char('i') => {
                    if let Some(i) = selected {
                        let changed = song_info(terminal, csv, &songs[i], audio, |frame| {
                            draw_playlist(&labels, query.as_deref(), frame, &mut state, &mut scroll);
                        }).await?;
                        previous = audio.snapshot().current;
                        if changed {
                            keep_index = i;
                            continue 'reload;
                        }
                    } else {
                        let changed = super::playlist_info::show(terminal, csv, audio, |frame| {
                            draw_playlist(&labels, query.as_deref(), frame, &mut state, &mut scroll);
                        }).await?;
                        if !csv.exists() { return Ok(()); }
                        if changed { continue 'reload; }
                    }
                }
                KeyCode::Char('f') => {
                    original_selection = state.selected().unwrap_or(0);
                    query = Some(String::new());
                    labels = songs.iter().map(|s| s.label.clone()).collect();
                    state.select((!songs.is_empty()).then_some(selected.unwrap_or(0)));
                }
                KeyCode::Esc | KeyCode::Char('q') => return Ok(()),
                KeyCode::Enter | KeyCode::Char('a') => {
                    if let Some(i) = selected {
                        audio.send(AudioCommand::Start(Arc::clone(&songs), i, k.code == KeyCode::Char('a')))?;
                    }
                }
                KeyCode::Char(' ') if playback.current.is_none() => {
                    if let Some(i) = selected {
                        audio.send(AudioCommand::Start(Arc::clone(&songs), i, false))?;
                    }
                }
                KeyCode::Char(' ') => audio.send(AudioCommand::Toggle)?,
                KeyCode::Left => audio.send(AudioCommand::Previous)?,
                KeyCode::Right | KeyCode::Char('n') => audio.send(AudioCommand::Next)?,
                code => move_selection(&mut state, labels.len(), code),
            }
        }
    }
}
