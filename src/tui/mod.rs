pub mod app;
pub mod buffer;
pub mod calendar;

use crate::tui::{
    app::{
        AiCoachOverlay, App, CloudCredentialField, CloudCredentialPrompt, ConflictMode,
        ConflictOverlay, DatePicker, ExportPrompt, IndexState, InfoOverlay, MenuId, MenuItem,
        MetadataField, MetadataPrompt, Overlay, PickerOverlay, ReplacePrompt, ReplaceStage,
        RestorePrompt, RestoreStage, SearchField, SearchOverlay, SettingPrompt, SetupStep,
        SetupWizard, SyncPhase, SyncStatusOverlay, index_detail_summary, index_row_flags,
    },
    buffer::MatchPos,
};
use chrono::{Datelike, Local, NaiveDate};
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

const MIN_WIDTH: u16 = 56;
const MIN_HEIGHT: u16 = 18;
const DOS_WIDTH: u16 = 80;
const DOS_HEIGHT: u16 = 25;

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
    app.enable_soundtrack_autoplay();
    let poll_timeout = Duration::from_millis(80);

    while !app.should_quit() {
        terminal.draw(|frame| draw(frame, &app))?;
        let area = terminal.size()?;
        let screen = workspace_rect(Rect::new(0, 0, area.width, area.height));
        let viewport_height = app.editor_viewport_height(screen.height.saturating_sub(3) as usize);
        let viewport_width = screen.width.max(1) as usize;

        if event::poll(poll_timeout)? {
            let ev = event::read()?;
            app.handle_event_with_viewport(ev, viewport_height, viewport_width);
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

    // Ratatui widgets style cells but may not rewrite symbols in untouched regions.
    // Clear first so closing menus/overlays never leaves stale glyph artifacts behind.
    frame.render_widget(Clear, area);

    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        draw_small_terminal_warning(frame, area);
        return;
    }

    frame.render_widget(Block::new().style(screen_style()), area);
    let screen = workspace_rect(area);
    let compact_mode = screen.width < DOS_WIDTH || screen.height < DOS_HEIGHT;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(screen);

    let header_area = chunks[0];
    let menu_area = chunks[1];
    let body_area = chunks[2];
    let footer_area = chunks[3];

    draw_header(frame, app, header_area, compact_mode);
    draw_menu_bar(frame, app, menu_area);
    let editor_cursor = draw_editor(frame, app, body_area);
    draw_footer(frame, app, footer_area, compact_mode);

    draw_menu_dropdown(frame, app, menu_area, body_area);
    let overlay_cursor = draw_overlay(frame, app, body_area);
    if let Some((x, y)) = overlay_cursor.or(editor_cursor) {
        frame.set_cursor_position((x, y));
    }
}

fn draw_header(frame: &mut Frame<'_>, app: &App, area: Rect, compact_mode: bool) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let left = format!(
        "BLUESCREEN JOURNAL{}{}  ENTRY DATE {} [{}]  ENTRY NO. {}  PAGE {}  TIME {}  VER {}",
        if compact_mode { " [COMPACT]" } else { "" },
        if app.favorite_marker().is_empty() {
            ""
        } else {
            " *"
        },
        app.header_entry_focus_label(),
        app.header_day_delta_label(),
        app.entry_number_label(),
        app.header_page_state_label(),
        app.header_time_label(),
        app.app_version_label(),
    );
    let right_candidates = vec![
        app.save_status_label(),
        app.lock_status_label().to_string(),
        app.save_reminder_label().to_string(),
        app.draft_recovered_label().to_string(),
        app.integrity_status_label(),
        app.word_goal_status_label(),
        app.soundtrack_status_label().to_string(),
        app.streak_status_label(),
        app.sprint_status_label(),
        app.session_status_label(),
    ];
    let left_len = left.chars().count();
    let budget = area.width as usize - left_len.min(area.width as usize);
    let mut right_parts = Vec::new();
    let mut used = 0usize;
    for part in right_candidates.into_iter().filter(|part| !part.is_empty()) {
        let part_len = part.chars().count();
        let separator = if right_parts.is_empty() { 0 } else { 3 };
        if used + separator + part_len > budget.saturating_sub(1) {
            continue;
        }
        used += separator + part_len;
        right_parts.push(part);
    }
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

fn draw_menu_bar(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let tabs = menu_tabs(area);
    let mut spans = Vec::new();
    for (index, tab) in tabs.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw(" "));
        }
        let mut style = screen_style().add_modifier(Modifier::BOLD);
        if app
            .menu()
            .map(|menu| menu.selected_menu == tab.id)
            .unwrap_or(false)
        {
            style = style.add_modifier(Modifier::REVERSED);
        }
        let mut chars = tab.label.chars();
        let first = chars.next().unwrap_or(' ');
        let rest = chars.collect::<String>();
        spans.push(Span::styled(" ".to_string(), style));
        spans.push(Span::styled(
            first.to_string(),
            style.add_modifier(Modifier::UNDERLINED),
        ));
        spans.push(Span::styled(rest, style));
        spans.push(Span::styled(" ".to_string(), style));
    }

    let hint = if app.menu().is_some() {
        "ARROWS MOVE  ENTER SELECT  ESC CLOSE  ALT+F/E/S/G/T/U/H JUMP"
    } else if app.should_show_menu_discovery_hint() {
        if area.width >= 130 {
            "ESC MENUS  F1 HELP  F2 SAVE  ALT+N NEXT  F3 CALENDAR  F7 INDEX"
        } else if area.width >= 104 {
            "ESC MENUS  F1 HELP  F2 SAVE  ALT+N NEXT  F3 CAL"
        } else {
            "ESC MENUS  F1 HELP  F2 SAVE"
        }
    } else if area.width >= 130 {
        "ESC MENUS  ALT+F/E/S/G/T/U/H JUMP  F2 SAVE  ALT+N NEXT  F3 CAL  F7 INDEX"
    } else if area.width >= 104 {
        "ESC MENUS  ALT+F/E/S/G/T/U/H  F2 SAVE  ALT+N NEXT  F3 CAL"
    } else {
        "ESC MENUS  F1 HELP  F2 SAVE"
    };
    let left_width = spans
        .iter()
        .map(|span| span.content.chars().count())
        .sum::<usize>();
    let available = area.width as usize - left_width;
    if available > 1 {
        let rendered_hint = truncate_to_width(hint, available.saturating_sub(1));
        spans.push(Span::raw(
            " ".repeat(available - rendered_hint.chars().count()),
        ));
        spans.push(Span::styled(
            rendered_hint,
            screen_style().add_modifier(Modifier::UNDERLINED),
        ));
    }

    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(screen_style()),
        area,
    );
}

