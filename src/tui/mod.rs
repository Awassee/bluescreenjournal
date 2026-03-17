pub mod app;
pub mod buffer;

use crate::tui::app::{App, Overlay, ReplacePrompt, ReplaceStage, SetupStep, SetupWizard};
use crate::tui::buffer::MatchPos;
use crossterm::{
    cursor,
    event::{self},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use std::{io, time::Duration};

const ROYAL_BLUE: Color = Color::Rgb(65, 105, 225);

pub fn run() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, cursor::Hide)?;
    let _ = execute!(stdout, cursor::SetCursorStyle::BlinkingBlock);

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let result = run_loop(&mut terminal);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), cursor::Show, LeaveAlternateScreen)?;
    let _ = execute!(
        terminal.backend_mut(),
        cursor::SetCursorStyle::DefaultUserShape
    );
    terminal.show_cursor()?;

    result
}

fn run_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    let mut app = App::new();
    let poll_timeout = Duration::from_millis(80);

    while !app.should_quit() {
        terminal.draw(|frame| draw(frame, &app))?;
        let area = terminal.size()?;
        let viewport_height = area.height.saturating_sub(2) as usize;

        if event::poll(poll_timeout)? {
            let ev = event::read()?;
            app.handle_event(ev, viewport_height);
        } else {
            app.tick();
        }
    }

    Ok(())
}

fn draw(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    if area.width == 0 || area.height == 0 {
        return;
    }

    frame.render_widget(Block::new().style(screen_style()), area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    let header_area = chunks[0];
    let body_area = chunks[1];
    let footer_area = chunks[2];

    draw_header(frame, app, header_area);
    let editor_cursor = draw_editor(frame, app, body_area);
    draw_footer(frame, app, footer_area);

    let overlay_cursor = draw_overlay(frame, app, body_area);
    if let Some((x, y)) = overlay_cursor.or(editor_cursor) {
        frame.set_cursor_position((x, y));
    }
}

fn draw_header(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let left = format!(
        "PERSONAL JOURNAL  {}  ENTRY NO. {}",
        app.now_time_label(),
        app.entry_number_label()
    );
    let mut right_parts = vec![
        app.lock_status_label().to_string(),
        app.save_status_label(),
        app.draft_recovered_label().to_string(),
    ];
    right_parts.retain(|part| !part.is_empty());
    let right = right_parts.join(" | ");
    let content = join_left_right(area.width as usize, &left, &right);
    frame.render_widget(
        Paragraph::new(Line::from(content)).style(
            screen_style()
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::UNDERLINED),
        ),
        area,
    );
}

fn draw_editor(frame: &mut Frame<'_>, app: &App, area: Rect) -> Option<(u16, u16)> {
    if area.width == 0 || area.height == 0 {
        return None;
    }

    let start = app.scroll_row();
    let end = (start + area.height as usize).min(app.buffer().line_count());
    let mut lines = Vec::new();
    for row in start..end {
        lines.push(line_with_matches(
            app.buffer().line(row).unwrap_or_default(),
            row,
            app.find_matches(),
            app.current_match(),
        ));
    }
    if lines.is_empty() {
        lines.push(Line::from(String::new()));
    }
    frame.render_widget(Paragraph::new(lines).style(screen_style()), area);

    if app.overlay().is_some() {
        return None;
    }
    let (row, col) = app.buffer().cursor();
    if row < start || row >= end {
        return None;
    }
    let x = (area.x + col as u16).min(area.right().saturating_sub(1));
    let y = (area.y + (row - start) as u16).min(area.bottom().saturating_sub(1));
    Some((x, y))
}

fn draw_footer(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let status = app.status_text().unwrap_or("");
    let sep = if status.is_empty() { "" } else { " | " };
    let strip =
        format!("{status}{sep}F1 Help  F2 Save  F3 Dates  F4 Find  F6 Replace  F7 Index  F10 Quit");
    frame.render_widget(
        Paragraph::new(Line::from(strip)).style(
            screen_style()
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::REVERSED),
        ),
        area,
    );
}

