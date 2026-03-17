pub mod app;
pub mod buffer;
pub mod calendar;

use crate::tui::{
    app::{
        App, ConflictMode, ConflictOverlay, DatePicker, IndexState, Overlay, ReplacePrompt,
        ReplaceStage, SearchField, SearchOverlay, SetupStep, SetupWizard,
    },
    buffer::MatchPos,
};
use chrono::{Datelike, NaiveDate};
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
use std::{env, io, sync::OnceLock, time::Duration};

const MIN_WIDTH: u16 = 80;
const MIN_HEIGHT: u16 = 25;

pub fn run(initial_date: Option<NaiveDate>) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, cursor::Hide)?;
    let _ = execute!(stdout, cursor::SetCursorStyle::BlinkingBlock);

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let result = run_loop(&mut terminal, initial_date);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), cursor::Show, LeaveAlternateScreen)?;
    let _ = execute!(
        terminal.backend_mut(),
        cursor::SetCursorStyle::DefaultUserShape
    );
    terminal.show_cursor()?;

    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    initial_date: Option<NaiveDate>,
) -> io::Result<()> {
    let mut app = App::with_initial_date(initial_date);
    let poll_timeout = Duration::from_millis(80);

    while !app.should_quit() {
        terminal.draw(|frame| draw(frame, &app))?;
        let area = terminal.size()?;
        let viewport_height = app.editor_viewport_height(area.height.saturating_sub(2) as usize);

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

    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        draw_small_terminal_warning(frame, area);
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
        app.header_date_time_label(),
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

    let show_reveal = app.reveal_codes_enabled() && area.height > 1;
    let mut constraints = Vec::new();
    if show_reveal {
        constraints.push(Constraint::Length(1));
    }
    let show_closing = area.height > u16::from(show_reveal) + 1;
    constraints.push(Constraint::Min(1));
    if show_closing {
        constraints.push(Constraint::Length(1));
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let mut chunk_index = 0usize;
    if show_reveal {
        frame.render_widget(
            Paragraph::new(Line::from(truncate_to_width(
                &app.reveal_codes_line(),
                chunks[0].width as usize,
            )))
            .style(screen_style().add_modifier(Modifier::BOLD)),
            chunks[0],
        );
        chunk_index += 1;
    }

    let editor_area = chunks[chunk_index];
    let start = app.scroll_row();
    let end = (start + editor_area.height as usize).min(app.buffer().line_count());
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
    frame.render_widget(Paragraph::new(lines).style(screen_style()), editor_area);

    if show_closing {
        let closing_text = app.closing_thought().unwrap_or("[none]");
        let closing_line = join_left_right(
            chunks[chunk_index + 1].width as usize,
            &format!("CLOSING THOUGHT: {}", truncate_to_width(closing_text, 48)),
            "F9 EDIT",
        );
        frame.render_widget(
            Paragraph::new(Line::from(closing_line))
                .style(screen_style().add_modifier(Modifier::UNDERLINED)),
            chunks[chunk_index + 1],
        );
    }

    if app.overlay().is_some() {
        return None;
    }
    let (row, col) = app.buffer().cursor();
    if row < start || row >= end {
        return None;
    }
    let x = (editor_area.x + col as u16).min(editor_area.right().saturating_sub(1));
    let y = (editor_area.y + (row - start) as u16).min(editor_area.bottom().saturating_sub(1));
    Some((x, y))
}

fn draw_footer(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let status = app.status_text().unwrap_or("");
    let strip = "F1Help F2Save F3Date F4Find F5Srch F6Repl F7Idx F9Close F10Quit F11Rev F12Lock";
    let content = join_left_right(area.width as usize, status, strip);
    frame.render_widget(
        Paragraph::new(Line::from(content)).style(
            screen_style()
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::REVERSED),
        ),
        area,
    );
}

fn draw_small_terminal_warning(frame: &mut Frame<'_>, area: Rect) {
    frame.render_widget(Block::new().style(screen_style()), area);
    let lines = vec![
        centered_line(area.width as usize, "BLUESCREEN JOURNAL"),
        Line::from(""),
        centered_line(
            area.width as usize,
            &format!(
                "TERMINAL TOO SMALL: NEED AT LEAST {}x{}",
                MIN_WIDTH, MIN_HEIGHT
            ),
        ),
        centered_line(
            area.width as usize,
            "Resize the window, then continue typing.",
        ),
        centered_line(
            area.width as usize,
            "Resize to restore prompts and editing.",
        ),
    ];
    frame.render_widget(Paragraph::new(lines).style(screen_style()), area);
}

fn draw_overlay(frame: &mut Frame<'_>, app: &App, body_area: Rect) -> Option<(u16, u16)> {
    let overlay = app.overlay()?;
    let rect = match overlay {
        Overlay::SetupWizard(_) => popup_rect(body_area, 76, 9),
        Overlay::UnlockPrompt { .. } => popup_rect(body_area, 64, 6),
        Overlay::Help => popup_rect(body_area, 72, 15),
        Overlay::DatePicker(_) => popup_rect(body_area, 38, 12),
        Overlay::FindPrompt { .. } => popup_rect(body_area, 54, 6),
        Overlay::ClosingPrompt { .. } => popup_rect(body_area, 58, 5),
        Overlay::ConflictChoice(_) => popup_rect(body_area, 72, 9),
        Overlay::MergeDiff(_) => popup_rect(body_area, 92, 18),
        Overlay::Search(_) => popup_rect(body_area, 84, 16),
        Overlay::ReplacePrompt(_) => popup_rect(body_area, 58, 8),
        Overlay::ReplaceConfirm(_) => popup_rect(body_area, 62, 8),
        Overlay::Index(_) => popup_rect(body_area, 78, 14),
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
        Overlay::DatePicker(picker) => {
            draw_date_picker_overlay(frame, inner, picker);
            None
        }
        Overlay::ConflictChoice(conflict) => {
            draw_conflict_choice_overlay(frame, inner, conflict);
            None
        }
        Overlay::MergeDiff(conflict) => {
            draw_merge_diff_overlay(frame, inner, conflict);
            None
        }
        Overlay::FindPrompt { input, error } => {
            let lines = vec![
                Line::from("Find in entry (live):"),
                Line::from(format!("> {input}")),
                Line::from("Up/Down move matches  Enter closes"),
                Line::from(error.clone().unwrap_or_default()),
            ];
            frame.render_widget(Paragraph::new(lines).style(screen_style()), inner);
            Some((
                (inner.x + 2 + input.chars().count() as u16).min(inner.right().saturating_sub(1)),
                (inner.y + 1).min(inner.bottom().saturating_sub(1)),
            ))
        }
        Overlay::ClosingPrompt { input } => {
            let lines = vec![
                Line::from("Closing Thought:"),
                Line::from(format!("> {input}")),
                Line::from("Enter save  Esc close"),
            ];
            frame.render_widget(Paragraph::new(lines).style(screen_style()), inner);
            Some((
                (inner.x + 2 + input.chars().count() as u16).min(inner.right().saturating_sub(1)),
                (inner.y + 1).min(inner.bottom().saturating_sub(1)),
            ))
        }
        Overlay::Search(search) => draw_search_overlay(frame, inner, search),
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
        Overlay::Index(index) => {
            draw_index_overlay(frame, inner, index);
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
        Line::from("| F4 Find         F5 Search      F6 Replace          |"),
        Line::from("| F7 Index        F9 Closing     F10 Quit            |"),
        Line::from("| F11 Reveal      F12 Lock       Ctrl+S Save         |"),
        Line::from("| Ctrl+F Find                                          |"),
        Line::from("|                                                    |"),
        Line::from("| Arrows move cursor | PgUp/PgDn scroll              |"),
        Line::from("| Find updates as you type | Search runs on Enter    |"),
        Line::from("| Calendar: Arrows move | PgUp/PgDn month            |"),
        Line::from("| Index: Up/Down/PgUp/PgDn | Enter opens date        |"),
        Line::from("| Search: Tab fields | Enter search/open result      |"),
        Line::from("| Closing Thought lives above footer | F9 edits it   |"),
        Line::from("| Reveal shows DATE / ENTRY / TAG / MOOD / CLOSE     |"),
        Line::from("| F12 locks and clears in-memory editor/search state |"),
        Line::from("+----------------------------------------------------+"),
    ];
    frame.render_widget(Paragraph::new(lines).style(screen_style()), area);
}

fn draw_date_picker_overlay(frame: &mut Frame<'_>, area: Rect, picker: &DatePicker) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let mut lines = Vec::new();
    lines.push(centered_line(area.width as usize, &picker.month_label()));

    let weekday_line = calendar::weekday_headers()
        .into_iter()
        .map(|day| format!("{:>2} ", weekday_label(day)))
        .collect::<String>()
        .trim_end()
        .to_string();
    lines.push(Line::from(weekday_line));

    for week in picker.grid() {
        let mut spans = Vec::new();
        for date in week {
            let mut style = screen_style();
            if date.month() != picker.month.month() {
                style = style.add_modifier(Modifier::DIM);
            }
            if picker.has_entry(date) {
                style = style.add_modifier(Modifier::BOLD);
            }
            if date == picker.selected_date {
                style = style.add_modifier(Modifier::REVERSED);
            }
            spans.push(Span::styled(format!("{:>2} ", date.day()), style));
        }
        lines.push(Line::from(spans));
    }

    lines.push(Line::from("Arrows move  PgUp/PgDn month"));
    lines.push(Line::from("Enter open   Esc close"));
    frame.render_widget(Paragraph::new(lines).style(screen_style()), area);
}

fn draw_search_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    search: &SearchOverlay,
) -> Option<(u16, u16)> {
    if area.width == 0 || area.height == 0 {
        return None;
    }

    let mut lines = vec![
        input_line(
            "Query",
            &search.query_input,
            search.active_field == SearchField::Query,
        ),
        input_line(
            "From ",
            &search.from_input,
            search.active_field == SearchField::From,
        ),
        input_line(
            "To   ",
            &search.to_input,
            search.active_field == SearchField::To,
        ),
        Line::from(""),
        Line::from("DATE         ENTRY NO  LOCATION  SNIPPET"),
        Line::from("----------------------------------------"),
    ];

    if search.results.is_empty() {
        lines.push(Line::from("No results yet. Enter runs the search."));
    } else {
        let visible_rows = area.height.saturating_sub(8) as usize;
        let (start, end) = search.window(visible_rows);
        let preview_width = area.width.saturating_sub(34) as usize;
        for (offset, result) in search.results[start..end].iter().enumerate() {
            let absolute_idx = start + offset;
            let mut spans = Vec::new();
            let row_style =
                if absolute_idx == search.selected && search.active_field == SearchField::Results {
                    screen_style().add_modifier(Modifier::REVERSED)
                } else {
                    screen_style()
                };
            spans.push(Span::styled(
                format!(
                    "{:<10}  {:<8}  {:>2}:{:<2}  ",
                    result.date.format("%Y-%m-%d"),
                    result.entry_number,
                    result.row + 1,
                    result.start_col + 1
                ),
                row_style,
            ));
            spans.extend(highlighted_snippet_spans(
                &truncate_snippet(&result.snippet.text, preview_width),
                result.snippet.highlight_start.min(preview_width),
                result.snippet.highlight_end.min(preview_width),
                absolute_idx == search.selected && search.active_field == SearchField::Results,
            ));
            lines.push(Line::from(spans));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(
        "Tab fields  Enter search/open  Up/Down move  Esc close",
    ));
    if let Some(error) = &search.error {
        lines.push(Line::from(error.clone()));
    }

    frame.render_widget(Paragraph::new(lines).style(screen_style()), area);

    let cursor_y = match search.active_field {
        SearchField::Query => area.y,
        SearchField::From => area.y + 1,
        SearchField::To => area.y + 2,
        SearchField::Results => return None,
    };
    let cursor_x = match search.active_field {
        SearchField::Query => area.x + 9 + search.query_input.chars().count() as u16,
        SearchField::From => area.x + 9 + search.from_input.chars().count() as u16,
        SearchField::To => area.x + 9 + search.to_input.chars().count() as u16,
        SearchField::Results => area.x,
    };
    Some((
        cursor_x.min(area.right().saturating_sub(1)),
        cursor_y.min(area.bottom().saturating_sub(1)),
    ))
}

fn draw_conflict_choice_overlay(frame: &mut Frame<'_>, area: Rect, conflict: &ConflictOverlay) {
    let head_a = conflict.conflict.heads.first();
    let head_b = conflict.conflict.heads.get(1).or(head_a);
    let more_heads = conflict.conflict.heads.len().saturating_sub(2);

    let mut lines = vec![
        Line::from("CONFLICT DETECTED"),
        conflict_option_line("1) View A", conflict.selected == ConflictMode::ViewA),
        Line::from(format!(
            "A: {}",
            head_a
                .map(|head| format!("{} {} {}", head.device_id, head.seq, head.preview))
                .unwrap_or_else(|| "Unavailable".to_string())
        )),
        conflict_option_line("2) View B", conflict.selected == ConflictMode::ViewB),
        Line::from(format!(
            "B: {}",
            head_b
                .map(|head| format!("{} {} {}", head.device_id, head.seq, head.preview))
                .unwrap_or_else(|| "Unavailable".to_string())
        )),
        conflict_option_line("3) Merge", conflict.selected == ConflictMode::Merge),
        Line::from(format!(
            "Merge keeps all heads and writes a new revision.{}",
            if more_heads > 0 {
                format!(" +{more_heads} more head(s) will be merged.")
            } else {
                String::new()
            }
        )),
        Line::from("Enter select  Tab cycle  Esc close"),
    ];

    frame.render_widget(
        Paragraph::new(std::mem::take(&mut lines)).style(screen_style()),
        area,
    );
}

fn draw_merge_diff_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    conflict: &crate::vault::ConflictState,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(2)])
        .split(area);
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[0]);

    let left = conflict.heads.first();
    let right = conflict.heads.get(1).or(left);

    draw_conflict_pane(frame, panes[0], "View A", left);
    draw_conflict_pane(frame, panes[1], "View B", right);

    let footer = vec![
        Line::from("Esc closes diff. Edit the main buffer underneath, then F2 saves the merge."),
        Line::from("Losing revisions are preserved and linked into the merged head."),
    ];
    frame.render_widget(Paragraph::new(footer).style(screen_style()), chunks[1]);
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