fn draw_menu_dropdown(frame: &mut Frame<'_>, app: &App, menu_area: Rect, body_area: Rect) {
    let Some(menu) = app.menu() else {
        return;
    };
    if body_area.width == 0 || body_area.height == 0 {
        return;
    }

    let items = app.menu_items(menu.selected_menu);
    if items.is_empty() {
        return;
    }

    let tabs = menu_tabs(menu_area);
    let Some(selected_tab) = tabs.iter().find(|tab| tab.id == menu.selected_menu) else {
        return;
    };

    let content_width = items
        .iter()
        .map(|item| item.label.chars().count() + item.detail.chars().count() + 3)
        .max()
        .unwrap_or(18)
        .max(selected_tab.label.chars().count() + 4);
    let width = (content_width as u16 + 2).min(body_area.width.max(1));
    let height = (items.len() as u16 + 2).min(body_area.height.max(1));
    let max_x = body_area.right().saturating_sub(width);
    let x = selected_tab.x.max(body_area.x).min(max_x.max(body_area.x));
    let rect = Rect::new(x, body_area.y, width, height);

    frame.render_widget(Clear, rect);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", menu.selected_menu.title()))
            .style(screen_style()),
        rect,
    );
    let inner = rect.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });

    let visible_rows = inner.height as usize;
    let start = menu
        .selected_item
        .saturating_sub(visible_rows.saturating_sub(1) / 2)
        .min(items.len().saturating_sub(visible_rows));
    let end = (start + visible_rows).min(items.len());
    let mut lines = Vec::new();
    for (offset, item) in items[start..end].iter().enumerate() {
        let absolute_idx = start + offset;
        lines.push(menu_item_line(
            inner.width as usize,
            item,
            absolute_idx == menu.selected_item,
        ));
    }

    frame.render_widget(Paragraph::new(lines).style(screen_style()), inner);
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
    let show_ruler = app.show_ruler_enabled() && area.height > u16::from(show_reveal) + 2;
    if show_ruler {
        constraints.push(Constraint::Length(1));
    }
    let show_closing = area.height > u16::from(show_reveal) + u16::from(show_ruler) + 1;
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

    if show_ruler {
        frame.render_widget(
            Paragraph::new(Line::from(ruler_spans(
                app.buffer().cursor().1,
                chunks[chunk_index].width as usize,
            )))
            .style(screen_style().add_modifier(Modifier::DIM)),
            chunks[chunk_index],
        );
        chunk_index += 1;
    }

    let editor_area = chunks[chunk_index];
    let start = app.scroll_row();
    let end = (start + editor_area.height as usize).min(app.buffer().line_count());
    let mut lines = Vec::new();
    let empty_editor = app.buffer().line_count() == 1
        && app.buffer().line(0).unwrap_or_default().is_empty()
        && app.overlay().is_none();
    if empty_editor {
        lines.push(Line::from(String::new()));
    } else {
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
    }
    frame.render_widget(Paragraph::new(lines).style(screen_style()), editor_area);

    if empty_editor {
        let hints = app
            .empty_state_lines()
            .into_iter()
            .map(|line| centered_line(editor_area.width as usize, line.as_str()))
            .collect::<Vec<_>>();
        let hint_height = hints.len() as u16;
        let hint_y = editor_area
            .y
            .saturating_add(editor_area.height.saturating_sub(hint_height) / 3);
        let hint_area = Rect::new(editor_area.x, hint_y, editor_area.width, hint_height);
        frame.render_widget(
            Paragraph::new(hints).style(screen_style().add_modifier(Modifier::DIM)),
            hint_area,
        );
    }

    if show_closing {
        let (closing_text, closing_hint) = match app.closing_thought() {
            Some(text) => (truncate_to_width(text, 48), "F9 EDIT/CLEAR (EDIT MENU)"),
            None => (
                "[none - press F9 to add] (closing thought)".to_string(),
                "F9 ADD",
            ),
        };
        let closing_line = join_left_right(
            chunks[chunk_index + 1].width as usize,
            &format!("CLOSING THOUGHT: {closing_text}"),
            closing_hint,
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

fn draw_footer(frame: &mut Frame<'_>, app: &App, area: Rect, compact_mode: bool) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let context = format!(
        "STATE {} | SAVE {} | {} | {} | VER {}",
        app.footer_mode_label(),
        app.save_status_label(),
        app.footer_context_label(),
        app.footer_stats_label(),
        app.app_version_label(),
    );
    let status = app.status_text().unwrap_or("");
    let left = if status.is_empty() {
        if let Some(next_hint) = app.footer_next_hint() {
            format!("{context} | {next_hint}")
        } else {
            context
        }
    } else {
        format!("{context} | {status}")
    };
    let strip = footer_legend(app, area.width as usize, compact_mode);
    let content = join_left_right(area.width as usize, &left, &strip);
    frame.render_widget(
        Paragraph::new(Line::from(content)).style(
            screen_style()
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::REVERSED),
        ),
        area,
    );
}

fn footer_legend(app: &App, width: usize, compact_mode: bool) -> String {
    if !app.show_footer_legend_enabled() {
        return "CTRL+K COMMANDS".to_string();
    }

    if app.menu().is_some() {
        if width >= 96 {
            return "MENU ARROWS MOVE | ENTER SELECT | ESC CLOSE | ALT+F/E/S/G/T/U/H JUMP"
                .to_string();
        }
        return "MENU ARROWS MOVE | ENTER SELECT | ESC CLOSE".to_string();
    }

    if let Some(hint) = app.overlay_footer_hint() {
        if width >= hint.chars().count() {
            return hint.to_string();
        }
        return truncate_to_width(hint, width.max(1));
    }

    let legend = if width >= 130 {
        "F1 HELP  F2 SAVE  ALT+N NEXT  F3 CALENDAR  F5 SEARCH  F7 INDEX  F8 SYNC  ESC MENUS"
            .to_string()
    } else if width >= 108 {
        "F1 HELP  F2 SAVE  ALT+N NEXT  F3 CAL  F5 SEARCH  ESC MENUS".to_string()
    } else if width >= 90 {
        "F1 HELP  F2 SAVE  ALT+N NEXT  ESC MENUS".to_string()
    } else {
        "F2 SAVE  ESC MENUS".to_string()
    };

    if compact_mode && width >= 100 {
        format!("{legend} | COMPACT")
    } else {
        legend
    }
}