fn draw_overlay(frame: &mut Frame<'_>, app: &App, body_area: Rect) -> Option<(u16, u16)> {
    let overlay = app.overlay()?;
    let rect = match overlay {
        Overlay::SetupWizard(_) => popup_rect(body_area, 76, 9),
        Overlay::UnlockPrompt { .. } => popup_rect(body_area, 64, 6),
        Overlay::Help => popup_rect(body_area, 72, 15),
        Overlay::DatePrompt { .. } => popup_rect(body_area, 50, 6),
        Overlay::FindPrompt { .. } => popup_rect(body_area, 54, 6),
        Overlay::ReplacePrompt(_) => popup_rect(body_area, 58, 8),
        Overlay::ReplaceConfirm(_) => popup_rect(body_area, 62, 8),
        Overlay::Index => popup_rect(body_area, 70, 10),
        Overlay::RecoverDraft { .. } => popup_rect(body_area, 44, 5),
        Overlay::QuitConfirm => popup_rect(body_area, 44, 5),
    };

    frame.render_widget(Clear, rect);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .style(screen_style())
            .title(overlay_title(overlay)),
        rect,
    );
    let inner = rect.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 2,
    });

    match overlay {
        Overlay::SetupWizard(wizard) => draw_setup_overlay(frame, inner, wizard),
        Overlay::UnlockPrompt { input, error } => {
            let masked = "*".repeat(input.chars().count());
            let lines = vec![
                Line::from("Enter vault passphrase to unlock:"),
                Line::from(format!("> {masked}")),
                Line::from(error.clone().unwrap_or_default()),
            ];
            frame.render_widget(Paragraph::new(lines).style(screen_style()), inner);
            Some((
                (inner.x + 2 + masked.chars().count() as u16).min(inner.right().saturating_sub(1)),
                (inner.y + 1).min(inner.bottom().saturating_sub(1)),
            ))
        }
        Overlay::Help => {
            draw_help_overlay(frame, inner);
            None
        }
        Overlay::DatePrompt { input, error } => {
            let lines = vec![
                Line::from("Jump to date (YYYY-MM-DD):"),
                Line::from(format!("> {input}")),
                Line::from(error.clone().unwrap_or_default()),
            ];
            frame.render_widget(Paragraph::new(lines).style(screen_style()), inner);
            Some((
                (inner.x + 2 + input.chars().count() as u16).min(inner.right().saturating_sub(1)),
                (inner.y + 1).min(inner.bottom().saturating_sub(1)),
            ))
        }
        Overlay::FindPrompt { input, error } => {
            let lines = vec![
                Line::from("Find in entry, then press Enter:"),
                Line::from(format!("> {input}")),
                Line::from(error.clone().unwrap_or_default()),
            ];
            frame.render_widget(Paragraph::new(lines).style(screen_style()), inner);
            Some((
                (inner.x + 2 + input.chars().count() as u16).min(inner.right().saturating_sub(1)),
                (inner.y + 1).min(inner.bottom().saturating_sub(1)),
            ))
        }
        Overlay::ReplacePrompt(prompt) => draw_replace_prompt_overlay(frame, inner, prompt),
        Overlay::ReplaceConfirm(confirm) => {
            let current = confirm
                .matches
                .get(confirm.current_idx)
                .map(|matched| format!("line {} col {}", matched.row + 1, matched.start_col + 1))
                .unwrap_or_else(|| "none".to_string());
            let lines = vec![
                Line::from("Replace next? (Y/N/A/Q)"),
                Line::from(format!("Find: {}", confirm.find_text)),
                Line::from(format!("With: {}", confirm.replace_text)),
                Line::from(format!(
                    "Current: {current} | Remaining: {}",
                    confirm.matches.len().saturating_sub(confirm.current_idx)
                )),
            ];
            frame.render_widget(Paragraph::new(lines).style(screen_style()), inner);
            None
        }
        Overlay::Index => {
            let lines = vec![
                Line::from("INDEX (MILESTONE 4 PLACEHOLDER)"),
                Line::from("--------------------------------"),
                Line::from("2026-03-14  Entry 0000000  \"No encrypted entries yet.\""),
                Line::from("2026-03-15  Entry 0000000  \"Save to create revisions.\""),
                Line::from("2026-03-16  Entry 0000001  \"Autosave drafts stay encrypted.\""),
                Line::from("Esc / Enter / F7 to close"),
            ];
            frame.render_widget(Paragraph::new(lines).style(screen_style()), inner);
            None
        }
        Overlay::RecoverDraft { .. } => {
            let lines = vec![
                Line::from("RECOVER UNSAVED DRAFT? (Y/N)"),
                Line::from("Y = load draft, N = keep latest revision"),
            ];
            frame.render_widget(Paragraph::new(lines).style(screen_style()), inner);
            None
        }
        Overlay::QuitConfirm => {
            let lines = vec![
                Line::from("Are you sure you want to quit?"),
                Line::from("Enter/Y = Yes, Esc/N = No"),
            ];
            frame.render_widget(Paragraph::new(lines).style(screen_style()), inner);
            None
        }
    }
}