fn draw_index_overlay(frame: &mut Frame<'_>, area: Rect, index: &IndexState) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let mut lines = vec![
        Line::from("DATE         ENTRY NO  CF        PREVIEW"),
        Line::from("----------------------------------------"),
    ];

    if index.items.is_empty() {
        lines.push(Line::from("No saved entries."));
    } else {
        let visible_rows = area.height.saturating_sub(4) as usize;
        let (start, end) = index.window(visible_rows);
        let preview_width = area.width.saturating_sub(31) as usize;
        for (offset, entry) in index.items[start..end].iter().enumerate() {
            let absolute_idx = start + offset;
            let conflict = if entry.has_conflict { "CONFLICT" } else { "-" };
            let row = format!(
                "{:<10}  {:<8}  {:<8}  {}",
                entry.date.format("%Y-%m-%d"),
                entry.entry_number,
                conflict,
                truncate_to_width(&entry.preview, preview_width)
            );
            let style = if absolute_idx == index.selected {
                screen_style().add_modifier(Modifier::REVERSED)
            } else {
                screen_style()
            };
            lines.push(Line::from(Span::styled(row, style)));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from("Up/Down/PgUp/PgDn move  Enter open  Esc close"));
    frame.render_widget(Paragraph::new(lines).style(screen_style()), area);
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

fn conflict_option_line(label: &str, active: bool) -> Line<'static> {
    let style = if active {
        screen_style().add_modifier(Modifier::REVERSED)
    } else {
        screen_style().add_modifier(Modifier::BOLD)
    };
    Line::from(Span::styled(label.to_string(), style))
}