fn draw_small_terminal_warning(frame: &mut Frame<'_>, area: Rect) {
    frame.render_widget(Block::new().style(screen_style()), area);
    let lines = vec![
        centered_line(area.width as usize, "BLUESCREEN JOURNAL"),
        Line::from(""),
        centered_line(
            area.width as usize,
            &format!(
                "TERMINAL TOO SMALL: NEED {}x{}  CURRENT {}x{}",
                MIN_WIDTH, MIN_HEIGHT, area.width, area.height
            ),
        ),
        centered_line(
            area.width as usize,
            &format!(
                "Missing: {} cols, {} rows",
                MIN_WIDTH.saturating_sub(area.width),
                MIN_HEIGHT.saturating_sub(area.height)
            ),
        ),
        centered_line(
            area.width as usize,
            "Current size is below the usable minimum.",
        ),
        centered_line(
            area.width as usize,
            &format!(
                "Need at least {}x{} to edit. {}x{} is recommended.",
                MIN_WIDTH, MIN_HEIGHT, DOS_WIDTH, DOS_HEIGHT
            ),
        ),
        Line::from(""),
        centered_line(
            area.width as usize,
            "Tip: maximize terminal and reduce font size one step.",
        ),
        centered_line(
            area.width as usize,
            "After resize: press ? for help or Esc for menus.",
        ),
    ];
    frame.render_widget(Paragraph::new(lines).style(screen_style()), area);
}

fn draw_overlay(frame: &mut Frame<'_>, app: &App, body_area: Rect) -> Option<(u16, u16)> {
    let overlay = app.overlay()?;
    let rect = match overlay {
        Overlay::SetupWizard(_) => popup_rect(body_area, 76, 10),
        Overlay::UnlockPrompt { .. } => popup_rect(body_area, 68, 8),
        Overlay::Help => popup_rect(body_area, 76, 26),
        Overlay::DatePicker(_) => popup_rect(body_area, 38, 13),
        Overlay::FindPrompt { .. } => popup_rect(body_area, 54, 6),
        Overlay::ClosingPrompt { .. } => popup_rect(body_area, 72, 7),
        Overlay::ConflictChoice(_) => popup_rect(body_area, 72, 15),
        Overlay::MergeDiff(_) => popup_rect(body_area, 92, 18),
        Overlay::Search(_) => popup_rect(body_area, 90, 22),
        Overlay::AiCoach(_) => popup_rect(body_area, 78, 13),
        Overlay::ReplacePrompt(_) => popup_rect(body_area, 58, 8),
        Overlay::ReplaceConfirm(_) => popup_rect(body_area, 62, 8),
        Overlay::ExportPrompt(_) => popup_rect(body_area, 72, 9),
        Overlay::SettingPrompt(_) => popup_rect(body_area, 70, 8),
        Overlay::CloudCredentialPrompt(_) => popup_rect(body_area, 76, 10),
        Overlay::MetadataPrompt(_) => popup_rect(body_area, 72, 9),
        Overlay::Index(_) => popup_rect(body_area, 78, 16),
        Overlay::SyncStatus(_) => popup_rect(body_area, 76, 14),
        Overlay::Info(_) => popup_rect(body_area, 76, 16),
        Overlay::Picker(_) => popup_rect(body_area, 76, 14),
        Overlay::RestorePrompt(_) => popup_rect(body_area, 76, 12),
        Overlay::RecoverDraft { .. } => popup_rect(body_area, 44, 5),
        Overlay::PruneConfirm { .. } => popup_rect(body_area, 52, 6),
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
            draw_unlock_overlay(frame, inner, app, input, error)
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
                Line::from("Edit Closing Thought:"),
                Line::from(format!("> {input}")),
                Line::from("Tip: Keep it short, intentional, and ending-focused."),
                Line::from("Example: \"Tomorrow I choose calm focus over noise.\""),
                Line::from("Enter save  Esc cancel  blank + Enter clears"),
            ];
            frame.render_widget(Paragraph::new(lines).style(screen_style()), inner);
            Some((
                (inner.x + 2 + input.chars().count() as u16).min(inner.right().saturating_sub(1)),
                (inner.y + 1).min(inner.bottom().saturating_sub(1)),
            ))
        }
        Overlay::Search(search) => draw_search_overlay(frame, inner, app, search),
        Overlay::AiCoach(coach) => draw_ai_coach_overlay(frame, inner, coach),
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
        Overlay::ExportPrompt(prompt) => draw_export_prompt_overlay(frame, inner, prompt),
        Overlay::SettingPrompt(prompt) => draw_setting_prompt_overlay(frame, inner, prompt),
        Overlay::CloudCredentialPrompt(prompt) => {
            draw_cloud_credential_prompt_overlay(frame, inner, prompt)
        }
        Overlay::MetadataPrompt(prompt) => draw_metadata_prompt_overlay(frame, inner, prompt),
        Overlay::Index(index) => {
            draw_index_overlay(frame, inner, index);
            None
        }
        Overlay::SyncStatus(sync_status) => {
            draw_sync_overlay(frame, inner, sync_status);
            None
        }
        Overlay::Info(info) => {
            draw_info_overlay(frame, inner, info);
            None
        }
        Overlay::Picker(picker) => {
            draw_picker_overlay(frame, inner, picker);
            None
        }
        Overlay::RestorePrompt(prompt) => draw_restore_prompt_overlay(frame, inner, prompt),
        Overlay::RecoverDraft { .. } => {
            let lines = vec![
                Line::from("RECOVER UNSAVED DRAFT? (Y/N)"),
                Line::from("Y = load draft, N = keep latest revision"),
            ];
            frame.render_widget(Paragraph::new(lines).style(screen_style()), inner);
            None
        }
        Overlay::PruneConfirm { prune_count } => {
            let lines = vec![
                Line::from("PRUNE OLD ENCRYPTED BACKUPS? (Y/N)"),
                Line::from(format!(
                    "Would remove {prune_count} backup(s) by current retention."
                )),
                Line::from("Y/Enter = prune now, N/Esc = cancel"),
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
    let step_hint = match wizard.step {
        SetupStep::VaultPath => "Enter accepts the default Documents vault path.",
        SetupStep::Passphrase => "Use a long passphrase; journal text stays encrypted on disk.",
        SetupStep::ConfirmPassphrase => "Re-enter the same passphrase exactly.",
        SetupStep::EpochDate => "Leave blank to start numbering from the vault creation date.",
    };
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
        Line::from(step_hint),
        Line::from("Enter = Next, Ctrl+Q = Quit setup"),
        Line::from(wizard.error.clone().unwrap_or_default()),
    ];
    frame.render_widget(Paragraph::new(lines).style(screen_style()), area);
    let input_len = wizard.display_input().chars().count() as u16;
    Some((
        (area.x + 2 + input_len).min(area.right().saturating_sub(1)),
        (area.y + 3).min(area.bottom().saturating_sub(1)),
    ))
}