fn draw_setup_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    wizard: &SetupWizard,
) -> Option<(u16, u16)> {
    let lines = vec![
        Line::from(wizard.title()),
        Line::from(format!("Vault: {}", wizard.path_input)),
        Line::from(wizard.prompt()),
        Line::from(format!("> {}", wizard.display_input())),
        Line::from(match wizard.step {
            SetupStep::VaultPath => "Step 1/4",
            SetupStep::Passphrase => "Step 2/4",
            SetupStep::ConfirmPassphrase => "Step 3/4",
            SetupStep::EpochDate => "Step 4/4",
        }),
        Line::from("Enter = Next, Esc = Quit setup"),
        Line::from(wizard.error.clone().unwrap_or_default()),
    ];
    frame.render_widget(Paragraph::new(lines).style(screen_style()), area);
    let input_len = wizard.display_input().chars().count() as u16;
    Some((
        (area.x + 2 + input_len).min(area.right().saturating_sub(1)),
        (area.y + 3).min(area.bottom().saturating_sub(1)),
    ))
}

fn draw_help_overlay(frame: &mut Frame<'_>, area: Rect) {
    let lines = vec![
        Line::from("+----------------------------------------------------+"),
        Line::from("| F1 Help         F2 Save        F3 Dates            |"),
        Line::from("| F4 Find         F6 Replace     F7 Index            |"),
        Line::from("| F10 Quit        Ctrl+S Save    Ctrl+F Find         |"),
        Line::from("|                                                    |"),
        Line::from("| Arrows move cursor | PgUp/PgDn scroll              |"),
        Line::from("| Enter newline      | Autosave every ~2.5 seconds   |"),
        Line::from("| Save appends a revision | Autosave writes draft    |"),
        Line::from("| Recovery prompt appears when draft is newer        |"),
        Line::from("+----------------------------------------------------+"),
    ];
    frame.render_widget(Paragraph::new(lines).style(screen_style()), area);
}

fn draw_replace_prompt_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    prompt: &ReplacePrompt,
) -> Option<(u16, u16)> {
    let active_find = prompt.stage == ReplaceStage::Find;
    let active_replace = prompt.stage == ReplaceStage::Replace;
    let lines = vec![
        Line::from(if active_find {
            "Step 1/2: Find text"
        } else {
            "Step 2/2: Replace with"
        }),
        input_line("Find", &prompt.find_input, active_find),
        input_line("With", &prompt.replace_input, active_replace),
        Line::from("Tab=toggle  Enter=next/start  Esc=cancel"),
        Line::from(prompt.error.clone().unwrap_or_default()),
    ];
    frame.render_widget(Paragraph::new(lines).style(screen_style()), area);
    let active_input = if active_find {
        &prompt.find_input
    } else {
        &prompt.replace_input
    };
    let y = if active_find { area.y + 1 } else { area.y + 2 };
    Some((
        (area.x + 8 + active_input.chars().count() as u16).min(area.right().saturating_sub(1)),
        y.min(area.bottom().saturating_sub(1)),
    ))
}

