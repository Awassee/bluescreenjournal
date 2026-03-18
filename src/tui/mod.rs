pub mod app;
pub mod buffer;
pub mod calendar;

use crate::tui::{
    app::{
        App, ConflictMode, ConflictOverlay, DatePicker, ExportPrompt, IndexState, InfoOverlay,
        MenuId, MenuItem, MetadataField, MetadataPrompt, Overlay, PickerOverlay, ReplacePrompt,
        ReplaceStage, RestorePrompt, RestoreStage, SearchField, SearchOverlay, SettingPrompt,
        SetupStep, SetupWizard, SyncPhase, SyncStatusOverlay, index_detail_summary,
        index_row_flags,
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
    let poll_timeout = Duration::from_millis(80);

    while !app.should_quit() {
        terminal.draw(|frame| draw(frame, &app))?;
        let area = terminal.size()?;
        let screen = workspace_rect(Rect::new(0, 0, area.width, area.height));
        let viewport_height = app.editor_viewport_height(screen.height.saturating_sub(3) as usize);

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
        "[{}]  PERSONAL JOURNAL{}{}  TIME {}  ENTRY NO. {}",
        app.header_entry_focus_label(),
        if compact_mode { " [COMPACT]" } else { "" },
        if app.favorite_marker().is_empty() {
            ""
        } else {
            " *"
        },
        app.header_time_label(),
        app.entry_number_label()
    );
    let mut right_parts = vec![
        app.lock_status_label().to_string(),
        app.integrity_status_label(),
        app.word_goal_status_label(),
        app.session_status_label(),
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
        "LEFT/RIGHT MENU  UP/DOWN ITEM  ENTER SELECT  ESC CLOSE"
    } else if area.width >= 130 {
        "ESC MENUS  ALT+F/E/S/G/T/U/H MENU  ALT+RIGHT NEXT DAY  ALT+N NEW ENTRY"
    } else if area.width >= 104 {
        "ESC MENUS  ALT+F/E/S/G/T/U/H MENU  ALT+RIGHT NEXT DAY  ALT+N NEW"
    } else {
        "ESC MENUS  ALT+F/E/S/G/T/U/H MENU"
    };
    let left_width = spans
        .iter()
        .map(|span| span.content.chars().count())
        .sum::<usize>();
    let hint_width = hint.chars().count();
    if area.width as usize > left_width + hint_width + 1 {
        spans.push(Span::raw(
            " ".repeat(area.width as usize - left_width - hint_width),
        ));
        spans.push(Span::styled(
            hint.to_string(),
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
            .map(|line| centered_line(editor_area.width as usize, line))
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

fn draw_footer(frame: &mut Frame<'_>, app: &App, area: Rect, compact_mode: bool) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let context = format!(
        "{} | {} | {} | {}",
        app.footer_mode_label(),
        app.save_status_label(),
        app.footer_context_label(),
        app.footer_stats_label(),
    );
    let status = app.status_text().unwrap_or("");
    let left = if status.is_empty() {
        context
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
        return "Ctrl+K Commands".to_string();
    }

    if app.menu().is_some() {
        if width >= 96 {
            return "Menu: Left/Right switch | Up/Down move | Enter select | Esc close".to_string();
        }
        return "Menu: Arrows move | Enter select | Esc close".to_string();
    }

    if app.overlay().is_some() {
        if width >= 90 {
            return "Esc close prompt | Enter confirm | F1 keys | Ctrl+K commands".to_string();
        }
        return "Esc close | Enter confirm | F1 keys".to_string();
    }

    let legend = if width >= 130 {
        "Esc Menus | Alt+F/E/S/G/T/U/H menu | Alt+Right next day | Alt+N next new entry | F2 Save | F10 Quit".to_string()
    } else if width >= 108 {
        "Esc Menus | Alt+F/E/S/G/T/U/H | Alt+Right next day | Alt+N new | F2 Save | F10 Quit"
            .to_string()
    } else if width >= 90 {
        "Esc Menus | Alt+F/E/S/G/T/U/H | F1 Help | F2 Save | F10 Quit".to_string()
    } else {
        "Esc Menus | Alt+Right day | Alt+N new | F2 Save | F10 Quit".to_string()
    };

    if compact_mode && width >= 100 {
        format!("{legend} | Compact layout")
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
        centered_line(area.width as usize, "Resize, then press Esc for menus."),
    ];
    frame.render_widget(Paragraph::new(lines).style(screen_style()), area);
}

fn draw_overlay(frame: &mut Frame<'_>, app: &App, body_area: Rect) -> Option<(u16, u16)> {
    let overlay = app.overlay()?;
    let rect = match overlay {
        Overlay::SetupWizard(_) => popup_rect(body_area, 76, 10),
        Overlay::UnlockPrompt { .. } => popup_rect(body_area, 68, 8),
        Overlay::Help => popup_rect(body_area, 72, 20),
        Overlay::DatePicker(_) => popup_rect(body_area, 38, 13),
        Overlay::FindPrompt { .. } => popup_rect(body_area, 54, 6),
        Overlay::ClosingPrompt { .. } => popup_rect(body_area, 58, 5),
        Overlay::ConflictChoice(_) => popup_rect(body_area, 72, 15),
        Overlay::MergeDiff(_) => popup_rect(body_area, 92, 18),
        Overlay::Search(_) => popup_rect(body_area, 84, 16),
        Overlay::ReplacePrompt(_) => popup_rect(body_area, 58, 8),
        Overlay::ReplaceConfirm(_) => popup_rect(body_area, 62, 8),
        Overlay::ExportPrompt(_) => popup_rect(body_area, 72, 9),
        Overlay::SettingPrompt(_) => popup_rect(body_area, 70, 8),
        Overlay::MetadataPrompt(_) => popup_rect(body_area, 72, 9),
        Overlay::Index(_) => popup_rect(body_area, 78, 16),
        Overlay::SyncStatus(_) => popup_rect(body_area, 76, 14),
        Overlay::Info(_) => popup_rect(body_area, 76, 16),
        Overlay::Picker(_) => popup_rect(body_area, 76, 14),
        Overlay::RestorePrompt(_) => popup_rect(body_area, 76, 12),
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
        Overlay::ExportPrompt(prompt) => draw_export_prompt_overlay(frame, inner, prompt),
        Overlay::SettingPrompt(prompt) => draw_setting_prompt_overlay(frame, inner, prompt),
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
        Line::from("Enter unlock  Esc keep locked"),
        Line::from(error.clone().unwrap_or_default()),
    ];
    frame.render_widget(Paragraph::new(lines).style(screen_style()), area);
    Some((
        (area.x + 2 + masked.chars().count() as u16).min(area.right().saturating_sub(1)),
        (area.y + 2).min(area.bottom().saturating_sub(1)),
    ))
}

fn draw_help_overlay(frame: &mut Frame<'_>, area: Rect) {
    let lines = vec![
        Line::from("Classic 80x25 workspace. Type immediately when no menu or prompt is open."),
        Line::from("Esc menus. Alt+F/E/S/G/T/U/H menu. Ctrl+K = commands."),
        Line::from("Alt+Right next day. Alt+N next blank new entry."),
        Line::from("F1 Help      F2 Save      F3 Dates      F4 Find"),
        Line::from("F5 Search    F6 Replace   F7 Index      F8 Sync"),
        Line::from("F9 Closing   F10 Quit     F11 Reveal    F12 Lock"),
        Line::from("Ctrl+S Save  Ctrl+F Find"),
        Line::from("Older entries: use F7 Index or F3 Calendar."),
        Line::from("FILE   Save, export, backup, restore, lock, quit"),
        Line::from("EDIT   Lines, stamps, metadata, favorite, reveal, typewriter"),
        Line::from("SEARCH Vault search, recent queries, presets, cache status"),
        Line::from("GO     Calendar, index, recents, favorites, random, today"),
        Line::from("TOOLS  Sync, verify, review, dashboard, prompts, doctor"),
        Line::from("Calendar: type YYYY-MM-DD, [ ] saved-day jump, < > entry months, T today"),
        Line::from("Index: type filter, S sort, Shift+F favorites, Shift+C conflicts"),
        Line::from("Search: Tab fields, Enter search/open, T/W/M/Y/A presets, Ctrl+R recall query"),
        Line::from("Header shows lock, verify, goal, session, and save state."),
        Line::from("Footer shows mode, context, stats, and status. Enter/Esc/F1 closes."),
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
    lines.push(Line::from("Arrows move  PgUp/PgDn month  < > entry months"));
    lines.push(Line::from(
        "Home/End month bounds  T today  Enter open  Esc close",
    ));
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
            } + 3
                + if search.error.is_some() { 1 } else { 0 };
        let visible_rows = area.height.saturating_sub((7 + footer_rows) as u16).max(1) as usize;
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
        "Tab fields  Enter search/open  Ctrl+N/Ctrl+P move",
    ));
    lines.push(Line::from("T today  W week  M month  Y year  A all"));
    lines.push(Line::from(
        "C clear filters  Ctrl+L clear all  Ctrl+R recall query",
    ));
    lines.push(Line::from(
        "Up/Down/PgUp/PgDn move results  Home/End jump  Esc close",
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

    let mut lines = vec![
        Line::from(format!(
            "Saved entries: {}  Selected: {}",
            index.items.len(),
            index
                .items
                .get(index.selected)
                .map(|entry| entry.date.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| "none".to_string())
        )),
        Line::from(format!(
            "Filter: {}  Sort: {}  FavOnly: {}  ConfOnly: {}",
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
        Line::from("DATE         ENTRY NO  FLAGS     PREVIEW"),
        Line::from("----------------------------------------"),
    ];

    if index.items.is_empty() {
        lines.push(Line::from("No saved entries match the current filter."));
    } else {
        let visible_rows = area.height.saturating_sub(11).max(1) as usize;
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
        "Type to filter  Backspace clear  S sort  T today",
    ));
    lines.push(Line::from("Shift+F favorites  Shift+C conflicts"));
    lines.push(Line::from("Up/Down/PgUp/PgDn move  Home/End jump"));
    lines.push(Line::from("Enter open  Esc close"));
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

    let mut lines = vec![
        Line::from(format!(
            "Filter: {}",
            if picker.filter_input.trim().is_empty() {
                "[all]".to_string()
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
    lines.push(Line::from("Type to filter  Up/Down move  Enter open"));
    lines.push(Line::from("PgUp/PgDn page  Backspace clear  Esc close"));
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
    let label_width = item.label.chars().count();
    let detail_width = item.detail.chars().count();
    let content = if label_width + detail_width + 1 > width {
        truncate_to_width(&format!("{} {}", item.label, item.detail), width)
    } else {
        format!(
            "{}{}{}",
            item.label,
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
        Overlay::Help => " Help ".to_string(),
        Overlay::DatePicker(_) => " Dates ".to_string(),
        Overlay::ConflictChoice(_) => " Conflict ".to_string(),
        Overlay::MergeDiff(_) => " Merge ".to_string(),
        Overlay::FindPrompt { .. } => " Find ".to_string(),
        Overlay::ClosingPrompt { .. } => " Closing ".to_string(),
        Overlay::Search(_) => " Search ".to_string(),
        Overlay::ReplacePrompt(_) => " Replace ".to_string(),
        Overlay::ReplaceConfirm(_) => " Replace ".to_string(),
        Overlay::ExportPrompt(prompt) => format!(" Export {} ", prompt.format.label()),
        Overlay::SettingPrompt(_) => " Setup ".to_string(),
        Overlay::MetadataPrompt(_) => " Metadata ".to_string(),
        Overlay::Index(_) => " Index ".to_string(),
        Overlay::SyncStatus(_) => " Sync ".to_string(),
        Overlay::Info(info) => format!(" {} ", info.title),
        Overlay::Picker(picker) => format!(" {} ", picker.title),
        Overlay::RestorePrompt(_) => " Restore ".to_string(),
        Overlay::RecoverDraft { .. } => " Recovery ".to_string(),
        Overlay::QuitConfirm => " Quit ".to_string(),
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
