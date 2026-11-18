use super::{input::{move_selection, read_menu_key, MouseCapture}, widgets::{draw_list, render_list, panel, Marquee}};
use crate::{audio::{Audio, AudioCommand}, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{DefaultTerminal, layout::{Constraint, Direction, Layout}, widgets::{ListState, Paragraph, Wrap}};
use std::{io, time::Duration};

pub(super) async fn choose(terminal: &mut DefaultTerminal, title: &str, storage: Option<&str>, labels: &[String], status: &str, audio: &Audio, main_menu: bool) -> Result<Option<usize>> {
    let mut state = ListState::default();
    choose_menu(terminal, title, storage, labels, status, audio, main_menu, false, &[], &mut state).await
}

pub(super) async fn choose_playlists(terminal: &mut DefaultTerminal, title: &str, storage: Option<&str>, labels: &[String], status: &str, audio: &Audio, main_menu: bool, paths: &[std::path::PathBuf], state: &mut ListState) -> Result<Option<usize>> {
    choose_menu(terminal, title, storage, labels, status, audio, main_menu, true, paths, state).await
}

async fn choose_menu(terminal: &mut DefaultTerminal, title: &str, storage: Option<&str>, labels: &[String], status: &str, audio: &Audio, main_menu: bool, bottom_action: bool, paths: &[std::path::PathBuf], state: &mut ListState) -> Result<Option<usize>> {
    let mut labels = labels.to_vec();
    let mut title = title.to_owned();
    crossterm::execute!(io::stdout(), event::EnableMouseCapture)?;
    let _mouse = MouseCapture;
    let mut quit_dialog = false;
    let mut scroll = [Marquee::default(), Marquee::default()];
    if !labels.is_empty() {
        state.select(Some(state.selected().unwrap_or(0).min(labels.len() - 1)));
    }

    loop {
        let playback = audio.snapshot();
        
        let footer = [status, playback.status.as_str()].into_iter()
            .filter(|s| !s.is_empty()).collect::<Vec<_>>().join("  ·  ");
        draw_list(
            terminal,
            &title,
            storage,
            &labels,
            state,
            &footer,
            playback.current.as_ref().map(|_| (
                if playback.paused { "▶ Reprendre" } else { "⏸ Pause" },
                playback.shuffled,
            )),
            false,
            None,
            quit_dialog,
            &mut scroll,
            bottom_action,
        )?;

        if quit_dialog {
            if event::poll(Duration::from_millis(150))? {
                if let Event::Key(k) = event::read()? {
                    if k.kind == KeyEventKind::Press {
                        match k.code {
                            KeyCode::Enter => return Ok(None),
                            KeyCode::Esc => quit_dialog = false,
                            _ => {}
                        }
                    }
                }
            }
            continue;
        }

        let Some((k, _)) = read_menu_key(terminal, playback.current.is_some(), state, labels.len(), false, bottom_action)? else {
            continue;
        };

        match k.code {
            KeyCode::Esc | KeyCode::Char('q') if main_menu => quit_dialog = true,
            KeyCode::Esc | KeyCode::Char('q') => return Ok(None),
            KeyCode::Char('i') if !paths.is_empty() => {
                let path = if main_menu { state.selected().and_then(|i| paths.get(i)) } else { paths.first() };
                if let Some(path) = path {
                    let changed = super::playlist_info::show(terminal, path, audio, |frame| {
                        let playback = audio.snapshot();
                        render_list(frame, &title, storage, &labels, state, &footer,
                            playback.current.as_ref().map(|_| (if playback.paused { "▶ Reprendre" } else { "⏸ Pause" }, playback.shuffled)),
                            false, None, false, &mut scroll, bottom_action);
                    }).await?;
                    crossterm::execute!(io::stdout(), event::EnableMouseCapture)?;
                    if !path.exists() { return Ok(Some(usize::MAX)); }
                    if changed {
                        if main_menu {
                            for (index, path) in paths.iter().enumerate() { labels[index] = super::playlist_name(path); }
                        } else { title = super::playlist_name(path); }
                    }
                }
            }
            KeyCode::Enter => return Ok(state.selected()),
            KeyCode::Char(' ') => audio.send(AudioCommand::Toggle)?,
            KeyCode::Left => audio.send(AudioCommand::Previous)?,
            KeyCode::Right | KeyCode::Char('n') => audio.send(AudioCommand::Next)?,
            code => move_selection(state, labels.len(), code),
        }
    }
}

pub(super) fn text_input(terminal: &mut DefaultTerminal, title: &str, hint: &str) -> Result<Option<String>> {
    let mut value = String::new();

    loop {
        terminal.draw(|frame| {
            let areas = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(4), Constraint::Min(2)])
                .split(frame.area());

            frame.render_widget(
                Paragraph::new(value.as_str())
                    .block(panel(title))
                    .wrap(Wrap { trim: false }),
                areas[0],
            );
            frame.render_widget(
                Paragraph::new(hint).wrap(Wrap { trim: true }),
                areas[1],
            );
        })?;

        if !event::poll(Duration::from_millis(200))? {
            continue;
        }

        match event::read()? {
            Event::Paste(text) => value.push_str(text.trim()),
            Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                KeyCode::Esc => return Ok(None),
                KeyCode::Enter => return Ok(Some(value.trim().to_owned())),
                KeyCode::Backspace => {
                    value.pop();
                }
                KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => {
                    value.push(c);
                }
                _ => {}
            },
            _ => {}
        }
    }
}