fn draw_conflict_pane(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    head: Option<&crate::vault::ConflictHead>,
) {
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" {title} "))
            .style(screen_style()),
        area,
    );
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let mut lines = Vec::new();
    if let Some(head) = head {
        lines.push(Line::from(format!(
            "{} seq {}  {}",
            head.device_id,
            head.seq,
            head.saved_at.format("%H:%M:%S")
        )));
        for line in head
            .body
            .lines()
            .take(inner.height.saturating_sub(1) as usize)
        {
            lines.push(Line::from(truncate_to_width(line, inner.width as usize)));
        }
    } else {
        lines.push(Line::from("Unavailable"));
    }

    frame.render_widget(Paragraph::new(lines).style(screen_style()), inner);
}

fn weekday_label(day: chrono::Weekday) -> &'static str {
    match day {
        chrono::Weekday::Sun => "Su",
        chrono::Weekday::Mon => "Mo",
        chrono::Weekday::Tue => "Tu",
        chrono::Weekday::Wed => "We",
        chrono::Weekday::Thu => "Th",
        chrono::Weekday::Fri => "Fr",
        chrono::Weekday::Sat => "Sa",
    }
}

fn centered_line(width: usize, text: &str) -> Line<'static> {
    let text_width = text.chars().count();
    let left_padding = width.saturating_sub(text_width) / 2;
    Line::from(format!("{}{}", " ".repeat(left_padding), text))
}

