use crate::Result;
use ratatui::{DefaultTerminal, style::{Color, Modifier, Style}, text::Span, widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap}};
use std::time::Instant;

pub(super) const CASE: Color = Color::Rgb(32, 33, 29);
pub(super) const INK: Color = Color::Rgb(234, 222, 190);
const EDGE: Color = Color::Rgb(116, 111, 88);
pub(super) const AMBER: Color = Color::Rgb(244, 181, 85);

#[derive(Default)]
pub(super) struct Marquee {
    key: Option<(String, usize, Instant)>,
}

impl Marquee {
    fn line(&mut self, text: &str, width: usize) -> String {
        let span = Span::raw(text);
        if width == 0 || span.width() <= width {
            self.key = None;
            return if width == 0 { String::new() } else { text.to_owned() };
        }
        if !self.key.as_ref().is_some_and(|(old, w, _)| old == text && *w == width) {
            self.key = Some((text.to_owned(), width, Instant::now()));
        }
        let elapsed = self.key.as_ref().unwrap().2.elapsed().as_millis();
        let cells: Vec<_> = span.styled_graphemes(Style::default())
            .map(|g| (g.symbol, Span::raw(g.symbol).width())).collect();
        let mut end_start = cells.len();
        let mut suffix = 0;
        while end_start > 0 && suffix + cells[end_start - 1].1 <= width {
            end_start -= 1;
            suffix += cells[end_start].1;
        }
        let step = 210;
        let travel = end_start as u128 * step;
        let phase = elapsed % (1800 + travel + 1500);
        let start = (phase.saturating_sub(1800) / step).min(end_start as u128) as usize;
        let mut line = String::new();
        let mut used = 0;
        for &(symbol, w) in &cells[start..] {
            if used + w > width { break; }
            line.push_str(symbol);
            used += w;
        }
        line
    }
}

pub(super) fn panel(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(EDGE))
}

pub(super) fn search_layout(area: ratatui::layout::Rect, active: bool)
    -> (ratatui::layout::Rect, ratatui::layout::Rect)
{
    use ratatui::layout::Rect;
    let height = if active && area.height > 0 { 1 } else { 0 };
    (Rect::new(area.x, area.y, area.width, height),
     Rect::new(area.x, area.y + height, area.width, area.height - height))
}

pub(super) fn draw_list(
    terminal: &mut DefaultTerminal,
    title: &str,
    storage: Option<&str>,
    labels: &[String],
    state: &mut ListState,
    status: &str,
    buttons: Option<(&str, bool)>,
    allow_shuffle: bool,
    query: Option<&str>,
    quit_dialog: bool,
    scroll: &mut [Marquee; 2],
    bottom_action: bool,
) -> Result<()> {
    terminal.draw(|frame| render_list(frame, title, storage, labels, state, status, buttons,
        allow_shuffle, query, quit_dialog, scroll, bottom_action))?;
    Ok(())
}