fn draw_unlock_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &App,
    input: &str,
    error: &Option<String>,
) -> Option<(u16, u16)> {
    let masked = "*".repeat(input.chars().count());
    let vault_label = truncate_to_width(&app.vault_path_label(), area.width as usize);
    let keychain_hint = if app.keychain_memory_enabled() {
        "Keychain memory is enabled for this vault."
    } else {
        "SETUP can enable Keychain memory after unlock."
    };
    let lines = vec![
        Line::from("Enter vault passphrase to unlock:"),
        Line::from(format!("Vault: {vault_label}")),
        Line::from(format!("> {masked}")),
        Line::from(keychain_hint),
        Line::from("Enter unlock  Ctrl+Q quit"),
        Line::from(error.clone().unwrap_or_default()),
    ];
    frame.render_widget(Paragraph::new(lines).style(screen_style()), area);
    Some((
        (area.x + 2 + masked.chars().count() as u16).min(area.right().saturating_sub(1)),
        (area.y + 2).min(area.bottom().saturating_sub(1)),
    ))
}

fn draw_help_overlay(frame: &mut Frame<'_>, area: Rect) {
    let version_line = format!(
        "BlueScreen Journal {}  |  Classic 80x25 workspace.",
        env!("CARGO_PKG_VERSION")
    );
    let lines = vec![
        Line::from(version_line),
        Line::from("(c) 2026 Awassee LLC and Sean Heiney  sean@sean.net"),
        Line::from("Flow: TYPE -> F2 SAVE -> **save** NEXT ENTRY -> ALT+N NEXT DAY."),
        Line::from("ESC opens menus. Arrows move. ENTER selects."),
        Line::from("ALT+F/E/S/G/T/U/H jumps menu tabs."),
        Line::from("CTRL+O/E/W/Y/T/U/L also opens menus."),
        Line::from("ALT+,/. day  ALT+[ ] saved jump  ALT+Y/0 today"),
        Line::from("ALT+D calendar  ALT+I index  ALT+K command palette"),
        Line::from(
            "ALT+- yesterday  ALT+= tomorrow  CTRL+SHIFT+S save+next day  CTRL+SHIFT+L save+lock",
        ),
        Line::from("? or Ctrl+/ opens this card instantly."),
        Line::from("F1 Help      F2 Save      F3 Calendar   F4 Find"),
        Line::from("F5 Search    F6 Replace   F7 Index      F8 Sync"),
        Line::from("F9 Closing   F10 Quit     F11 Reveal    F12 Lock"),
        Line::from("Ctrl+Shift+F or TOOLS -> Spellcheck Entry checks current page."),
        Line::from("FILE   Save, export, backup, restore, lock, quit"),
        Line::from("EDIT   Find/replace, closing thought, lines, stamps, metadata, reveal"),
        Line::from("SEARCH Vault search, recent queries, presets, cache status"),
        Line::from("GO     Calendar, index, recents, favorites, random, today"),
        Line::from("TOOLS  Spellcheck, sync, soundtrack, verify, review, dashboard, prompts"),
        Line::from("TOOLS  Insights Center, Today Brief, Week Compass, optional AI"),
        Line::from("HELP   Quick start, menu guide, updates, doctor, about"),
        Line::from("TOOLS  Optional AI Summary + AI Coach (Ctrl+Shift+A)"),
        Line::from("Calendar: YYYY-MM-DD jump, [ ] saved jump, < > months, N/P blank, T/0 today"),
        Line::from(
            "Index: type filter, / clear, 1-4 scopes, N/P blank, S sort, F favorite, C conflict",
        ),
        Line::from("Search: Tab fields, / query, T/W/M/Y/A ranges, Enter opens result"),
        Line::from("Search: Ctrl+G close, Ctrl+B pin, Ctrl+Shift+B preset, Ctrl+1..9 slot"),
        Line::from(
            "Old entries are deliberate: use Calendar, Index, or Search to browse archive dates.",
        ),
        Line::from("Footer keeps mode + status visible. Enter/Esc/F1 closes."),
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
            if date == Local::now().date_naive() {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            if date == picker.selected_date {
                style = style.add_modifier(Modifier::REVERSED);
            }
            spans.push(Span::styled(format!("{:>2} ", date.day()), style));
        }
        lines.push(Line::from(spans));
    }

    let saved_this_month = picker
        .entry_dates
        .iter()
        .filter(|date| date.year() == picker.month.year() && date.month() == picker.month.month())
        .count();
    let selected_state = if picker.has_entry(picker.selected_date) {
        "SAVED"
    } else {
        "BLANK"
    };
    let day_offset = (picker.selected_date - Local::now().date_naive()).num_days();
    lines.push(Line::from(format!(
        "Saved this month: {saved_this_month}  Total saved dates: {}",
        picker.entry_dates.len()
    )));
    lines.push(Line::from(format!(
        "Selected: {}  Status: {}  Delta from today: {:+} day(s)",
        picker.selected_date.format("%Y-%m-%d"),
        selected_state,
        day_offset
    )));
    lines.push(Line::from(
        "Archive flow is intentional: use Calendar/Index when opening older dates.",
    ));
    lines.push(Line::from("Bold = saved day  Underline = today"));
    lines.push(Line::from("Reverse = selected  [ ] saved day jump"));
    lines.push(Line::from(format!(
        "Jump: {}",
        if picker.jump_input.trim().is_empty() {
            "[type YYYY-MM-DD]".to_string()
        } else {
            picker.jump_input.clone()
        }
    )));
    lines.push(Line::from(
        "Arrows or H/J/K/L move  PgUp/PgDn month  < > entry months  N/P blank day",
    ));
    lines.push(Line::from(
        "Home/End month bounds  T/0/G today  / clear jump  Enter open  Esc close",
    ));
    frame.render_widget(Paragraph::new(lines).style(screen_style()), area);
}