fn overlay_title(overlay: &Overlay) -> &'static str {
    match overlay {
        Overlay::SetupWizard(_) => " Setup ",
        Overlay::UnlockPrompt { .. } => " Unlock ",
        Overlay::Help => " Help ",
        Overlay::DatePicker(_) => " Dates ",
        Overlay::ConflictChoice(_) => " Conflict ",
        Overlay::MergeDiff(_) => " Merge ",
        Overlay::FindPrompt { .. } => " Find ",
        Overlay::ClosingPrompt { .. } => " Closing ",
        Overlay::Search(_) => " Search ",
        Overlay::ReplacePrompt(_) => " Replace ",
        Overlay::ReplaceConfirm(_) => " Replace ",
        Overlay::Index(_) => " Index ",
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
        let mut style = screen_style()
            .fg(color_palette().highlight_fg)
            .bg(color_palette().highlight_bg);
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

fn truncate_snippet(input: &str, width: usize) -> String {
    truncate_to_width(input, width)
}

fn highlighted_snippet_spans(
    text: &str,
    highlight_start: usize,
    highlight_end: usize,
    selected: bool,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let base_style = if selected {
        screen_style().add_modifier(Modifier::REVERSED)
    } else {
        screen_style()
    };
    if highlight_start > 0 {
        spans.push(Span::styled(
            slice_by_chars(text, 0, highlight_start),
            base_style,
        ));
    }
    if highlight_end > highlight_start {
        let highlight_style = if selected {
            base_style
                .fg(color_palette().highlight_fg)
                .bg(color_palette().highlight_bg)
                .add_modifier(Modifier::BOLD)
        } else {
            screen_style()
                .fg(color_palette().highlight_fg)
                .bg(color_palette().highlight_bg)
                .add_modifier(Modifier::BOLD)
        };
        spans.push(Span::styled(
            slice_by_chars(text, highlight_start, highlight_end),
            highlight_style,
        ));
    }
    let total_chars = text.chars().count();
    if highlight_end < total_chars {
        spans.push(Span::styled(
            slice_by_chars(text, highlight_end, total_chars),
            base_style,
        ));
    }
    spans
}

fn screen_style() -> Style {
    let palette = color_palette();
    Style::default().fg(palette.fg).bg(palette.bg)
}

#[derive(Clone, Copy)]
struct ColorPalette {
    fg: Color,
    bg: Color,
    highlight_fg: Color,
    highlight_bg: Color,
}

fn color_palette() -> &'static ColorPalette {
    static PALETTE: OnceLock<ColorPalette> = OnceLock::new();
    PALETTE.get_or_init(|| {
        let truecolor = env::var("COLORTERM")
            .map(|value| {
                let lowered = value.to_ascii_lowercase();
                lowered.contains("truecolor") || lowered.contains("24bit")
            })
            .unwrap_or(false);
        if truecolor {
            return ColorPalette {
                fg: Color::White,
                bg: Color::Rgb(65, 105, 225),
                highlight_fg: Color::Black,
                highlight_bg: Color::Yellow,
            };
        }

        let term = env::var("TERM").unwrap_or_default().to_ascii_lowercase();
        if term.contains("256color") {
            return ColorPalette {
                fg: Color::Indexed(15),
                bg: Color::Indexed(20),
                highlight_fg: Color::Indexed(16),
                highlight_bg: Color::Indexed(226),
            };
        }

        ColorPalette {
            fg: Color::White,
            bg: Color::Blue,
            highlight_fg: Color::Black,
            highlight_bg: Color::Yellow,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{centered_line, popup_rect};
    use ratatui::layout::Rect;

    #[test]
    fn popup_rect_stays_inside_small_area() {
        let area = Rect::new(0, 0, 10, 3);
        let popup = popup_rect(area, 20, 10);
        assert!(popup.width <= area.width);
        assert!(popup.height <= area.height);
    }

    #[test]
    fn centered_line_adds_left_padding() {
        let line = centered_line(10, "TEST");
        assert_eq!(line.width(), 7);
    }
}
