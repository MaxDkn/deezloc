use super::widgets::{panel, AMBER, CASE, INK};
use crate::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{DefaultTerminal, layout::{Constraint, Layout, Rect}, style::{Modifier, Style}, text::Line,
    widgets::{Clear, List, ListItem, ListState, Paragraph, Wrap}};
use std::time::Duration;

pub(super) fn centered(area: Rect, height: u16) -> Rect {
    let width = area.width.saturating_sub(4).min(76).max(area.width.min(20));
    let height = height.min(area.height);
    Rect::new(area.x + (area.width - width) / 2, area.y + (area.height - height) / 2, width, height)
}

pub(super) enum Action { Save(usize, String), Activate(usize), Delete }

#[derive(Clone, Copy)]
pub(super) enum Kind { Edit, Url, ReadOnly, Action }
pub(super) struct Row { pub label: String, pub value: String, pub kind: Kind, pub gap: bool }
impl Row {
    pub(super) fn new(label: &str, value: impl Into<String>, kind: Kind) -> Self {
        Self { label: label.into(), value: value.into(), kind, gap: false }
    }
    pub(super) fn separated(mut self) -> Self { self.gap = true; self }
}

// Une seule liste : les champs informatifs restent sélectionnables.
pub(super) fn show(
    terminal: &mut DefaultTerminal, title: &str, fields: &[Row],
    confirmation: &str, notice: &str, selected: &mut ListState,
    mut background: impl FnMut(&mut ratatui::Frame<'_>),
) -> Result<Option<Action>> {
    let ink = Style::default().fg(INK).bg(CASE);
    let highlight = Style::default().fg(CASE).bg(AMBER).add_modifier(Modifier::BOLD);
    let mut editing: Option<String> = None;
    let mut select_all = false;
    let mut confirming = false;
    let mut confirm_yes = false;
    let count = fields.len() + 1;
    if selected.selected().is_none() { selected.select(Some(0)); }
    loop {
        let mut hit_rows = Vec::new();
        let mut confirm_rows = Vec::new();
        terminal.draw(|frame| {
            background(frame);
            let card = centered(frame.area(), 20);
            frame.render_widget(Clear, card);
            let block = panel(title).style(ink);
            let body = block.inner(card);
            frame.render_widget(block, card);
            let [rows, feedback] = Layout::vertical([
                Constraint::Min(0), Constraint::Length(if notice.is_empty() { 0 } else { 3 }),
            ]).areas(body);
            let index = selected.selected().unwrap_or(0);
            let mut positions = Vec::with_capacity(count);
            let mut position = 0;
            for row in fields {
                if row.gap && position > 0 { position += 1; }
                positions.push(position);
                position += 1;
            }
            let padding = (rows.height as usize).saturating_sub(position + 1).max(1);
            positions.push(position + padding);
            let field_items: Vec<ListItem> = fields.iter().enumerate().map(|(i, row)| {
                let text = if i == index && editing.is_some() {
                    let value = editing.as_ref().unwrap();
                    format!("{} : {}▏", row.label, value)
                } else if row.value.is_empty() { row.label.clone() }
                else { format!("{} : {}", row.label, row.value) };
                // Garder la fin de la saisie visible sur les terminaux étroits.
                let text = if i == index && editing.is_some() {
                    let mut chars: Vec<char> = text.chars().collect();
                    while !chars.is_empty() && Line::raw(chars.iter().collect::<String>()).width() > rows.width.saturating_sub(2) as usize {
                        chars.remove(0);
                    }
                    chars.into_iter().collect()
                } else { text };
                ListItem::new(text)
            }).collect();
            let mut items = Vec::new();
            for (i, item) in field_items.into_iter().enumerate() {
                while items.len() < positions[i] { items.push(ListItem::new("")); }
                items.push(item);
            }
            items.extend((0..padding).map(|_| ListItem::new("")));
            items.push(ListItem::new("Supprimer  [d]"));
            let mut visual = selected.clone();
            visual.select(Some(positions[index]));
            if positions[count - 1] < rows.height as usize { *visual.offset_mut() = 0; }
            frame.render_stateful_widget(List::new(items).style(ink).highlight_symbol("› ")
                .highlight_style(highlight), rows, &mut visual);
            *selected.offset_mut() = visual.offset();
            for logical in 0..count {
                let item = positions[logical];
                if item >= visual.offset() && item - visual.offset() < rows.height as usize {
                    hit_rows.push((logical, Rect::new(rows.x, rows.y + (item - visual.offset()) as u16, rows.width, 1)));
                }
            }
            frame.render_widget(Paragraph::new(notice).style(ink.fg(AMBER)).wrap(Wrap { trim: true }), feedback);
            if confirming {
                let card = centered(frame.area(), 7);
                frame.render_widget(Clear, card);
                let block = panel(" Confirmation ").style(ink);
                let inner = block.inner(card);
                frame.render_widget(block, card);
                let [message, choices] = Layout::vertical([Constraint::Min(0), Constraint::Length(2)]).areas(inner);
                frame.render_widget(Paragraph::new(confirmation).style(ink).wrap(Wrap { trim: true }), message);
                for i in 0..2u16 {
                    if i < choices.height { confirm_rows.push((i == 1, Rect::new(choices.x, choices.y + i, choices.width, 1))); }
                }
                let mut state = ListState::default(); state.select(Some(usize::from(confirm_yes)));
                frame.render_stateful_widget(List::new(["Annuler", "Supprimer"]).style(ink)
                    .highlight_symbol("› ").highlight_style(highlight), choices, &mut state);
            }
        })?;
        if !event::poll(Duration::from_millis(150))? { continue; }
        let input = event::read()?;
        let key = match input {
            Event::Paste(text) if editing.is_some() => {
                let value = editing.as_mut().unwrap();
                if select_all { value.clear(); select_all = false; }
                value.extend(text.chars().filter(|c| !c.is_control())); continue;
            }
            Event::Mouse(m) if confirming && m.kind == event::MouseEventKind::Down(event::MouseButton::Left) => {
                confirm_rows.iter().find(|(_, rect)| m.column >= rect.x && m.column < rect.right() && m.row == rect.y)
                    .map(|(yes, _)| { confirm_yes = *yes; crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE) })
            }
            Event::Mouse(m) if editing.is_none() && !confirming => {
                match m.kind {
                    event::MouseEventKind::ScrollUp => Some(KeyCode::Up),
                    event::MouseEventKind::ScrollDown => Some(KeyCode::Down),
                    event::MouseEventKind::Down(event::MouseButton::Left) => {
                        if let Some((index, _)) = hit_rows.iter().find(|(_, rect)| m.column >= rect.x && m.column < rect.right() && m.row == rect.y) {
                            selected.select(Some(*index)); Some(KeyCode::Enter)
                        } else { None }
                    }
                    _ => None,
                }.map(|code| crossterm::event::KeyEvent::new(code, KeyModifiers::NONE))
            }
            Event::Key(k) if k.kind == KeyEventKind::Press => Some(k),
            _ => None,
        };
        let Some(k) = key else { continue; };
        if let Some(value) = editing.as_mut() {
            match k.code {
                KeyCode::Esc => editing = None,
                KeyCode::Enter => return Ok(Some(Action::Save(selected.selected().unwrap_or(0), value.trim().to_owned()))),
                KeyCode::Backspace => { if select_all { value.clear(); } else { value.pop(); } select_all = false; }
                KeyCode::Char('a') if k.modifiers.contains(KeyModifiers::CONTROL) => select_all = true,
                KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => { value.clear(); select_all = false; }
                KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) && !c.is_control() => {
                    if select_all { value.clear(); select_all = false; } value.push(c);
                }
                _ => {}
            }
        } else if confirming {
            match k.code {
                KeyCode::Esc => confirming = false,
                KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right => confirm_yes = !confirm_yes,
                KeyCode::Enter if confirm_yes => return Ok(Some(Action::Delete)),
                KeyCode::Enter => confirming = false,
                _ => {}
            }
        } else {
            let index = selected.selected().unwrap_or(0);
            match k.code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('i') => return Ok(None),
                KeyCode::Up => selected.select(Some(index.saturating_sub(1))),
                KeyCode::Down => selected.select(Some((index + 1).min(count - 1))),
                KeyCode::Home => selected.select(Some(0)),
                KeyCode::End => selected.select(Some(count - 1)),
                KeyCode::Char('d') => { confirming = true; confirm_yes = false; }
                KeyCode::Enter if index == fields.len() => { confirming = true; confirm_yes = false; }
                KeyCode::Enter => match fields[index].kind {
                    Kind::Edit => { editing = Some(fields[index].value.clone()); select_all = false; }
                    Kind::Url => { editing = Some(String::new()); select_all = false; }
                    Kind::Action => return Ok(Some(Action::Activate(index))),
                    Kind::ReadOnly => {}
                },
                _ => {}
            }
        }
    }
}

pub(super) fn waiting(terminal: &mut DefaultTerminal, text: &str,
    mut background: impl FnMut(&mut ratatui::Frame<'_>)) -> Result<()> {
    terminal.draw(|frame| {
        background(frame);
        let card = centered(frame.area(), 5);
        frame.render_widget(Clear, card);
        frame.render_widget(Paragraph::new(text).block(panel(" Informations "))
            .style(Style::default().fg(INK).bg(CASE)).wrap(Wrap { trim: true }), card);
    })?;
    Ok(())
}
