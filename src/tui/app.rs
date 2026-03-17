use crate::{
    config::{AppConfig, default_vault_path},
    search::{SearchIndex, SearchQuery, SearchResult},
    tui::{
        buffer::{MatchPos, TextBuffer},
        calendar,
    },
    vault::{self, IndexEntry, UnlockedVault},
};
use chrono::{DateTime, Duration as ChronoDuration, Local, NaiveDate};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use secrecy::SecretString;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use zeroize::Zeroize;

const STATUS_DURATION: Duration = Duration::from_millis(1600);
const AUTOSAVE_INTERVAL: Duration = Duration::from_millis(2500);
const INDEX_PREVIEW_CHARS: usize = 54;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Overlay {
    SetupWizard(SetupWizard),
    UnlockPrompt {
        input: String,
        error: Option<String>,
    },
    Help,
    DatePicker(DatePicker),
    FindPrompt {
        input: String,
        error: Option<String>,
    },
    ConflictChoice(ConflictOverlay),
    MergeDiff(vault::ConflictState),
    Search(SearchOverlay),
    ReplacePrompt(ReplacePrompt),
    ReplaceConfirm(ReplaceConfirm),
    Index(IndexState),
    RecoverDraft {
        draft_text: String,
    },
    QuitConfirm,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetupStep {
    VaultPath,
    Passphrase,
    ConfirmPassphrase,
    EpochDate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupWizard {
    pub step: SetupStep,
    pub path_input: String,
    pub passphrase_input: String,
    pub confirm_input: String,
    pub epoch_input: String,
    pub error: Option<String>,
}

impl SetupWizard {
    fn new(default_path: &Path) -> Self {
        Self {
            step: SetupStep::VaultPath,
            path_input: default_path.display().to_string(),
            passphrase_input: String::new(),
            confirm_input: String::new(),
            epoch_input: String::new(),
            error: None,
        }
    }

    pub fn title(&self) -> &'static str {
        match self.step {
            SetupStep::VaultPath => "Setup: Vault Path",
            SetupStep::Passphrase => "Setup: Passphrase",
            SetupStep::ConfirmPassphrase => "Setup: Confirm Passphrase",
            SetupStep::EpochDate => "Setup: Entry Number Epoch",
        }
    }

    pub fn prompt(&self) -> &'static str {
        match self.step {
            SetupStep::VaultPath => "Vault path:",
            SetupStep::Passphrase => "Set passphrase:",
            SetupStep::ConfirmPassphrase => "Confirm passphrase:",
            SetupStep::EpochDate => "Epoch date (YYYY-MM-DD, blank = today):",
        }
    }

    fn current_input_mut(&mut self) -> &mut String {
        match self.step {
            SetupStep::VaultPath => &mut self.path_input,
            SetupStep::Passphrase => &mut self.passphrase_input,
            SetupStep::ConfirmPassphrase => &mut self.confirm_input,
            SetupStep::EpochDate => &mut self.epoch_input,
        }
    }

    pub fn display_input(&self) -> String {
        match self.step {
            SetupStep::Passphrase => "*".repeat(self.passphrase_input.chars().count()),
            SetupStep::ConfirmPassphrase => "*".repeat(self.confirm_input.chars().count()),
            SetupStep::VaultPath => self.path_input.clone(),
            SetupStep::EpochDate => self.epoch_input.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DatePicker {
    pub month: NaiveDate,
    pub selected_date: NaiveDate,
    pub entry_dates: BTreeSet<NaiveDate>,
}

impl DatePicker {
    pub fn new(selected_date: NaiveDate, entry_dates: BTreeSet<NaiveDate>) -> Self {
        Self {
            month: calendar::month_start(selected_date),
            selected_date,
            entry_dates,
        }
    }

    pub fn month_label(&self) -> String {
        self.month.format("%B %Y").to_string()
    }

    pub fn grid(&self) -> Vec<Vec<NaiveDate>> {
        calendar::month_grid(self.month)
    }

    pub fn has_entry(&self, date: NaiveDate) -> bool {
        self.entry_dates.contains(&date)
    }

    fn move_selection_by_days(&mut self, days: i64) {
        self.selected_date += ChronoDuration::days(days);
        self.month = calendar::month_start(self.selected_date);
    }

    fn shift_month(&mut self, months: i32) {
        self.selected_date = calendar::shift_date_by_months(self.selected_date, months);
        self.month = calendar::month_start(self.selected_date);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplaceStage {
    Find,
    Replace,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplacePrompt {
    pub stage: ReplaceStage,
    pub find_input: String,
    pub replace_input: String,
    pub error: Option<String>,
}

impl ReplacePrompt {
    fn new() -> Self {
        Self {
            stage: ReplaceStage::Find,
            find_input: String::new(),
            replace_input: String::new(),
            error: None,
        }
    }

    fn active_input_mut(&mut self) -> &mut String {
        match self.stage {
            ReplaceStage::Find => &mut self.find_input,
            ReplaceStage::Replace => &mut self.replace_input,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplaceConfirm {
    pub find_text: String,
    pub replace_text: String,
    pub matches: Vec<MatchPos>,
    pub current_idx: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictMode {
    ViewA,
    ViewB,
    Merge,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictOverlay {
    pub conflict: vault::ConflictState,
    pub selected: ConflictMode,
}

impl ConflictOverlay {
    fn new(conflict: vault::ConflictState) -> Self {
        Self {
            conflict,
            selected: ConflictMode::ViewA,
        }
    }

    fn cycle(&mut self) {
        self.selected = match self.selected {
            ConflictMode::ViewA => ConflictMode::ViewB,
            ConflictMode::ViewB => ConflictMode::Merge,
            ConflictMode::Merge => ConflictMode::ViewA,
        };
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchField {
    Query,
    From,
    To,
    Results,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchOverlay {
    pub query_input: String,
    pub from_input: String,
    pub to_input: String,
    pub active_field: SearchField,
    pub results: Vec<SearchResult>,
    pub selected: usize,
    pub error: Option<String>,
}

impl SearchOverlay {
    fn new(initial_query: Option<String>) -> Self {
        Self {
            query_input: initial_query.unwrap_or_default(),
            from_input: String::new(),
            to_input: String::new(),
            active_field: SearchField::Query,
            results: Vec::new(),
            selected: 0,
            error: None,
        }
    }

    pub fn window(&self, max_rows: usize) -> (usize, usize) {
        if self.results.is_empty() || max_rows == 0 {
            return (0, 0);
        }
        let max_rows = max_rows.max(1);
        let mut start = self.selected.saturating_sub(max_rows / 2);
        let max_start = self.results.len().saturating_sub(max_rows);
        if start > max_start {
            start = max_start;
        }
        let end = (start + max_rows).min(self.results.len());
        (start, end)
    }

    fn active_input_mut(&mut self) -> Option<&mut String> {
        match self.active_field {
            SearchField::Query => Some(&mut self.query_input),
            SearchField::From => Some(&mut self.from_input),
            SearchField::To => Some(&mut self.to_input),
            SearchField::Results => None,
        }
    }

    fn clear_results(&mut self) {
        self.results.clear();
        self.selected = 0;
        if self.active_field == SearchField::Results {
            self.active_field = SearchField::Query;
        }
    }

    fn cycle_field(&mut self) {
        self.active_field = match self.active_field {
            SearchField::Query => SearchField::From,
            SearchField::From => SearchField::To,
            SearchField::To if self.results.is_empty() => SearchField::Query,
            SearchField::To => SearchField::Results,
            SearchField::Results => SearchField::Query,
        };
    }

    fn move_selection(&mut self, delta: isize) {
        if self.results.is_empty() {
            return;
        }
        let next = self.selected as isize + delta;
        self.selected = next.clamp(0, self.results.len() as isize - 1) as usize;
    }

    fn page_up(&mut self, amount: usize) {
        self.move_selection(-(amount.max(1) as isize));
    }

    fn page_down(&mut self, amount: usize) {
        self.move_selection(amount.max(1) as isize);
    }

    fn selected_result(&self) -> Option<&SearchResult> {
        self.results.get(self.selected)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexState {
    pub items: Vec<IndexEntry>,
    pub selected: usize,
}

impl IndexState {
    fn new(items: Vec<IndexEntry>, selected_date: NaiveDate) -> Self {
        let selected = items
            .iter()
            .position(|entry| entry.date == selected_date)
            .unwrap_or(0);
        Self { items, selected }
    }

    pub fn window(&self, max_rows: usize) -> (usize, usize) {
        if self.items.is_empty() || max_rows == 0 {
            return (0, 0);
        }
        let max_rows = max_rows.max(1);
        let mut start = self.selected.saturating_sub(max_rows / 2);
        let max_start = self.items.len().saturating_sub(max_rows);
        if start > max_start {
            start = max_start;
        }
        let end = (start + max_rows).min(self.items.len());
        (start, end)
    }

    fn move_selection(&mut self, delta: isize) {
        if self.items.is_empty() {
            return;
        }
        let next = self.selected as isize + delta;
        self.selected = next.clamp(0, self.items.len() as isize - 1) as usize;
    }

    fn page_up(&mut self, amount: usize) {
        self.move_selection(-(amount.max(1) as isize));
    }

    fn page_down(&mut self, amount: usize) {
        self.move_selection(amount.max(1) as isize);
    }

    fn selected_date(&self) -> Option<NaiveDate> {
        self.items.get(self.selected).map(|entry| entry.date)
    }
}

#[derive(Clone, Debug)]
struct StatusMessage {
    text: String,
    expires_at: Instant,
}

#[derive(Clone, Debug)]
struct SearchJump {
    match_text: String,
    row: usize,
    start_col: usize,
}

#[derive(Clone, Debug)]
struct MergeContext {
    primary_hash: String,
    merged_hashes: Vec<String>,
}

#[derive(Clone, Copy, Debug)]
enum SaveKind {
    Saved,
    Autosaved,
}

pub struct App {
    buffer: TextBuffer,
    selected_date: NaiveDate,
    scroll_row: usize,
    overlay: Option<Overlay>,
    status_flash: Option<StatusMessage>,
    find_query: Option<String>,
    find_matches: Vec<MatchPos>,
    current_match_idx: usize,
    should_quit: bool,
    vault_path: PathBuf,
    vault: Option<UnlockedVault>,
    dirty: bool,
    last_save_kind: Option<SaveKind>,
    last_save_time: Option<DateTime<Local>>,
    draft_recovered: bool,
    search_index: Option<SearchIndex>,
    pending_search_jump: Option<SearchJump>,
    pending_conflict: Option<vault::ConflictState>,
    merge_context: Option<MergeContext>,
    last_viewport_height: usize,
    last_autosave_check: Instant,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        Self::with_initial_date(None)
    }

    pub fn with_initial_date(initial_date: Option<NaiveDate>) -> Self {
        let config = AppConfig::load_or_default();
        let vault_path = if config.vault_path.as_os_str().is_empty() {
            default_vault_path()
        } else {
            config.vault_path.clone()
        };
        let overlay = if vault::vault_exists(&vault_path) {
            Some(Overlay::UnlockPrompt {
                input: String::new(),
                error: None,
            })
        } else {
            Some(Overlay::SetupWizard(SetupWizard::new(&vault_path)))
        };

        Self {
            buffer: TextBuffer::new(),
            selected_date: initial_date.unwrap_or_else(|| Local::now().date_naive()),
            scroll_row: 0,
            overlay,
            status_flash: None,
            find_query: None,
            find_matches: Vec::new(),
            current_match_idx: 0,
            should_quit: false,
            vault_path,
            vault: None,
            dirty: false,
            last_save_kind: None,
            last_save_time: None,
            draft_recovered: false,
            search_index: None,
            pending_search_jump: None,
            pending_conflict: None,
            merge_context: None,
            last_viewport_height: 23,
            last_autosave_check: Instant::now(),
        }
    }

    pub fn buffer(&self) -> &TextBuffer {
        &self.buffer
    }

    pub fn scroll_row(&self) -> usize {
        self.scroll_row
    }

    pub fn overlay(&self) -> Option<&Overlay> {
        self.overlay.as_ref()
    }

    pub fn status_text(&self) -> Option<&str> {
        self.status_flash
            .as_ref()
            .map(|status| status.text.as_str())
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn find_matches(&self) -> &[MatchPos] {
        &self.find_matches
    }

    pub fn current_match(&self) -> Option<&MatchPos> {
        self.find_matches.get(self.current_match_idx)
    }

    pub fn header_date_time_label(&self) -> String {
        format!(
            "{} {}",
            self.selected_date.format("%Y-%m-%d"),
            Local::now().format("%H:%M")
        )
    }

    pub fn entry_number_label(&self) -> String {
        let Some(vault) = &self.vault else {
            return "-------".to_string();
        };
        let Ok(epoch) = vault.metadata().epoch_date() else {
            return "-------".to_string();
        };
        vault::compute_entry_number(epoch, self.selected_date)
    }

    pub fn lock_status_label(&self) -> &'static str {
        if self.vault.is_some() {
            "UNLOCKED"
        } else {
            "LOCKED"
        }
    }

    pub fn save_status_label(&self) -> String {
        match (self.last_save_kind, self.last_save_time) {
            (Some(SaveKind::Saved), Some(time)) => format!("SAVED {}", time.format("%H:%M:%S")),
            (Some(SaveKind::Autosaved), Some(time)) => {
                format!("AUTOSAVED {}", time.format("%H:%M:%S"))
            }
            _ => "NOT SAVED".to_string(),
        }
    }

    pub fn draft_recovered_label(&self) -> &'static str {
        if self.draft_recovered {
            "DRAFT RECOVERED"
        } else {
            ""
        }
    }

    pub fn tick(&mut self) {
        if let Some(status) = &self.status_flash
            && Instant::now() >= status.expires_at
        {
            self.status_flash = None;
        }

        if self.dirty
            && self.vault.is_some()
            && self.last_autosave_check.elapsed() >= AUTOSAVE_INTERVAL
            && !matches!(
                self.overlay,
                Some(Overlay::SetupWizard(_)) | Some(Overlay::UnlockPrompt { .. })
            )
        {
            self.autosave_current_date();
            self.last_autosave_check = Instant::now();
        }
    }

    pub fn handle_event(&mut self, event: Event, viewport_height: usize) {
        self.last_viewport_height = viewport_height.max(1);
        match event {
            Event::Key(key) => self.handle_key(key, viewport_height),
            Event::Resize(_, _) => self.ensure_cursor_visible(viewport_height),
            _ => {}
        }
    }

    fn handle_key(&mut self, key: KeyEvent, viewport_height: usize) {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return;
        }

        if self.overlay.is_some() {
            self.handle_overlay_key(key, viewport_height);
            return;
        }

        if key.code == KeyCode::F(2) || Self::is_ctrl_char(&key, 's') {
            self.save_current_date();
            return;
        }
        if key.code == KeyCode::F(4) || Self::is_ctrl_char(&key, 'f') {
            self.overlay = Some(Overlay::FindPrompt {
                input: self.find_query.clone().unwrap_or_default(),
                error: None,
            });
            return;
        }

        let mut mutated = false;
        match key.code {
            KeyCode::F(1) => self.overlay = Some(Overlay::Help),
            KeyCode::F(3) => self.open_date_picker(),
            KeyCode::F(5) => self.open_search_overlay(),
            KeyCode::F(6) => self.overlay = Some(Overlay::ReplacePrompt(ReplacePrompt::new())),
            KeyCode::F(7) => self.open_index_overlay(),
            KeyCode::F(10) => self.overlay = Some(Overlay::QuitConfirm),
            KeyCode::Left => self.buffer.move_left(),
            KeyCode::Right => self.buffer.move_right(),
            KeyCode::Up => self.buffer.move_up(),
            KeyCode::Down => self.buffer.move_down(),
            KeyCode::PageUp => self.buffer.page_up(viewport_height),
            KeyCode::PageDown => self.buffer.page_down(viewport_height),
            KeyCode::Home => self.buffer.move_to_line_start(),
            KeyCode::End => self.buffer.move_to_line_end(),
            KeyCode::Backspace => {
                self.buffer.backspace();
                mutated = true;
            }
            KeyCode::Delete => {
                self.buffer.delete();
                mutated = true;
            }
            KeyCode::Enter => {
                self.buffer.insert_newline();
                mutated = true;
            }
            KeyCode::Tab => {
                self.buffer.insert_char('\t');
                mutated = true;
            }
            KeyCode::Char(ch) if Self::is_text_input_key(&key) => {
                self.buffer.insert_char(ch);
                mutated = true;
            }
            _ => {}
        }

        if mutated {
            self.dirty = true;
            self.refresh_find_matches();
        }
        self.ensure_cursor_visible(viewport_height);
    }

    fn handle_overlay_key(&mut self, key: KeyEvent, viewport_height: usize) {
        let Some(mut overlay) = self.overlay.take() else {
            return;
        };
        let mut keep_overlay = true;

        match &mut overlay {
            Overlay::SetupWizard(wizard) => match key.code {
                KeyCode::Esc => {
                    self.should_quit = true;
                    keep_overlay = false;
                }
                KeyCode::Backspace => {
                    wizard.current_input_mut().pop();
                    wizard.error = None;
                }
                KeyCode::Enter => {
                    if self.advance_setup_wizard(wizard) {
                        keep_overlay = false;
                    }
                }
                KeyCode::Char(ch) if Self::is_text_input_key(&key) => {
                    wizard.current_input_mut().push(ch);
                    wizard.error = None;
                }
                _ => {}
            },
            Overlay::UnlockPrompt { input, error } => match key.code {
                KeyCode::Esc => {
                    self.should_quit = true;
                    keep_overlay = false;
                }
                KeyCode::Backspace => {
                    input.pop();
                    *error = None;
                }
                KeyCode::Enter => {
                    if self.try_unlock(input, error) {
                        keep_overlay = false;
                    }
                }
                KeyCode::Char(ch) if Self::is_text_input_key(&key) => {
                    input.push(ch);
                    *error = None;
                }
                _ => {}
            },
            Overlay::Help => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Enter | KeyCode::F(1)) {
                    keep_overlay = false;
                }
            }
            Overlay::DatePicker(picker) => match key.code {
                KeyCode::Esc | KeyCode::F(3) => keep_overlay = false,
                KeyCode::Left => picker.move_selection_by_days(-1),
                KeyCode::Right => picker.move_selection_by_days(1),
                KeyCode::Up => picker.move_selection_by_days(-7),
                KeyCode::Down => picker.move_selection_by_days(7),
                KeyCode::PageUp => picker.shift_month(-1),
                KeyCode::PageDown => picker.shift_month(1),
                KeyCode::Enter => {
                    let date = picker.selected_date;
                    self.open_date(date);
                    self.flash_status("DATE SET.");
                    keep_overlay = false;
                }
                _ => {}
            },
            Overlay::ConflictChoice(conflict) => match key.code {
                KeyCode::Esc => keep_overlay = false,
                KeyCode::Left | KeyCode::Char('1') => conflict.selected = ConflictMode::ViewA,
                KeyCode::Right | KeyCode::Char('2') => conflict.selected = ConflictMode::ViewB,
                KeyCode::Char('3') | KeyCode::Char('m') | KeyCode::Char('M') => {
                    conflict.selected = ConflictMode::Merge
                }
                KeyCode::Tab => conflict.cycle(),
                KeyCode::Enter => {
                    self.execute_conflict_choice(conflict, viewport_height);
                    keep_overlay = false;
                }
                _ => {}
            },
            Overlay::MergeDiff(_) => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                    keep_overlay = false;
                }
            }
            Overlay::FindPrompt { input, error } => match key.code {
                KeyCode::Esc | KeyCode::F(4) => keep_overlay = false,
                KeyCode::Backspace => {
                    input.pop();
                    self.update_incremental_find(input, viewport_height, error);
                }
                KeyCode::Enter => {
                    if input.trim().is_empty() {
                        self.clear_find_state();
                    } else {
                        self.select_next_find_match(viewport_height);
                    }
                    keep_overlay = false;
                }
                KeyCode::Down => self.select_next_find_match(viewport_height),
                KeyCode::Up => self.select_previous_find_match(viewport_height),
                KeyCode::Char(ch) if Self::is_text_input_key(&key) => {
                    input.push(ch);
                    self.update_incremental_find(input, viewport_height, error);
                }
                _ => {}
            },
            Overlay::Search(search) => match key.code {
                KeyCode::Esc | KeyCode::F(5) => keep_overlay = false,
                KeyCode::Tab => {
                    search.cycle_field();
                    search.error = None;
                }
                KeyCode::Backspace => {
                    if let Some(input) = search.active_input_mut() {
                        input.pop();
                        search.clear_results();
                        search.error = None;
                    }
                }
                KeyCode::Up => {
                    if search.active_field == SearchField::Results {
                        search.move_selection(-1);
                    }
                }
                KeyCode::Down => {
                    if search.active_field == SearchField::Results {
                        search.move_selection(1);
                    } else if !search.results.is_empty() {
                        search.active_field = SearchField::Results;
                    }
                }
                KeyCode::PageUp => {
                    if search.active_field == SearchField::Results {
                        search.page_up(viewport_height.saturating_sub(7));
                    }
                }
                KeyCode::PageDown => {
                    if search.active_field == SearchField::Results {
                        search.page_down(viewport_height.saturating_sub(7));
                    }
                }
                KeyCode::Home => {
                    if search.active_field == SearchField::Results {
                        search.selected = 0;
                    }
                }
                KeyCode::End => {
                    if search.active_field == SearchField::Results && !search.results.is_empty() {
                        search.selected = search.results.len() - 1;
                    }
                }
                KeyCode::Enter => {
                    if search.active_field == SearchField::Results {
                        if let Some(result) = search.selected_result().cloned() {
                            self.open_search_result(&result, viewport_height);
                        }
                        keep_overlay = false;
                    } else {
                        self.run_global_search(search);
                    }
                }
                KeyCode::Char(ch) if Self::is_text_input_key(&key) => {
                    let accepts_char = match search.active_field {
                        SearchField::Query => true,
                        SearchField::From | SearchField::To => ch.is_ascii_digit() || ch == '-',
                        SearchField::Results => false,
                    };
                    if accepts_char && let Some(input) = search.active_input_mut() {
                        input.push(ch);
                        search.clear_results();
                        search.error = None;
                    }
                }
                _ => {}
            },
            Overlay::ReplacePrompt(prompt) => match key.code {
                KeyCode::Esc | KeyCode::F(6) => keep_overlay = false,
                KeyCode::Tab => {
                    prompt.stage = match prompt.stage {
                        ReplaceStage::Find => ReplaceStage::Replace,
                        ReplaceStage::Replace => ReplaceStage::Find,
                    };
                    prompt.error = None;
                }
                KeyCode::Backspace => {
                    prompt.active_input_mut().pop();
                    prompt.error = None;
                }
                KeyCode::Enter => {
                    if prompt.find_input.trim().is_empty() {
                        prompt.error = Some("Find text cannot be empty.".to_string());
                    } else if prompt.stage == ReplaceStage::Find {
                        prompt.stage = ReplaceStage::Replace;
                    } else {
                        let matches = self.buffer.find(prompt.find_input.trim());
                        if matches.is_empty() {
                            self.flash_status("NOT FOUND.");
                            keep_overlay = false;
                        } else {
                            let (row, col) = self.buffer.cursor();
                            let current_idx = matches
                                .iter()
                                .position(|matched| {
                                    matched.row > row
                                        || (matched.row == row && matched.start_col >= col)
                                })
                                .unwrap_or(0);
                            self.buffer.set_cursor(
                                matches[current_idx].row,
                                matches[current_idx].start_col,
                            );
                            self.ensure_cursor_visible(viewport_height);
                            overlay = Overlay::ReplaceConfirm(ReplaceConfirm {
                                find_text: prompt.find_input.trim().to_string(),
                                replace_text: prompt.replace_input.clone(),
                                matches,
                                current_idx,
                            });
                        }
                    }
                }
                KeyCode::Char(ch) if Self::is_text_input_key(&key) => {
                    prompt.active_input_mut().push(ch);
                    prompt.error = None;
                }
                _ => {}
            },
            Overlay::ReplaceConfirm(confirm) => match key.code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                    self.flash_status("REPLACE CANCELED.");
                    keep_overlay = false;
                }
                KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                    if self.replace_confirm_yes(confirm, viewport_height) {
                        self.flash_status("REPLACE DONE.");
                        keep_overlay = false;
                    }
                }
                KeyCode::Char('n') | KeyCode::Char('N') => {
                    if self.replace_confirm_skip(confirm, viewport_height) {
                        self.flash_status("REPLACE DONE.");
                        keep_overlay = false;
                    }
                }
                KeyCode::Char('a') | KeyCode::Char('A') => {
                    let replaced = self
                        .buffer
                        .replace_all(&confirm.find_text, &confirm.replace_text);
                    self.dirty = replaced > 0;
                    self.refresh_find_matches();
                    self.flash_status(&format!("REPLACED {replaced}."));
                    keep_overlay = false;
                }
                _ => {}
            },
            Overlay::Index(index) => match key.code {
                KeyCode::Esc | KeyCode::F(7) => keep_overlay = false,
                KeyCode::Up => index.move_selection(-1),
                KeyCode::Down => index.move_selection(1),
                KeyCode::PageUp => index.page_up(viewport_height.saturating_sub(4)),
                KeyCode::PageDown => index.page_down(viewport_height.saturating_sub(4)),
                KeyCode::Home => index.selected = 0,
                KeyCode::End => {
                    if !index.items.is_empty() {
                        index.selected = index.items.len() - 1;
                    }
                }
                KeyCode::Enter => {
                    if let Some(date) = index.selected_date() {
                        self.open_date(date);
                        self.flash_status("DATE OPENED.");
                    }
                    keep_overlay = false;
                }
                _ => {}
            },
            Overlay::RecoverDraft { draft_text } => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    self.apply_recovery_choice(true, draft_text);
                    keep_overlay = false;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.apply_recovery_choice(false, draft_text);
                    keep_overlay = false;
                }
                _ => {}
            },
            Overlay::QuitConfirm => match key.code {
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::F(10) => {
                    keep_overlay = false
                }
                KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.should_quit = true;
                    keep_overlay = false;
                }
                _ => {}
            },
        }

        if keep_overlay {
            self.overlay = Some(overlay);
        }
    }

    fn advance_setup_wizard(&mut self, wizard: &mut SetupWizard) -> bool {
        match wizard.step {
            SetupStep::VaultPath => {
                if wizard.path_input.trim().is_empty() {
                    wizard.error = Some("Vault path cannot be empty.".to_string());
                    return false;
                }
                wizard.step = SetupStep::Passphrase;
                false
            }
            SetupStep::Passphrase => {
                if wizard.passphrase_input.chars().count() < 8 {
                    wizard.error = Some("Passphrase must be at least 8 characters.".to_string());
                    return false;
                }
                wizard.step = SetupStep::ConfirmPassphrase;
                false
            }
            SetupStep::ConfirmPassphrase => {
                if wizard.confirm_input != wizard.passphrase_input {
                    wizard.error = Some("Passphrases do not match.".to_string());
                    return false;
                }
                wizard.step = SetupStep::EpochDate;
                false
            }
            SetupStep::EpochDate => self.complete_setup(wizard),
        }
    }

    fn complete_setup(&mut self, wizard: &mut SetupWizard) -> bool {
        let vault_path = expand_tilde(wizard.path_input.trim());
        let epoch = if wizard.epoch_input.trim().is_empty() {
            None
        } else {
            match NaiveDate::parse_from_str(wizard.epoch_input.trim(), "%Y-%m-%d") {
                Ok(date) => Some(date),
                Err(_) => {
                    wizard.error = Some("Epoch date must be YYYY-MM-DD.".to_string());
                    return false;
                }
            }
        };

        let mut passphrase = std::mem::take(&mut wizard.passphrase_input);
        let secret = SecretString::new(passphrase.clone().into_boxed_str());
        passphrase.zeroize();
        wizard.confirm_input.zeroize();

        let metadata = match vault::create_vault(&vault_path, &secret, epoch, "This Mac") {
            Ok(metadata) => metadata,
            Err(error) => {
                wizard.error = Some(format!("Setup failed: {error}"));
                return false;
            }
        };

        let mut config = AppConfig::load_or_default();
        config.vault_path = vault_path.clone();
        config.local_device_id = Some(metadata.device_id.clone());
        if config.device_nickname.trim().is_empty() {
            config.device_nickname = "This Mac".to_string();
        }

        match vault::unlock_vault_with_device(&vault_path, &secret, metadata.device_id.clone()) {
            Ok(unlocked) => {
                self.vault = Some(unlocked);
                self.search_index = None;
                self.vault_path = vault_path.clone();
                if let Err(error) = config.save() {
                    self.flash_status(&format!("VAULT CREATED (config warning: {error})"));
                } else {
                    self.flash_status("VAULT CREATED.");
                }
                self.load_selected_date();
                wizard.passphrase_input.zeroize();
                wizard.confirm_input.zeroize();
                true
            }
            Err(error) => {
                wizard.error = Some(format!("Unlock failed: {error}"));
                false
            }
        }
    }

    fn try_unlock(&mut self, input: &mut String, error: &mut Option<String>) -> bool {
        if input.is_empty() {
            *error = Some("Passphrase cannot be empty.".to_string());
            return false;
        }
        let secret = SecretString::new(std::mem::take(input).into_boxed_str());
        let mut config = AppConfig::load_or_default();
        let mut config_dirty = false;
        let device_id = match config.local_device_id.clone() {
            Some(device_id) => device_id,
            None => {
                let device_id = vault::random_device_id();
                config.local_device_id = Some(device_id.clone());
                config_dirty = true;
                device_id
            }
        };
        if config.device_nickname.trim().is_empty() {
            config.device_nickname = "This Mac".to_string();
            config_dirty = true;
        }

        match vault::unlock_vault_with_device(&self.vault_path, &secret, device_id.clone()) {
            Ok(unlocked) => {
                if let Err(register_error) =
                    vault::register_device(&self.vault_path, &device_id, &config.device_nickname)
                {
                    *error = Some(format!("Device registration failed: {register_error}"));
                    return false;
                }
                if config_dirty {
                    config.vault_path = self.vault_path.clone();
                    if let Err(save_error) = config.save() {
                        self.flash_status(&format!("DEVICE CONFIG WARNING: {save_error}"));
                    }
                }
                self.vault = Some(unlocked);
                self.search_index = None;
                self.flash_status("UNLOCKED.");
                self.load_selected_date();
                true
            }
            Err(_) => {
                *error = Some("Unlock failed. Check passphrase.".to_string());
                false
            }
        }
    }

    fn load_selected_date(&mut self) {
        self.overlay = None;
        self.draft_recovered = false;
        self.pending_conflict = None;
        self.merge_context = None;
        self.last_autosave_check = Instant::now();
        self.last_save_kind = None;
        self.last_save_time = None;

        let Some(vault) = &self.vault else {
            self.buffer = TextBuffer::new();
            self.scroll_row = 0;
            self.dirty = false;
            return;
        };

        match vault.load_date_state(self.selected_date) {
            Ok(state) => {
                self.buffer = TextBuffer::from_text(state.revision_text.as_deref().unwrap_or(""));
                self.scroll_row = 0;
                self.dirty = false;
                self.refresh_find_matches();
                self.pending_conflict = state.conflict.clone();
                if let Some(draft_text) = state.recovery_draft_text {
                    self.overlay = Some(Overlay::RecoverDraft { draft_text });
                } else if let Some(conflict) = state.conflict {
                    self.overlay = Some(Overlay::ConflictChoice(ConflictOverlay::new(conflict)));
                }
            }
            Err(_) => {
                self.buffer = TextBuffer::new();
                self.scroll_row = 0;
                self.dirty = false;
                self.refresh_find_matches();
                self.flash_status("LOAD FAILED.");
            }
        }
    }

    fn open_date(&mut self, date: NaiveDate) {
        if self.dirty {
            self.autosave_current_date();
        }
        self.selected_date = date;
        self.load_selected_date();
    }

    fn open_date_picker(&mut self) {
        let entry_dates = self.load_entry_dates();
        self.overlay = Some(Overlay::DatePicker(DatePicker::new(
            self.selected_date,
            entry_dates,
        )));
    }

    fn open_search_overlay(&mut self) {
        self.overlay = Some(Overlay::Search(SearchOverlay::new(self.find_query.clone())));
    }

    fn open_index_overlay(&mut self) {
        let items = self.load_index_entries();
        self.overlay = Some(Overlay::Index(IndexState::new(items, self.selected_date)));
    }

    fn load_entry_dates(&mut self) -> BTreeSet<NaiveDate> {
        let Some(vault) = &self.vault else {
            return BTreeSet::new();
        };
        match vault.list_entry_dates() {
            Ok(dates) => dates.into_iter().collect(),
            Err(_) => {
                self.flash_status("INDEX LOAD FAILED.");
                BTreeSet::new()
            }
        }
    }

    fn load_index_entries(&mut self) -> Vec<IndexEntry> {
        let Some(vault) = &self.vault else {
            return Vec::new();
        };
        match vault.list_index_entries(INDEX_PREVIEW_CHARS) {
            Ok(entries) => entries,
            Err(_) => {
                self.flash_status("INDEX LOAD FAILED.");
                Vec::new()
            }
        }
    }

    fn save_current_date(&mut self) {
        let Some(vault) = &self.vault else {
            self.flash_status("LOCKED.");
            return;
        };
        let body = self.buffer.to_text();
        let save_result = if let Some(merge_context) = &self.merge_context {
            vault.save_merge_revision(
                self.selected_date,
                &body,
                &merge_context.primary_hash,
                &merge_context.merged_hashes,
            )
        } else {
            vault.save_revision(self.selected_date, &body)
        };

        match save_result {
            Ok(()) => {
                self.dirty = false;
                self.search_index = None;
                self.merge_context = None;
                self.last_save_kind = Some(SaveKind::Saved);
                self.last_save_time = Some(Local::now());
                self.load_selected_date();
                self.last_save_kind = Some(SaveKind::Saved);
                self.last_save_time = Some(Local::now());
                self.flash_status("SAVED.");
            }
            Err(_) => self.flash_status("SAVE FAILED."),
        }
    }

    fn autosave_current_date(&mut self) {
        let Some(vault) = &self.vault else {
            return;
        };
        let body = self.buffer.to_text();
        if vault.save_draft(self.selected_date, &body).is_ok() {
            self.last_save_kind = Some(SaveKind::Autosaved);
            self.last_save_time = Some(Local::now());
        }
    }

    fn apply_recovery_choice(&mut self, use_draft: bool, draft_text: &str) {
        let (resolved_text, recovered) =
            resolve_recovery_text(use_draft, self.buffer.to_text().as_str(), draft_text);
        self.buffer = TextBuffer::from_text(&resolved_text);
        self.scroll_row = 0;
        self.refresh_find_matches();
        self.draft_recovered = recovered;
        self.dirty = recovered;
        self.apply_pending_search_jump(None);
        if recovered {
            self.flash_status("DRAFT RECOVERED.");
        } else {
            self.flash_status("DRAFT IGNORED.");
        }
        if let Some(conflict) = self.pending_conflict.clone() {
            self.overlay = Some(Overlay::ConflictChoice(ConflictOverlay::new(conflict)));
        }
    }

    fn update_incremental_find(
        &mut self,
        query: &str,
        viewport_height: usize,
        error: &mut Option<String>,
    ) {
        let query = query.trim();
        if query.is_empty() {
            self.clear_find_state();
            *error = None;
            return;
        }
        self.apply_find(query.to_string(), viewport_height, error, false);
    }

    fn apply_find(
        &mut self,
        query: String,
        viewport_height: usize,
        error: &mut Option<String>,
        flash_status: bool,
    ) {
        self.find_query = Some(query.clone());
        self.find_matches = self.buffer.find(&query);
        if self.find_matches.is_empty() {
            self.current_match_idx = 0;
            *error = Some("No matches.".to_string());
            if flash_status {
                self.flash_status("NOT FOUND.");
            }
            return;
        }
        *error = None;
        let (row, col) = self.buffer.cursor();
        self.current_match_idx = self
            .find_matches
            .iter()
            .position(|matched| {
                matched.row > row || (matched.row == row && matched.start_col >= col)
            })
            .unwrap_or(0);
        if let Some(matched) = self.current_match() {
            self.buffer.set_cursor(matched.row, matched.start_col);
            self.ensure_cursor_visible(viewport_height);
            if flash_status {
                self.flash_status("FOUND.");
            }
        }
    }

    fn clear_find_state(&mut self) {
        self.find_query = None;
        self.find_matches.clear();
        self.current_match_idx = 0;
        self.flash_status("FIND CLEARED.");
    }

    fn select_next_find_match(&mut self, viewport_height: usize) {
        if self.find_matches.is_empty() {
            return;
        }
        self.current_match_idx = (self.current_match_idx + 1) % self.find_matches.len();
        if let Some(matched) = self.current_match() {
            self.buffer.set_cursor(matched.row, matched.start_col);
            self.ensure_cursor_visible(viewport_height);
        }
    }

    fn select_previous_find_match(&mut self, viewport_height: usize) {
        if self.find_matches.is_empty() {
            return;
        }
        if self.current_match_idx == 0 {
            self.current_match_idx = self.find_matches.len() - 1;
        } else {
            self.current_match_idx -= 1;
        }
        if let Some(matched) = self.current_match() {
            self.buffer.set_cursor(matched.row, matched.start_col);
            self.ensure_cursor_visible(viewport_height);
        }
    }

    fn run_global_search(&mut self, search: &mut SearchOverlay) {
        let query = search.query_input.trim().to_string();
        if query.is_empty() {
            search.error = Some("Query cannot be empty.".to_string());
            return;
        }

        let from = match parse_optional_overlay_date("from", &search.from_input) {
            Ok(date) => date,
            Err(error) => {
                search.error = Some(error);
                return;
            }
        };
        let to = match parse_optional_overlay_date("to", &search.to_input) {
            Ok(date) => date,
            Err(error) => {
                search.error = Some(error);
                return;
            }
        };
        if let (Some(from), Some(to)) = (from, to)
            && from > to
        {
            search.error = Some("FROM cannot be after TO.".to_string());
            return;
        }

        if let Err(error) = self.ensure_search_index() {
            search.error = Some(error);
            return;
        }

        let results = self
            .search_index
            .as_ref()
            .expect("search index exists")
            .search(&SearchQuery {
                text: query,
                from,
                to,
            });

        search.results = results;
        search.selected = 0;
        search.error = if search.results.is_empty() {
            Some("No matches.".to_string())
        } else {
            None
        };
        search.active_field = if search.results.is_empty() {
            SearchField::Query
        } else {
            SearchField::Results
        };
    }

    fn ensure_search_index(&mut self) -> Result<(), String> {
        if self.search_index.is_some() {
            return Ok(());
        }
        let Some(vault) = &self.vault else {
            return Err("Vault locked.".to_string());
        };
        let documents = vault
            .load_search_documents()
            .map_err(|error| format!("search load failed: {error}"))?;
        let document_count = documents.len();
        self.search_index = Some(SearchIndex::build(documents));
        self.flash_status(&format!("SEARCH INDEX READY ({document_count})."));
        Ok(())
    }

    fn open_search_result(&mut self, result: &SearchResult, viewport_height: usize) {
        self.pending_search_jump = Some(SearchJump {
            match_text: result.matched_text.clone(),
            row: result.row,
            start_col: result.start_col,
        });
        self.open_date(result.date);
        if !matches!(self.overlay, Some(Overlay::RecoverDraft { .. })) {
            self.apply_pending_search_jump(Some(viewport_height));
        }
        self.flash_status("MATCH OPENED.");
    }

    fn execute_conflict_choice(&mut self, conflict: &ConflictOverlay, viewport_height: usize) {
        let Some(head_a) = conflict.conflict.heads.first() else {
            return;
        };
        let head_b = conflict.conflict.heads.get(1).unwrap_or(head_a);

        match conflict.selected {
            ConflictMode::ViewA => {
                self.buffer = TextBuffer::from_text(&head_a.body);
                self.scroll_row = 0;
                self.dirty = false;
                self.merge_context = None;
                self.refresh_find_matches();
                self.apply_pending_search_jump(Some(viewport_height));
                self.flash_status("VIEW A.");
            }
            ConflictMode::ViewB => {
                self.buffer = TextBuffer::from_text(&head_b.body);
                self.scroll_row = 0;
                self.dirty = false;
                self.merge_context = None;
                self.refresh_find_matches();
                self.apply_pending_search_jump(Some(viewport_height));
                self.flash_status("VIEW B.");
            }
            ConflictMode::Merge => {
                self.buffer = TextBuffer::from_text(&head_a.body);
                self.scroll_row = 0;
                self.dirty = false;
                self.refresh_find_matches();
                self.merge_context = Some(MergeContext {
                    primary_hash: head_a.revision_hash.clone(),
                    merged_hashes: conflict
                        .conflict
                        .heads
                        .iter()
                        .skip(1)
                        .map(|head| head.revision_hash.clone())
                        .collect(),
                });
                self.overlay = Some(Overlay::MergeDiff(conflict.conflict.clone()));
                self.flash_status("MERGE MODE.");
            }
        }
    }

    fn apply_pending_search_jump(&mut self, viewport_height: Option<usize>) {
        let Some(jump) = self.pending_search_jump.take() else {
            return;
        };

        self.find_query = Some(jump.match_text.clone());
        self.refresh_find_matches();
        if self.find_matches.is_empty() {
            return;
        }

        self.current_match_idx = self
            .find_matches
            .iter()
            .position(|matched| matched.row == jump.row && matched.start_col == jump.start_col)
            .unwrap_or_else(|| {
                self.find_matches
                    .iter()
                    .position(|matched| {
                        matched.row > jump.row
                            || (matched.row == jump.row && matched.start_col >= jump.start_col)
                    })
                    .unwrap_or(0)
            });

        if let Some(matched) = self.current_match() {
            self.buffer.set_cursor(matched.row, matched.start_col);
            self.ensure_cursor_visible(viewport_height.unwrap_or(self.last_viewport_height));
        }
    }

    fn replace_confirm_yes(
        &mut self,
        confirm: &mut ReplaceConfirm,
        viewport_height: usize,
    ) -> bool {
        let Some(current) = confirm.matches.get(confirm.current_idx).cloned() else {
            return true;
        };
        self.buffer.replace_at(&current, &confirm.replace_text);
        self.dirty = true;
        confirm.matches = self.buffer.find(&confirm.find_text);
        if confirm.matches.is_empty() {
            self.refresh_find_matches();
            return true;
        }

        let (row, col) = self.buffer.cursor();
        confirm.current_idx = confirm
            .matches
            .iter()
            .position(|matched| {
                matched.row > row || (matched.row == row && matched.start_col >= col)
            })
            .unwrap_or(0);

        if let Some(next_match) = confirm.matches.get(confirm.current_idx) {
            self.buffer.set_cursor(next_match.row, next_match.start_col);
            self.ensure_cursor_visible(viewport_height);
        }
        self.refresh_find_matches();
        false
    }

    fn replace_confirm_skip(
        &mut self,
        confirm: &mut ReplaceConfirm,
        viewport_height: usize,
    ) -> bool {
        if confirm.current_idx + 1 >= confirm.matches.len() {
            return true;
        }
        confirm.current_idx += 1;
        if let Some(next_match) = confirm.matches.get(confirm.current_idx) {
            self.buffer.set_cursor(next_match.row, next_match.start_col);
            self.ensure_cursor_visible(viewport_height);
        }
        false
    }

    fn refresh_find_matches(&mut self) {
        if let Some(query) = self.find_query.clone() {
            self.find_matches = self.buffer.find(&query);
            if self.current_match_idx >= self.find_matches.len() {
                self.current_match_idx = 0;
            }
        } else {
            self.find_matches.clear();
            self.current_match_idx = 0;
        }
    }

    fn ensure_cursor_visible(&mut self, viewport_height: usize) {
        let rows = viewport_height.max(1);
        let cursor_row = self.buffer.cursor_row();
        if cursor_row < self.scroll_row {
            self.scroll_row = cursor_row;
            return;
        }
        let bottom = self.scroll_row + rows.saturating_sub(1);
        if cursor_row > bottom {
            self.scroll_row = cursor_row + 1 - rows;
        }
    }

    fn flash_status(&mut self, text: &str) {
        self.status_flash = Some(StatusMessage {
            text: text.to_string(),
            expires_at: Instant::now() + STATUS_DURATION,
        });
    }

    fn is_ctrl_char(key: &KeyEvent, ch: char) -> bool {
        key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char(input) if input.eq_ignore_ascii_case(&ch))
    }

    fn is_text_input_key(key: &KeyEvent) -> bool {
        key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT
    }
}

fn resolve_recovery_text(use_draft: bool, revision_text: &str, draft_text: &str) -> (String, bool) {
    if use_draft {
        (draft_text.to_string(), true)
    } else {
        (revision_text.to_string(), false)
    }
}

fn expand_tilde(input: &str) -> PathBuf {
    if input == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(input));
    }
    if let Some(rest) = input.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    PathBuf::from(input)
}