pub(super) fn render_list(
    frame: &mut ratatui::Frame<'_>,
    title: &str,
    storage: Option<&str>,
    labels: &[String],
    state: &mut ListState,
    status: &str,
    buttons: Option<(&str, bool)>,
    allow_shuffle: bool,
    query: Option<&str>,
    quit_dialog: bool,
    scroll: &mut [Marquee; 2],
    bottom_action: bool,
) {
    frame.render_widget(Block::default().style(Style::default().bg(CASE)), frame.area());
    let [library, caption_row, bar] = deck_layout(frame.area(), buttons.is_some());
    let (search_bar, library) = search_layout(library, query.is_some());
    if let Some(query) = query {
        let prompt = format!(" Recherche : {query}▏");
        let offset = Span::raw(prompt.as_str()).width()
            .saturating_sub(search_bar.width as usize).min(u16::MAX as usize) as u16;
        frame.render_widget(Paragraph::new(prompt).scroll((0, offset))
            .style(Style::default().fg(AMBER).bg(CASE)), search_bar);
    }
    let badge_width = if caption_row.width >= 48 && (allow_shuffle || buttons.is_some_and(|b| b.1)) {
        14
    } else { 0 };
    let mut caption_area = caption_row;
    caption_area.width = caption_area.width.saturating_sub(badge_width);
    let caption = scroll[0].line(status, caption_area.width as usize);
    frame.render_widget(Paragraph::new(caption).style(Style::default().fg(INK).bg(CASE)), caption_area);
    if badge_width > 0 {
        let badge = ratatui::layout::Rect::new(caption_area.right(), caption_row.y, badge_width, caption_row.height);
        let active = buttons.is_some_and(|b| b.1);
        let label = if active { "● Aléatoire" } else { "[a] Aléatoire" };
        frame.render_widget(Paragraph::new(label).alignment(ratatui::layout::Alignment::Right)
            .style(Style::default().fg(if active { AMBER } else { EDGE })), badge);
    }

    let selected = state.selected();
    let action_index = labels.len().checked_sub(1).filter(|_| bottom_action);
    let mut list_block = panel(title);
    if let Some(size) = storage {
        if usize::from(library.width) >= Span::raw(title).width() + Span::raw(size).width() + 5 {
            let header = ratatui::text::Line::from(Span::styled(
                format!(" {size} "), Style::default().fg(INK)
            )).right_aligned();
            list_block = list_block.title(header);
        }
    }
    let content = list_block.inner(library);
    let label_width = content.width.saturating_sub(2) as usize;
    let mut items: Vec<ListItem> = labels.iter().enumerate().map(|(index, label)| {
        if selected == Some(index) {
            ListItem::new(scroll[1].line(label, label_width))
        } else {
            ListItem::new(label.as_str())
        }
    }).collect();
    // Les espaces sont purement visuels : la sélection garde les indices des actions.
    let padding = if bottom_action { (content.height as usize).saturating_sub(labels.len()) } else { 0 };
    if let Some(index) = action_index {
        items.splice(index..index, (0..padding).map(|_| ListItem::new("")));
    }
    let list = List::new(items)
        .block(list_block)
        .style(Style::default().fg(INK).bg(CASE))
        .highlight_symbol("› ")
        .highlight_style(Style::default().fg(CASE).bg(AMBER).add_modifier(Modifier::BOLD));
    let mut list_state = state.clone();
    if padding > 0 {
        list_state.select(selected.map(|index| {
            if Some(index) == action_index { index + padding } else { index }
        }));
        *list_state.offset_mut() = 0;
    }
    frame.render_stateful_widget(list, library, &mut list_state);
    *state.offset_mut() = list_state.offset();
    if query.is_some() && labels.is_empty() {
        frame.render_widget(Paragraph::new("Aucun morceau trouvé.").style(Style::default().fg(INK)), content);
    }
    // Le bord droit sert de rail ; l'offset reflète la première ligne affichée.
    let visible = usize::from(content.height);
    if content.width > 0 && visible > 0 && labels.len() > visible {
        let max_offset = labels.len() - visible;
        let thumb = ((visible as u64).pow(2) / labels.len() as u64).max(1) as usize;
        let travel = visible - thumb;
        let offset = state.offset().min(max_offset);
        let top = (offset as u128 * travel as u128 + max_offset as u128 / 2)
            / max_offset as u128;
        let cursor = ratatui::layout::Rect::new(
            library.right() - 1, content.y + top as u16, 1, thumb as u16,
        );
        frame.render_widget(Block::default().style(Style::default().bg(AMBER)), cursor);
    }
    if let Some((center, _)) = buttons {
        for (i, rect) in player_buttons(bar).into_iter().enumerate() {
            let glyph = if i == 0 { "│◀◀" } else if i == 2 { "▶▶│" }
                        else if center == "⏸ Pause" { "▌▌" } else { "▶" };
            let framed = rect.height >= 3 && rect.width >= 5;
            let label = match i {
                0 => "Préc.",
                2 => "Suiv.",
                _ => center.split_whitespace().nth(1).unwrap_or("Lire"),
            };
            let content = if framed && rect.width >= 13 {
                format!("{glyph}  {label}")
            } else { glyph.to_owned() };
            let accent = if i == 1 { AMBER } else { INK };
            let mut key = Paragraph::new(content)
                .alignment(ratatui::layout::Alignment::Center)
                .style(Style::default().fg(accent).bg(CASE).add_modifier(Modifier::BOLD));
            if framed {
                key = key.block(Block::default().borders(Borders::ALL)
                    .border_type(ratatui::widgets::BorderType::Rounded)
                    .border_style(Style::default().fg(if i == 1 { AMBER } else { EDGE })));
            }
            frame.render_widget(key, rect);
        }
    }
    if quit_dialog {
        use ratatui::{layout::{Alignment, Rect}, widgets::Clear};
        let area = frame.area();
        let width = area.width.min(42);
        let height = area.height.min(6);
        let dialog = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width, height,
        );
        frame.render_widget(Clear, dialog);
        let message = Paragraph::new("Quitter Deezloc ?\n\nEntrée : quitter   Échap : rester")
            .block(panel(" Confirmation "))
            .style(Style::default().fg(INK).bg(CASE))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });
        frame.render_widget(message, dialog);
    }

}

// Le rendu et les clics utilisent la même géométrie, même après redimensionnement.
pub(super) fn deck_layout(area: ratatui::layout::Rect, controls: bool) -> [ratatui::layout::Rect; 3] {
    use ratatui::layout::Rect;
    let keys = if !controls { 0 } else if area.height >= 7 { 3 } else { 1.min(area.height) };
    let caption = 1.min(area.height - keys);
    let library = Rect::new(area.x, area.y, area.width, area.height - keys - caption);
    let line = Rect::new(area.x, library.bottom(), area.width, caption);
    [library, line, Rect::new(area.x, line.bottom(), area.width, keys)]
}

pub(super) fn player_buttons(area: ratatui::layout::Rect) -> [ratatui::layout::Rect; 3] {
    use ratatui::layout::Rect;
    let gap = if area.width >= 20 { 1 } else { 0 };
    let usable = area.width - 2 * gap;
    let side = usable / 3;
    let center = usable - 2 * side;
    let left = Rect::new(area.x, area.y, side, area.height);
    let middle = Rect::new(left.right() + gap, area.y, center, area.height);
    let right = Rect::new(middle.right() + gap, area.y, side, area.height);
    [left, middle, right]
}