fn draw_search_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &App,
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
        Line::from(format!(
            "Results: {}  Selected: {}  Active: {}  Range: {}",
            search.results.len(),
            if search.results.is_empty() {
                "0/0".to_string()
            } else {
                format!("{}/{}", search.selected + 1, search.results.len())
            },
            match search.active_field {
                SearchField::Query => "QUERY",
                SearchField::From => "FROM",
                SearchField::To => "TO",
                SearchField::Results => "RESULTS",
            },
            search.range_label(),
        )),
        Line::from(format!(
            "Query chars: {}  Pinned: {}  Presets: {}  Filters: {}",
            search.query_input.chars().count(),
            app.pinned_query_count(),
            app.search_preset_count(),
            usize::from(!search.from_input.trim().is_empty())
                + usize::from(!search.to_input.trim().is_empty())
        )),
        Line::from(format!(
            "Cache: {} (in-memory only, no plaintext index on disk)",
            if app.search_cache_ready() {
                "READY"
            } else {
                "BUILDING"
            }
        )),
        Line::from("Examples: launch plan | mood:7 | #work | closing thought"),
        Line::from("DATE         ENTRY NO  LOCATION  SNIPPET"),
        Line::from("----------------------------------------"),
    ];

    if search.results.is_empty() {
        let empty_state = if search.query_input.trim().is_empty() {
            "Type a query to search saved revisions."
        } else {
            "No matches yet. Enter reruns the search with the current filters."
        };
        lines.push(Line::from(empty_state));
    } else {
        let footer_rows =
            1 + if search.selected_result().is_some() {
                2
            } else {
                0
            } + 7
                + if search.error.is_some() { 1 } else { 0 };
        let visible_rows = area.height.saturating_sub((10 + footer_rows) as u16).max(1) as usize;
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
    if let Some(result) = search.selected_result() {
        lines.push(Line::from(format!(
            "Selected: {} / {}  {} entry {} line {} col {}",
            search.selected + 1,
            search.results.len(),
            result.date.format("%Y-%m-%d"),
            result.entry_number,
            result.row + 1,
            result.start_col + 1
        )));
        lines.push(Line::from(format!(
            "Match   : {}",
            truncate_to_width(&result.matched_text, area.width.saturating_sub(10) as usize)
        )));
    }
    lines.push(Line::from(
        "Tab fields  / query focus  Enter search/open  Ctrl+J focus results",
    ));
    lines.push(Line::from(
        "Ctrl+N/Ctrl+P move results  Up/Down/PgUp/PgDn also work",
    ));
    lines.push(Line::from("T today  W week  M month  Y year  A all"));
    lines.push(Line::from(
        "C clear filters  Ctrl+B pin query  Ctrl+Shift+B save preset",
    ));
    lines.push(Line::from(
        "Ctrl+I load pinned  Ctrl+U clear query  Ctrl+1..9 preset slots",
    ));
    lines.push(Line::from(
        "Ctrl+L clear all  Ctrl+R recall query  Home/End jump  Ctrl+G close",
    ));
    lines.push(Line::from(
        "Esc in results returns to query. Esc again closes search.",
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

fn draw_ai_coach_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    coach: &AiCoachOverlay,
) -> Option<(u16, u16)> {
    if area.width == 0 || area.height == 0 {
        return None;
    }

    let question = coach.current_prompt().unwrap_or("[no question loaded]");
    let mut lines = vec![
        Line::from("AI Coach Mode (optional, nostalgia-safe)"),
        Line::from(format!("Provider: {}", coach.provider)),
        Line::from(format!(
            "Question {}/{}",
            coach.current_idx.saturating_add(1),
            coach.prompts.len()
        )),
        Line::from(""),
        Line::from(truncate_to_width(
            question,
            area.width.saturating_sub(1) as usize,
        )),
        Line::from(""),
        Line::from(format!("> {}", coach.input)),
        Line::from("Enter next/apply  Ctrl+U clear  Esc cancel"),
    ];
    if let Some(error) = coach.error.as_deref() {
        lines.push(Line::from(error.to_string()));
    }

    frame.render_widget(Paragraph::new(lines).style(screen_style()), area);
    Some((
        (area.x + 2 + coach.input.chars().count() as u16).min(area.right().saturating_sub(1)),
        (area.y + 6).min(area.bottom().saturating_sub(1)),
    ))
}

fn draw_conflict_choice_overlay(frame: &mut Frame<'_>, area: Rect, conflict: &ConflictOverlay) {
    let head_a = conflict.conflict.heads.first();
    let head_b = conflict.conflict.heads.get(1).or(head_a);
    let more_heads = conflict.conflict.heads.len().saturating_sub(2);

    let mut lines = vec![
        Line::from(format!(
            "CONFLICT DETECTED FOR {}",
            conflict.conflict.date.format("%Y-%m-%d")
        )),
        Line::from(format!("Heads: {}", conflict.conflict.heads.len())),
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
        conflict_option_line("3) Accept A", conflict.selected == ConflictMode::AcceptA),
        Line::from("Accept A writes a merged head using A as the primary view."),
        conflict_option_line("4) Accept B", conflict.selected == ConflictMode::AcceptB),
        Line::from("Accept B writes a merged head using B as the primary view."),
        conflict_option_line("5) Merge", conflict.selected == ConflictMode::Merge),
        Line::from(format!(
            "Merge keeps all heads and writes a new revision.{}",
            if more_heads > 0 {
                format!(" +{more_heads} more head(s) will be merged.")
            } else {
                String::new()
            }
        )),
        Line::from("Enter select  Tab cycle  Left/Right or 1-5 choose  Esc close"),
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

fn draw_export_prompt_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    prompt: &ExportPrompt,
) -> Option<(u16, u16)> {
    let lines = vec![
        Line::from("Export the current editor contents to a plaintext file."),
        Line::from(format!("Format: {} (Tab toggles)", prompt.format.label())),
        Line::from(format!("Path  : {}", prompt.path_input)),
        Line::from("Warning: exports are plaintext. Unsaved edits are included."),
        Line::from("Enter write file  Esc cancel"),
        Line::from(prompt.error.clone().unwrap_or_default()),
    ];
    frame.render_widget(Paragraph::new(lines).style(screen_style()), area);
    Some((
        (area.x + 8 + prompt.path_input.chars().count() as u16).min(area.right().saturating_sub(1)),
        (area.y + 2).min(area.bottom().saturating_sub(1)),
    ))
}

fn draw_setting_prompt_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    prompt: &SettingPrompt,
) -> Option<(u16, u16)> {
    let lines = vec![
        Line::from(format!("{} ({})", prompt.field.label(), prompt.field.key())),
        Line::from(prompt.field.prompt()),
        Line::from(format!("> {}", prompt.input)),
        Line::from(prompt.field.help()),
        Line::from("Enter save  Esc cancel"),
        Line::from(prompt.error.clone().unwrap_or_default()),
    ];
    frame.render_widget(Paragraph::new(lines).style(screen_style()), area);
    Some((
        (area.x + 2 + prompt.input.chars().count() as u16).min(area.right().saturating_sub(1)),
        (area.y + 2).min(area.bottom().saturating_sub(1)),
    ))
}

fn draw_cloud_credential_prompt_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    prompt: &CloudCredentialPrompt,
) -> Option<(u16, u16)> {
    let lines = vec![
        credential_input_line(CloudCredentialField::AccessToken, prompt),
        credential_input_line(CloudCredentialField::RefreshToken, prompt),
        credential_input_line(CloudCredentialField::ClientId, prompt),
        credential_input_line(CloudCredentialField::ClientSecret, prompt),
        Line::from("Access token or refresh+client credentials are required."),
        Line::from("Saved to macOS Keychain only. Env vars still override matching fields."),
        Line::from("Tab next field  Enter save  Esc cancel"),
        Line::from(prompt.error.clone().unwrap_or_default()),
    ];
    frame.render_widget(Paragraph::new(lines).style(screen_style()), area);

    let (input, row_offset) = match prompt.active_field {
        CloudCredentialField::AccessToken => (&prompt.access_token_input, 0),
        CloudCredentialField::RefreshToken => (&prompt.refresh_token_input, 1),
        CloudCredentialField::ClientId => (&prompt.client_id_input, 2),
        CloudCredentialField::ClientSecret => (&prompt.client_secret_input, 3),
    };
    Some((
        (area.x + 14 + input.chars().count() as u16).min(area.right().saturating_sub(1)),
        (area.y + row_offset).min(area.bottom().saturating_sub(1)),
    ))
}

fn credential_input_line(
    field: CloudCredentialField,
    prompt: &CloudCredentialPrompt,
) -> Line<'static> {
    let label = field.label(prompt.provider);
    let value = prompt.masked_value(field);
    let mut style = screen_style();
    if prompt.active_field == field {
        style = style.add_modifier(Modifier::REVERSED);
    }
    Line::from(vec![
        Span::styled(format!("{label:<12}"), style.add_modifier(Modifier::BOLD)),
        Span::styled(format!(" {}", value), style),
    ])
}

