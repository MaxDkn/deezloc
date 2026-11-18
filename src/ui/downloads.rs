use super::{input::move_selection, playlist_name, widgets::{panel, AMBER, CASE, INK}};
use crate::{audio::{Audio, AudioCommand}, download::{run_batch, BatchEvent}, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{DefaultTerminal, layout::{Constraint, Direction, Layout}, style::Style, widgets::{Block, List, ListItem, ListState, Paragraph, Wrap}};
use std::{path::Path, time::Duration};

// Les téléchargements individuels utilisent la fonction du projet ; la vue affiche leur progression.
pub(super) async fn download_screen(terminal: &mut DefaultTerminal, path: &Path, limit: Option<usize>, audio: &Audio) -> Result<()> {
    use ratatui::widgets::Gauge;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Report {
        total: usize,
        downloaded: usize,
        present: usize,
        failures: Vec<String>,
    }

    let report = RefCell::new(Report::default());
    let batch = run_batch(path, limit, |update| {
        let mut r = report.borrow_mut();
        match update {
            BatchEvent::Started(total) => r.total = total,
            BatchEvent::Downloaded => r.downloaded += 1,
            BatchEvent::Present => r.present += 1,
            BatchEvent::Failed(name) => r.failures.push(name),
        }
    });
    tokio::pin!(batch);
    let mut finished = false;
    let mut error: Option<String> = None;
    let mut selection = ListState::default();
    let title = format!(" {} · Téléchargement ", playlist_name(path));

    loop {
        terminal.draw(|frame| {
            let r = report.borrow();
            let done = r.downloaded + r.present + r.failures.len();
            let areas = Layout::default().direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Length(2),
                              Constraint::Min(1), Constraint::Length(2)])
                .split(frame.area());
            frame.render_widget(Block::default().style(Style::default().bg(CASE)), frame.area());
            let ratio = if r.total == 0 { 0.0 } else { done as f64 / r.total as f64 };
            let bar = Gauge::default().block(panel(&title))
                .gauge_style(Style::default().fg(AMBER).bg(CASE))
                .ratio(ratio).label(format!("{done}/{}", r.total));
            frame.render_widget(bar, areas[0]);
            let counts = format!("Téléchargés : {} · Présents : {} · Échecs : {}",
                                 r.downloaded, r.present, r.failures.len());
            frame.render_widget(Paragraph::new(counts).wrap(Wrap { trim: true }), areas[1]);
            if !r.failures.is_empty() {
                let list = List::new(r.failures.iter().map(|s| ListItem::new(s.lines().next().unwrap_or(""))))
                    .block(panel(" Morceaux non téléchargés "))
                    .highlight_style(Style::default().fg(AMBER));
                frame.render_stateful_widget(list, areas[2], &mut selection);
            }
            if let Some(message) = error.as_deref() {
                frame.render_widget(Paragraph::new(message).wrap(Wrap { trim: true }), areas[2]);
            }
            let footer = if finished { "i · Détail de l’erreur    Entrée / Échap · Retour" }
                         else { "Téléchargement en cours…" };
            frame.render_widget(Paragraph::new(footer).style(Style::default().fg(INK)), areas[3]);
        })?;

        if finished {
            if !event::poll(Duration::from_millis(150))? { continue; }
        } else {
            tokio::select! {
                result = &mut batch => {
                    finished = true;
                    if let Err(e) = result { error = Some(e.to_string()); }
                    report.borrow_mut().failures.sort();
                    selection.select((!report.borrow().failures.is_empty()).then_some(0));
                    // Afficher le bilan avant d'accepter une touche de retour.
                    continue;
                }
                _ = tokio::time::sleep(Duration::from_millis(100)) => {}
            }
            if !event::poll(Duration::ZERO)? { continue; }
        }

        if let Event::Key(k) = event::read()? {
            if k.kind != KeyEventKind::Press { continue; }
            match k.code {
                KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q') if finished => return Ok(()),
                KeyCode::Char('i') if finished => {
                    if let Some(message) = selection.selected()
                        .and_then(|index| report.borrow().failures.get(index).cloned())
                    {
                        failure_details(terminal, &message, audio)?;
                    }
                }
                KeyCode::Char(' ') => audio.send(AudioCommand::Toggle)?,
                KeyCode::Left => audio.send(AudioCommand::Previous)?,
                KeyCode::Right => audio.send(AudioCommand::Next)?,
                code => move_selection(&mut selection, report.borrow().failures.len(), code),
            }
        }
    }
}

fn failure_details(terminal: &mut DefaultTerminal, message: &str, audio: &Audio) -> Result<()> {
    let mut offset = 0u16;
    loop {
        terminal.draw(|frame| {
            let [body, footer] = Layout::default().direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
            let style = Style::default().fg(INK).bg(CASE);
            frame.render_widget(Block::default().style(style), frame.area());
            frame.render_widget(Paragraph::new(message)
                .block(panel(" Détail de l’échec ")).style(style)
                .wrap(Wrap { trim: false }).scroll((offset, 0)), body);
            frame.render_widget(Paragraph::new("↑ / ↓ · Défiler    Échap · Retour")
                .style(style.fg(AMBER)), footer);
        })?;
        if !event::poll(Duration::from_millis(150))? { continue; }
        let Event::Key(key) = event::read()? else { continue };
        if key.kind != KeyEventKind::Press { continue; }
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('i') => return Ok(()),
            KeyCode::Up => offset = offset.saturating_sub(1),
            KeyCode::Down => offset = offset.saturating_add(1),
            KeyCode::PageUp => offset = offset.saturating_sub(10),
            KeyCode::PageDown => offset = offset.saturating_add(10),
            KeyCode::Home => offset = 0,
            KeyCode::Char(' ') => audio.send(AudioCommand::Toggle)?,
            KeyCode::Left => audio.send(AudioCommand::Previous)?,
            KeyCode::Right => audio.send(AudioCommand::Next)?,
            _ => {}
        }
    }
}
