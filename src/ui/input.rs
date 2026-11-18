use super::widgets::{deck_layout, player_buttons, search_layout};
use crate::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{DefaultTerminal, widgets::ListState};
use std::{io, time::Duration};

fn control_key(event: Event, bar: Option<ratatui::layout::Rect>) -> Option<KeyEvent> {
    match event {
        Event::Key(k) if k.kind == KeyEventKind::Press => Some(k),
        Event::Mouse(m) if m.kind == event::MouseEventKind::Down(event::MouseButton::Left) => {
            let hit = player_buttons(bar?).iter().position(|r| {
                m.row >= r.y && m.row < r.bottom() && m.column >= r.x && m.column < r.right()
            })?;
            let code = [KeyCode::Left, KeyCode::Char(' '), KeyCode::Right][hit];
            Some(KeyEvent::new(code, KeyModifiers::NONE))
        }
        _ => None,
    }
}

pub(super) fn read_menu_key(
    terminal: &DefaultTerminal,
    controls: bool,
    state: &mut ListState,
    len: usize,
    searching: bool,
    bottom_action: bool,
) -> Result<Option<(KeyEvent, bool)>> {
    if !event::poll(Duration::from_millis(150))? { return Ok(None); }
    let size = terminal.size()?;
    let area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
    let [list, _, bar] = deck_layout(area, controls);
    let (_, list) = search_layout(list, searching);
    let input = event::read()?;
    let mouse = matches!(&input, Event::Mouse(_));
    if let Event::Mouse(m) = &input {
        let visible = usize::from(list.height.saturating_sub(2));
        let action_row = len.saturating_sub(1).saturating_sub(state.offset())
            + visible.saturating_sub(len);
        if bottom_action && len > 0 && action_row < visible
            && usize::from(m.row.saturating_sub(list.y + 1)) == action_row
            && m.row > list.y && m.row < list.bottom().saturating_sub(1)
            && m.column > list.x && m.column < list.right().saturating_sub(1)
            && m.kind == event::MouseEventKind::Down(event::MouseButton::Left)
        {
            state.select(Some(len - 1));
            return Ok(Some((KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), true)));
        }
        if len > 0 && visible > 0 && m.row >= list.y && m.row < list.bottom()
            && m.column >= list.x && m.column < list.right()
        {
            if m.kind == event::MouseEventKind::Down(event::MouseButton::Left)
                && m.row > list.y && m.row < list.bottom().saturating_sub(1) {
                let index = state.offset() + usize::from(m.row - list.y - 1);
                let ordinary_len = if bottom_action { len - 1 } else { len };
                if index < ordinary_len {
                    state.select(Some(index));
                    return Ok(Some((KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), true)));
                }
                return Ok(None);
            }
            let last = len - 1;
            let selected = state.selected().unwrap_or(0);
            let offset = state.offset();
            let max_offset = len.saturating_sub(visible);
            let (next, top) = match m.kind {
                event::MouseEventKind::ScrollUp =>
                    (selected.saturating_sub(1), offset.saturating_sub(1)),
                event::MouseEventKind::ScrollDown =>
                    ((selected + 1).min(last), (offset + 1).min(max_offset)),
                _ => return Ok(control_key(input, controls.then_some(bar)).map(|key| (key, mouse))),
            };
            state.select(Some(next));
            *state.offset_mut() = top;
            return Ok(None);
        }
    }
    Ok(control_key(input, controls.then_some(bar)).map(|key| (key, mouse)))
}

pub(super) fn move_selection(state: &mut ListState, len: usize, key: KeyCode) {
    if len == 0 {
        return;
    }

    let current = state.selected().unwrap_or(0);
    let next = match key {
        KeyCode::Up => current.saturating_sub(1),
        KeyCode::Down => (current + 1).min(len - 1),
        KeyCode::Home => 0,
        KeyCode::End => len - 1,
        _ => current,
    };

    state.select(Some(next));
}

pub(super) struct MouseCapture;

impl Drop for MouseCapture {
    fn drop(&mut self) {
        let _ = crossterm::execute!(io::stdout(), event::DisableMouseCapture);
    }
}