fn draw_metadata_prompt_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    prompt: &MetadataPrompt,
) -> Option<(u16, u16)> {
    let lines = vec![
        metadata_input_line(
            "Tags   ",
            &prompt.tags_input,
            prompt.active_field == MetadataField::Tags,
        ),
        metadata_input_line(
            "People ",
            &prompt.people_input,
            prompt.active_field == MetadataField::People,
        ),
        metadata_input_line(
            "Project",
            &prompt.project_input,
            prompt.active_field == MetadataField::Project,
        ),
        metadata_input_line(
            "Mood   ",
            &prompt.mood_input,
            prompt.active_field == MetadataField::Mood,
        ),
        Line::from("Comma-separate tags and people. Mood accepts 0-9."),
        Line::from("Tab next field  Enter save  Esc cancel"),
        Line::from(prompt.error.clone().unwrap_or_default()),
    ];
    frame.render_widget(Paragraph::new(lines).style(screen_style()), area);

    let (input, row_offset) = match prompt.active_field {
        MetadataField::Tags => (&prompt.tags_input, 0),
        MetadataField::People => (&prompt.people_input, 1),
        MetadataField::Project => (&prompt.project_input, 2),
        MetadataField::Mood => (&prompt.mood_input, 3),
    };
    Some((
        (area.x + 10 + input.chars().count() as u16).min(area.right().saturating_sub(1)),
        (area.y + row_offset).min(area.bottom().saturating_sub(1)),
    ))
}

fn draw_restore_prompt_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    prompt: &RestorePrompt,
) -> Option<(u16, u16)> {
    if area.width == 0 || area.height == 0 {
        return None;
    }

    let mut lines = vec![
        Line::from("Restore an encrypted backup into a separate folder."),
        Line::from(if prompt.stage == RestoreStage::SelectBackup {
            "Stage 1/2: choose backup"
        } else {
            "Stage 2/2: set target path"
        }),
        Line::from(""),
    ];

    if prompt.backups.is_empty() {
        lines.push(Line::from("No backups available."));
    } else {
        let visible_rows = area.height.saturating_sub(8).max(1) as usize;
        let (start, end) = prompt.window(visible_rows.max(1));
        for (offset, backup) in prompt.backups[start..end].iter().enumerate() {
            let absolute_idx = start + offset;
            let style =
                if absolute_idx == prompt.selected && prompt.stage == RestoreStage::SelectBackup {
                    screen_style().add_modifier(Modifier::REVERSED)
                } else {
                    screen_style()
                };
            let file_name = backup
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("backup");
            let row = format!(
                "{}  {:>8}  {}",
                backup.created_at.format("%Y-%m-%d %H:%M"),
                human_bytes(backup.size_bytes),
                truncate_to_width(file_name, area.width.saturating_sub(29) as usize)
            );
            lines.push(Line::from(Span::styled(row, style)));
        }
    }

    lines.push(Line::from(""));
    let target_style = if prompt.stage == RestoreStage::TargetPath {
        screen_style().add_modifier(Modifier::REVERSED)
    } else {
        screen_style()
    };
    lines.push(Line::from(Span::styled(
        format!("> Target: {}", prompt.target_input),
        target_style,
    )));
    lines.push(Line::from(
        "Use a new or empty folder. Existing vaults stay untouched.",
    ));
    lines.push(Line::from(
        "Tab switch stage  Enter next/restore  Esc cancel",
    ));
    lines.push(Line::from(prompt.error.clone().unwrap_or_default()));
    frame.render_widget(Paragraph::new(lines).style(screen_style()), area);

    if prompt.stage == RestoreStage::TargetPath {
        Some((
            (area.x + 10 + prompt.target_input.chars().count() as u16)
                .min(area.right().saturating_sub(1)),
            area.bottom().saturating_sub(3),
        ))
    } else {
        None
    }
}