fn parse_optional_overlay_date(label: &str, input: &str) -> Result<Option<NaiveDate>, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    NaiveDate::parse_from_str(trimmed, "%Y-%m-%d")
        .map(Some)
        .map_err(|_| format!("Invalid {label} date. Use YYYY-MM-DD."))
}

#[cfg(test)]
mod tests {
    use super::{
        App, IndexState, SearchField, SearchOverlay, parse_optional_overlay_date,
        resolve_recovery_text,
    };
    use crate::vault::IndexEntry;
    use chrono::{Duration, NaiveDate};
    use crossterm::event::Event;

    #[test]
    fn resize_event_does_not_panic() {
        let mut app = App::new();
        app.handle_event(Event::Resize(80, 25), 23);
        assert_eq!(app.scroll_row(), 0);
    }

    #[test]
    fn recovery_selection_prefers_draft_when_yes() {
        let (text, recovered) = resolve_recovery_text(true, "revision", "draft");
        assert_eq!(text, "draft");
        assert!(recovered);
    }

    #[test]
    fn recovery_selection_keeps_revision_when_no() {
        let (text, recovered) = resolve_recovery_text(false, "revision", "draft");
        assert_eq!(text, "revision");
        assert!(!recovered);
    }

    #[test]
    fn index_window_centers_current_selection_when_possible() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");
        let items = (0..10)
            .map(|offset| IndexEntry {
                date: date + Duration::days(offset),
                entry_number: format!("{offset:07}"),
                preview: format!("Entry {offset}"),
                has_conflict: false,
            })
            .collect();
        let index = IndexState::new(items, date + Duration::days(5));
        assert_eq!(index.window(5), (3, 8));
    }

    #[test]
    fn app_respects_initial_open_date() {
        let initial = NaiveDate::from_ymd_opt(2026, 4, 2).expect("date");
        let app = App::with_initial_date(Some(initial));
        assert!(app.header_date_time_label().starts_with("2026-04-02"));
    }

    #[test]
    fn search_overlay_cycles_into_results_when_present() {
        let mut overlay = SearchOverlay::new(None);
        overlay.results.push(crate::search::SearchResult {
            date: NaiveDate::from_ymd_opt(2026, 3, 16).expect("date"),
            entry_number: "0000016".to_string(),
            snippet: crate::search::Snippet {
                text: "note".to_string(),
                highlight_start: 0,
                highlight_end: 4,
            },
            row: 0,
            start_col: 0,
            end_col: 4,
            matched_text: "note".to_string(),
        });
        overlay.cycle_field();
        overlay.cycle_field();
        overlay.cycle_field();
        assert_eq!(overlay.active_field, SearchField::Results);
    }

    #[test]
    fn overlay_date_parser_accepts_blank_and_valid_dates() {
        assert_eq!(
            parse_optional_overlay_date("from", "").expect("blank"),
            None
        );
        assert_eq!(
            parse_optional_overlay_date("to", "2026-03-16").expect("date"),
            Some(NaiveDate::from_ymd_opt(2026, 3, 16).expect("date"))
        );
    }
}