fn input_line(label: &str, value: &str, active: bool) -> Line<'static> {
    let marker = if active { '>' } else { ' ' };
    let style = if active {
        screen_style().add_modifier(Modifier::REVERSED)
    } else {
        screen_style()
    };
    Line::from(Span::styled(format!("{marker} {label}: {value}"), style))
}

fn overlay_title(overlay: &Overlay) -> &'static str {
    match overlay {
        Overlay::SetupWizard(_) => " Setup ",
        Overlay::UnlockPrompt { .. } => " Unlock ",
        Overlay::Help => " Help ",
        Overlay::DatePrompt { .. } => " Dates ",
        Overlay::FindPrompt { .. } => " Find ",
        Overlay::ReplacePrompt(_) => " Replace ",
        Overlay::ReplaceConfirm(_) => " Replace ",
        Overlay::Index => " Index ",
        Overlay::RecoverDraft { .. } => " Recovery ",
        Overlay::QuitConfirm => " Quit ",
    }
}

fn line_with_matches(
    text: &str,
    row: usize,
    matches: &[MatchPos],
    current: Option<&MatchPos>,
) -> Line<'static> {
    let row_matches = matches
        .iter()
        .filter(|matched| matched.row == row)
        .collect::<Vec<_>>();
    if row_matches.is_empty() {
        return Line::from(text.to_string());
    }

    let mut spans = Vec::new();
    let mut col = 0usize;
    for matched in row_matches {
        if matched.start_col > col {
            spans.push(Span::raw(slice_by_chars(text, col, matched.start_col)));
        }
        let highlight = slice_by_chars(text, matched.start_col, matched.end_col);
        let mut style = screen_style().fg(Color::Black).bg(Color::Yellow);
        if current == Some(matched) {
            style = style.add_modifier(Modifier::BOLD);
        }
        spans.push(Span::styled(highlight, style));
        col = matched.end_col;
    }
    let total_chars = text.chars().count();
    if col < total_chars {
        spans.push(Span::raw(slice_by_chars(text, col, total_chars)));
    }
    Line::from(spans)
}

fn slice_by_chars(input: &str, start: usize, end: usize) -> String {
    input
        .chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}

fn popup_rect(area: Rect, width: u16, height: u16) -> Rect {
    let popup_width = width.min(area.width.saturating_sub(2).max(1));
    let popup_height = height.min(area.height.saturating_sub(2).max(1));
    let x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_height)) / 2;
    Rect::new(x, y, popup_width, popup_height)
}

fn join_left_right(total_width: usize, left: &str, right: &str) -> String {
    if total_width == 0 {
        return String::new();
    }
    let left_len = left.chars().count();
    let right_len = right.chars().count();
    if left_len + right_len + 1 > total_width {
        return truncate_to_width(&format!("{left} {right}"), total_width);
    }
    format!(
        "{left}{}{right}",
        " ".repeat(total_width - left_len - right_len)
    )
}

fn truncate_to_width(input: &str, width: usize) -> String {
    input.chars().take(width).collect()
}

fn screen_style() -> Style {
    Style::default().fg(Color::White).bg(ROYAL_BLUE)
}

#[cfg(test)]
mod tests {
    use super::popup_rect;
    use ratatui::layout::Rect;

    #[test]
    fn popup_rect_stays_inside_small_area() {
        let area = Rect::new(0, 0, 10, 3);
        let popup = popup_rect(area, 20, 10);
        assert!(popup.width <= area.width);
        assert!(popup.height <= area.height);
    }
}