fn draw_index_overlay(frame: &mut Frame<'_>, area: Rect, index: &IndexState) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let total_favorites = index
        .all_items
        .iter()
        .filter(|entry| index.favorite_dates.contains(&entry.date))
        .count();
    let shown_favorites = index
        .items
        .iter()
        .filter(|entry| index.favorite_dates.contains(&entry.date))
        .count();
    let total_conflicts = index
        .all_items
        .iter()
        .filter(|entry| entry.has_conflict)
        .count();
    let shown_conflicts = index
        .items
        .iter()
        .filter(|entry| entry.has_conflict)
        .count();

    let mut lines = vec![
        Line::from(format!(
            "Saved entries: {} shown / {} total  Selected: {}",
            index.items.len(),
            index.all_items.len(),
            index
                .items
                .get(index.selected)
                .map(|entry| entry.date.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| "none".to_string())
        )),
        Line::from(format!(
            "Scope: {}  Filter: {}  Sort: {}  FavOnly: {}  ConfOnly: {}",
            index.scope_label(),
            if index.filter_input.trim().is_empty() {
                "[all]".to_string()
            } else {
                truncate_to_width(&index.filter_input, area.width.saturating_sub(36) as usize)
            },
            if index.sort_oldest_first {
                "OLDEST"
            } else {
                "NEWEST"
            },
            if index.favorites_only { "YES" } else { "NO" },
            if index.conflicts_only { "YES" } else { "NO" },
        )),
        Line::from(format!(
            "Favorites: {shown_favorites}/{total_favorites} shown  Conflicts: {shown_conflicts}/{total_conflicts} shown",
        )),
        Line::from("DATE         ENTRY NO  FLAGS     PREVIEW"),
        Line::from("----------------------------------------"),
    ];

    if index.items.is_empty() {
        lines.push(Line::from("No saved entries match the current filter."));
    } else {
        let visible_rows = area.height.saturating_sub(12).max(1) as usize;
        let (start, end) = index.window(visible_rows);
        let preview_width = area.width.saturating_sub(31) as usize;
        for (offset, entry) in index.items[start..end].iter().enumerate() {
            let absolute_idx = start + offset;
            let flags = index_row_flags(entry, &index.favorite_dates);
            let row = format!(
                "{:<10}  {:<8}  {:<8}  {}",
                entry.date.format("%Y-%m-%d"),
                entry.entry_number,
                flags,
                truncate_to_width(&entry.preview, preview_width)
            );
            let style = if absolute_idx == index.selected {
                screen_style().add_modifier(Modifier::REVERSED)
            } else {
                screen_style()
            };
            lines.push(Line::from(Span::styled(row, style)));
        }
        if let Some(entry) = index.items.get(index.selected) {
            lines.push(Line::from(""));
            lines.push(Line::from(truncate_to_width(
                &index_detail_summary(entry, &index.favorite_dates),
                area.width as usize,
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(
        "Type to filter  / clear filter  Backspace delete  S sort  * toggle favorite",
    ));
    lines.push(Line::from(
        "F favorites  C conflicts  1 all 2 last7 3 last30 4 ytd  T today",
    ));
    lines.push(Line::from(
        "Up/Down/PgUp/PgDn move  Home/End jump  [ ] month jump  N/P blank day",
    ));
    lines.push(Line::from("Enter open selected date  Esc close"));
    frame.render_widget(Paragraph::new(lines).style(screen_style()), area);
}

fn draw_sync_overlay(frame: &mut Frame<'_>, area: Rect, sync_status: &SyncStatusOverlay) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let mut lines = vec![
        Line::from(format!("Backend: {}", sync_status.backend_label)),
        Line::from(format!(
            "Target : {}",
            truncate_to_width(
                &sync_status.target_label,
                area.width.saturating_sub(9) as usize
            )
        )),
        Line::from(""),
    ];

    match &sync_status.phase {
        SyncPhase::Pending => {
            lines.push(Line::from("Preparing encrypted sync..."));
            lines.push(Line::from("Pull  Reconcile  Conflict Check  Push  Verify"));
        }
        SyncPhase::Running => {
            lines.push(Line::from("SYNCING ENCRYPTED BLOBS..."));
            lines.push(Line::from("Pull  Reconcile  Conflict Check  Push  Verify"));
        }
        SyncPhase::Complete {
            pulled,
            pushed,
            conflicts,
            integrity_ok,
            integrity_issue_count,
        } => {
            lines.push(Line::from("SYNC COMPLETE"));
            lines.push(Line::from(format!("Pulled   : {pulled}")));
            lines.push(Line::from(format!("Pushed   : {pushed}")));
            lines.push(Line::from(if conflicts.is_empty() {
                "Conflicts: none".to_string()
            } else {
                format!(
                    "Conflicts: {}",
                    conflicts
                        .iter()
                        .map(|date| date.format("%Y-%m-%d").to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }));
            lines.push(Line::from(if *integrity_ok {
                "Verify   : OK".to_string()
            } else {
                format!("Verify   : BROKEN ({integrity_issue_count})")
            }));
        }
        SyncPhase::Error { message } => {
            lines.push(Line::from("SYNC FAILED"));
            lines.push(Line::from(truncate_to_width(
                message,
                area.width.saturating_sub(1) as usize,
            )));
        }
    }

    if sync_status.draft_notice {
        lines.push(Line::from(""));
        lines.push(Line::from(
            "Note: unsaved edits were autosaved locally; only revisions sync.",
        ));
    }
    lines.push(Line::from(""));
    lines.push(Line::from("Enter/Esc close"));

    frame.render_widget(Paragraph::new(lines).style(screen_style()), area);
}

fn draw_info_overlay(frame: &mut Frame<'_>, area: Rect, info: &InfoOverlay) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let visible_rows = area.height.saturating_sub(2) as usize;
    let (start, end) = info.window(visible_rows);
    let mut lines = info.lines[start..end]
        .iter()
        .map(|line| Line::from(truncate_to_width(line, area.width as usize)))
        .collect::<Vec<_>>();
    lines.push(Line::from(""));
    lines.push(Line::from("Up/Down/PgUp/PgDn scroll  Enter/Esc close"));
    frame.render_widget(Paragraph::new(lines).style(screen_style()), area);
}

fn draw_picker_overlay(frame: &mut Frame<'_>, area: Rect, picker: &PickerOverlay) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let is_command_palette = picker.title == "Command Palette";
    let mut lines = vec![
        Line::from(format!(
            "{}: {}",
            if is_command_palette {
                "Action"
            } else {
                "Filter"
            },
            if picker.filter_input.trim().is_empty() {
                if is_command_palette {
                    "[save | next | old | search | sync | help]".to_string()
                } else {
                    "[all]".to_string()
                }
            } else {
                picker.filter_input.clone()
            }
        )),
        Line::from(""),
    ];

    let visible_rows = area.height.saturating_sub(5) as usize;
    let (filtered, start, end) = picker.window(visible_rows.max(1));
    if filtered.is_empty() {
        lines.push(Line::from(picker.empty_message.clone()));
    } else {
        for (offset, item_index) in filtered[start..end].iter().enumerate() {
            let absolute_idx = start + offset;
            if let Some(item) = picker.items.get(*item_index) {
                let style = if absolute_idx == picker.selected {
                    screen_style().add_modifier(Modifier::REVERSED)
                } else {
                    screen_style()
                };
                let row = join_left_right(
                    area.width as usize,
                    &item.title,
                    &truncate_to_width(&item.detail, area.width.saturating_sub(20) as usize),
                );
                lines.push(Line::from(Span::styled(row, style)));
            }
        }
    }

    lines.push(Line::from(""));
    if is_command_palette {
        lines.push(Line::from(
            "Type what you want: save / next / old / search / backup / help",
        ));
        lines.push(Line::from(
            "Up/Down move  Enter run  PgUp/PgDn page  Backspace clear  Esc close",
        ));
    } else {
        lines.push(Line::from("Type to filter  Up/Down move  Enter open"));
        lines.push(Line::from("PgUp/PgDn page  Backspace clear  Esc close"));
    }
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

fn metadata_input_line(label: &str, value: &str, active: bool) -> Line<'static> {
    let style = if active {
        screen_style().add_modifier(Modifier::REVERSED)
    } else {
        screen_style()
    };
    Line::from(Span::styled(format!("{label}: {value}"), style))
}

fn human_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    let bytes_f = bytes as f64;
    if bytes_f >= MIB {
        format!("{:.1} MiB", bytes_f / MIB)
    } else if bytes_f >= KIB {
        format!("{:.1} KiB", bytes_f / KIB)
    } else {
        format!("{bytes} B")
    }
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

struct MenuTab {
    id: MenuId,
    label: &'static str,
    x: u16,
}

fn menu_tabs(area: Rect) -> Vec<MenuTab> {
    let mut tabs = Vec::new();
    let mut x = area.x;
    for menu in MenuId::all() {
        let label = menu.title();
        let width = label.chars().count() as u16 + 2;
        if x + width > area.right() {
            break;
        }
        tabs.push(MenuTab {
            id: *menu,
            label,
            x,
        });
        x = x.saturating_add(width + 1);
    }
    tabs
}

fn menu_item_line(width: usize, item: &MenuItem, selected: bool) -> Line<'static> {
    let mut style = screen_style();
    if selected {
        style = style.add_modifier(Modifier::REVERSED);
    }
    if !item.enabled {
        style = style.add_modifier(Modifier::DIM);
    }
    let label = if item.enabled {
        item.label.clone()
    } else {
        format!("{} [UNAVAILABLE]", item.label)
    };
    let label_width = label.chars().count();
    let detail_width = item.detail.chars().count();
    let content = if label_width + detail_width + 1 > width {
        truncate_to_width(&format!("{} {}", label, item.detail), width)
    } else {
        format!(
            "{}{}{}",
            label,
            " ".repeat(width - label_width - detail_width),
            item.detail
        )
    };
    Line::from(Span::styled(content, style))
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

fn ruler_spans(cursor_col: usize, width: usize) -> Vec<Span<'static>> {
    let mut spans = Vec::with_capacity(width);
    for idx in 0..width {
        let column = idx + 1;
        let marker = if column % 10 == 0 {
            char::from(b'0' + ((column / 10) % 10) as u8)
        } else if column % 5 == 0 {
            '+'
        } else {
            '.'
        };
        let mut style = screen_style();
        if idx == cursor_col.min(width.saturating_sub(1)) {
            style = style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
        }
        spans.push(Span::styled(marker.to_string(), style));
    }
    spans
}

fn centered_line(width: usize, text: &str) -> Line<'static> {
    let text_width = text.chars().count();
    let left_padding = width.saturating_sub(text_width) / 2;
    Line::from(format!("{}{}", " ".repeat(left_padding), text))
}

fn overlay_title(overlay: &Overlay) -> String {
    match overlay {
        Overlay::SetupWizard(_) => " Setup ".to_string(),
        Overlay::UnlockPrompt { .. } => " Unlock ".to_string(),
        Overlay::Help => " Help (F1) ".to_string(),
        Overlay::DatePicker(_) => " Dates (F3) ".to_string(),
        Overlay::ConflictChoice(_) => " Conflict ".to_string(),
        Overlay::MergeDiff(_) => " Merge ".to_string(),
        Overlay::FindPrompt { .. } => " Find (F4) ".to_string(),
        Overlay::ClosingPrompt { .. } => " Closing Thought (F9) ".to_string(),
        Overlay::Search(_) => " Search (F5) ".to_string(),
        Overlay::AiCoach(_) => " AI Coach (Optional) ".to_string(),
        Overlay::ReplacePrompt(_) => " Replace (F6) ".to_string(),
        Overlay::ReplaceConfirm(_) => " Replace (F6) ".to_string(),
        Overlay::ExportPrompt(prompt) => format!(" Export {} ", prompt.format.label()),
        Overlay::SettingPrompt(_) => " Setup ".to_string(),
        Overlay::CloudCredentialPrompt(prompt) => {
            format!(" {} Keychain ", prompt.provider.label())
        }
        Overlay::MetadataPrompt(_) => " Metadata ".to_string(),
        Overlay::Index(_) => " Index (F7) ".to_string(),
        Overlay::SyncStatus(_) => " Sync (F8) ".to_string(),
        Overlay::Info(info) => format!(" {} ", info.title),
        Overlay::Picker(picker) => format!(" {} ", picker.title),
        Overlay::RestorePrompt(_) => " Restore ".to_string(),
        Overlay::RecoverDraft { .. } => " Recovery ".to_string(),
        Overlay::PruneConfirm { .. } => " Prune Backups ".to_string(),
        Overlay::QuitConfirm => " Quit (F10) ".to_string(),
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

fn canonical_screen_rect(area: Rect) -> Rect {
    let width = DOS_WIDTH.min(area.width);
    let height = DOS_HEIGHT.min(area.height);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}

fn workspace_rect(area: Rect) -> Rect {
    if area.width >= DOS_WIDTH && area.height >= DOS_HEIGHT {
        canonical_screen_rect(area)
    } else {
        area
    }
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
    use super::{canonical_screen_rect, centered_line, popup_rect};
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

    #[test]
    fn canonical_screen_rect_caps_large_terminal_to_dos_size() {
        let screen = canonical_screen_rect(Rect::new(0, 0, 120, 40));
        assert_eq!(screen.width, 80);
        assert_eq!(screen.height, 25);
        assert_eq!(screen.x, 20);
        assert_eq!(screen.y, 7);
    }

    #[test]
    fn canonical_screen_rect_uses_full_area_when_exactly_dos_size() {
        let screen = canonical_screen_rect(Rect::new(0, 0, 80, 25));
        assert_eq!(screen, Rect::new(0, 0, 80, 25));
    }
}
