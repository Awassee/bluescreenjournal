use crate::{
    config::{
        self, AppConfig, LastSyncInfo, MacroActionConfig, MacroCommandConfig, RecentExportInfo,
        default_vault_path,
    },
    doctor,
    help::{self, EnvironmentSettings},
    logging, platform,
    search::{SearchIndex, SearchQuery, SearchResult},
    secure_fs, sync,
    tui::{
        buffer::{BufferStats, MatchPos, TextBuffer},
        calendar,
    },
    vault::{self, EntryMetadata, IndexEntry, UnlockedVault},
};
use chrono::{DateTime, Datelike, Duration as ChronoDuration, Local, NaiveDate};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use secrecy::SecretString;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    env,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use zeroize::Zeroize;

const STATUS_DURATION: Duration = Duration::from_millis(1600);
const AUTOSAVE_INTERVAL: Duration = Duration::from_millis(2500);
const INDEX_PREVIEW_CHARS: usize = 54;
const TAB_INSERT_TEXT: &str = "     ";
const SOUNDTRACK_CACHE_DIR_NAME: &str = "bsj-soundtrack-cache";

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
    ClosingPrompt {
        input: String,
    },
    ConflictChoice(ConflictOverlay),
    MergeDiff(vault::ConflictState),
    Search(SearchOverlay),
    ReplacePrompt(ReplacePrompt),
    ReplaceConfirm(ReplaceConfirm),
    MetadataPrompt(MetadataPrompt),
    ExportPrompt(ExportPrompt),
    SettingPrompt(SettingPrompt),
    Index(IndexState),
    SyncStatus(SyncStatusOverlay),
    RestorePrompt(RestorePrompt),
    Info(InfoOverlay),
    Picker(PickerOverlay),
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

    fn wipe(&mut self) {
        self.path_input.zeroize();
        self.passphrase_input.zeroize();
        self.confirm_input.zeroize();
        self.epoch_input.zeroize();
        zeroize_optional_string(&mut self.error);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DatePicker {
    pub month: NaiveDate,
    pub selected_date: NaiveDate,
    pub entry_dates: BTreeSet<NaiveDate>,
    pub jump_input: String,
}

impl DatePicker {
    pub fn new(selected_date: NaiveDate, entry_dates: BTreeSet<NaiveDate>) -> Self {
        Self {
            month: calendar::month_start(selected_date),
            selected_date,
            entry_dates,
            jump_input: String::new(),
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

    fn apply_jump_input(&mut self) -> Result<NaiveDate, String> {
        let trimmed = self.jump_input.trim();
        if trimmed.is_empty() {
            return Ok(self.selected_date);
        }
        let date = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d")
            .map_err(|_| "Use YYYY-MM-DD for date jump.".to_string())?;
        self.selected_date = date;
        self.month = calendar::month_start(date);
        self.jump_input.clear();
        Ok(date)
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

    fn wipe(&mut self) {
        self.find_input.zeroize();
        self.replace_input.zeroize();
        zeroize_optional_string(&mut self.error);
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
    AcceptA,
    AcceptB,
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
            ConflictMode::ViewB => ConflictMode::AcceptA,
            ConflictMode::AcceptA => ConflictMode::AcceptB,
            ConflictMode::AcceptB => ConflictMode::Merge,
            ConflictMode::Merge => ConflictMode::ViewA,
        };
    }

    fn wipe(&mut self) {
        wipe_conflict_state(&mut self.conflict);
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

    fn apply_today_scope(&mut self, today: NaiveDate) {
        let today = today.format("%Y-%m-%d").to_string();
        self.from_input = today.clone();
        self.to_input = today;
        self.error = None;
    }

    fn apply_month_scope(&mut self, today: NaiveDate) {
        let month_start = today.with_day(1).unwrap_or(today);
        self.from_input = month_start.format("%Y-%m-%d").to_string();
        self.to_input = today.format("%Y-%m-%d").to_string();
        self.error = None;
    }

    fn apply_week_scope(&mut self, today: NaiveDate) {
        let week_start = today
            .checked_sub_signed(ChronoDuration::days(6))
            .unwrap_or(today);
        self.from_input = week_start.format("%Y-%m-%d").to_string();
        self.to_input = today.format("%Y-%m-%d").to_string();
        self.error = None;
    }

    fn apply_year_scope(&mut self, today: NaiveDate) {
        let year_start = NaiveDate::from_ymd_opt(today.year(), 1, 1).unwrap_or(today);
        self.from_input = year_start.format("%Y-%m-%d").to_string();
        self.to_input = today.format("%Y-%m-%d").to_string();
        self.error = None;
    }

    fn clear_filters(&mut self) {
        self.from_input.clear();
        self.to_input.clear();
        self.error = None;
    }

    pub(crate) fn range_label(&self) -> String {
        match (self.from_input.trim(), self.to_input.trim()) {
            ("", "") => "ALL TIME".to_string(),
            (from, "") => format!("FROM {from}"),
            ("", to) => format!("TO {to}"),
            (from, to) => format!("{from}..{to}"),
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
        wipe_search_results(&mut self.results);
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

    pub(crate) fn selected_result(&self) -> Option<&SearchResult> {
        self.results.get(self.selected)
    }

    fn wipe(&mut self) {
        self.query_input.zeroize();
        self.from_input.zeroize();
        self.to_input.zeroize();
        self.clear_results();
        zeroize_optional_string(&mut self.error);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InfoOverlay {
    pub title: String,
    pub lines: Vec<String>,
    pub scroll: usize,
}

impl InfoOverlay {
    fn from_text(title: impl Into<String>, text: String) -> Self {
        let mut lines = text
            .lines()
            .map(|line| line.to_string())
            .collect::<Vec<_>>();
        if lines.is_empty() {
            lines.push(String::new());
        }
        Self {
            title: title.into(),
            lines,
            scroll: 0,
        }
    }

    pub fn window(&self, max_rows: usize) -> (usize, usize) {
        if self.lines.is_empty() || max_rows == 0 {
            return (0, 0);
        }
        let max_rows = max_rows.max(1);
        let max_start = self.lines.len().saturating_sub(max_rows);
        let start = self.scroll.min(max_start);
        let end = (start + max_rows).min(self.lines.len());
        (start, end)
    }

    fn move_scroll(&mut self, delta: isize) {
        if self.lines.is_empty() {
            self.scroll = 0;
            return;
        }
        let max_scroll = self.lines.len().saturating_sub(1);
        let next = self.scroll as isize + delta;
        self.scroll = next.clamp(0, max_scroll as isize) as usize;
    }

    fn page_up(&mut self, amount: usize) {
        self.move_scroll(-(amount.max(1) as isize));
    }

    fn page_down(&mut self, amount: usize) {
        self.move_scroll(amount.max(1) as isize);
    }

    fn wipe(&mut self) {
        self.title.zeroize();
        for line in &mut self.lines {
            line.zeroize();
        }
        self.lines.clear();
        self.scroll = 0;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickerAction {
    Menu(MenuAction),
    OpenDate(NaiveDate),
    OpenSearch(String),
    InstallUpdate {
        tag: String,
        release_url: String,
        command: String,
    },
    OpenExportPrompt {
        format: ExportFormatUi,
        path: String,
    },
    OpenRestorePrompt {
        backup_path: PathBuf,
    },
    InsertText(String),
    ShowInfo {
        title: String,
        text: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickerItem {
    pub title: String,
    pub detail: String,
    pub keywords: String,
    pub action: PickerAction,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickerOverlay {
    pub title: String,
    pub items: Vec<PickerItem>,
    pub selected: usize,
    pub filter_input: String,
    pub empty_message: String,
    filter_haystacks: Vec<String>,
    filtered_indices: Vec<usize>,
}

impl PickerOverlay {
    fn new(
        title: impl Into<String>,
        items: Vec<PickerItem>,
        empty_message: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            filter_haystacks: items
                .iter()
                .map(|item| {
                    format!(
                        "{} {} {}",
                        item.title.to_ascii_lowercase(),
                        item.detail.to_ascii_lowercase(),
                        item.keywords.to_ascii_lowercase()
                    )
                })
                .collect(),
            filtered_indices: (0..items.len()).collect(),
            items,
            selected: 0,
            filter_input: String::new(),
            empty_message: empty_message.into(),
        }
    }

    fn recompute_filter(&mut self) {
        let trimmed = self.filter_input.trim().to_ascii_lowercase();
        if trimmed.is_empty() {
            self.filtered_indices = (0..self.items.len()).collect();
            self.clamp_selection();
            return;
        }

        let tokens = trimmed.split_whitespace().collect::<Vec<_>>();
        self.filtered_indices = self
            .filter_haystacks
            .iter()
            .enumerate()
            .filter_map(|(idx, haystack)| {
                tokens
                    .iter()
                    .all(|token| haystack.contains(token))
                    .then_some(idx)
            })
            .collect();
        self.clamp_selection();
    }

    fn filtered_indices(&self) -> &[usize] {
        &self.filtered_indices
    }

    fn clamp_selection(&mut self) {
        let filtered_len = self.filtered_indices.len();
        self.selected = self.selected.min(filtered_len.saturating_sub(1));
        if filtered_len == 0 {
            self.selected = 0;
        }
    }

    pub fn window(&self, max_rows: usize) -> (&[usize], usize, usize) {
        if self.filtered_indices.is_empty() || max_rows == 0 {
            return (&self.filtered_indices, 0, 0);
        }
        let max_rows = max_rows.max(1);
        let mut start = self.selected.saturating_sub(max_rows / 2);
        let max_start = self.filtered_indices.len().saturating_sub(max_rows);
        if start > max_start {
            start = max_start;
        }
        let end = (start + max_rows).min(self.filtered_indices.len());
        (&self.filtered_indices, start, end)
    }

    fn selected_item(&self) -> Option<&PickerItem> {
        self.filtered_indices
            .get(self.selected)
            .and_then(|index| self.items.get(*index))
    }

    fn move_selection(&mut self, delta: isize) {
        let filtered_len = self.filtered_indices.len();
        if filtered_len == 0 {
            self.selected = 0;
            return;
        }
        let next = self.selected as isize + delta;
        self.selected = next.clamp(0, filtered_len as isize - 1) as usize;
    }

    fn page_up(&mut self, amount: usize) {
        self.move_selection(-(amount.max(1) as isize));
    }

    fn page_down(&mut self, amount: usize) {
        self.move_selection(amount.max(1) as isize);
    }

    fn push_filter_char(&mut self, ch: char) {
        self.filter_input.push(ch);
        self.selected = 0;
        self.recompute_filter();
    }

    fn pop_filter_char(&mut self) {
        self.filter_input.pop();
        self.recompute_filter();
    }

    fn wipe(&mut self) {
        self.title.zeroize();
        self.filter_input.zeroize();
        self.empty_message.zeroize();
        for haystack in &mut self.filter_haystacks {
            haystack.zeroize();
        }
        self.filter_haystacks.clear();
        self.filtered_indices.clear();
        for item in &mut self.items {
            item.title.zeroize();
            item.detail.zeroize();
            item.keywords.zeroize();
            match &mut item.action {
                PickerAction::Menu(_) | PickerAction::OpenDate(_) => {}
                PickerAction::OpenSearch(query)
                | PickerAction::OpenExportPrompt { path: query, .. }
                | PickerAction::InsertText(query) => {
                    query.zeroize();
                }
                PickerAction::InstallUpdate {
                    tag,
                    release_url,
                    command,
                } => {
                    tag.zeroize();
                    release_url.zeroize();
                    command.zeroize();
                }
                PickerAction::OpenRestorePrompt { backup_path } => {
                    backup_path.as_mut_os_string().clear();
                }
                PickerAction::ShowInfo { title, text } => {
                    title.zeroize();
                    text.zeroize();
                }
            }
        }
        self.items.clear();
        self.selected = 0;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormatUi {
    PlainText,
    Markdown,
}

impl ExportFormatUi {
    pub fn label(self) -> &'static str {
        match self {
            Self::PlainText => "TEXT",
            Self::Markdown => "MARKDOWN",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::PlainText => "txt",
            Self::Markdown => "md",
        }
    }

    fn toggle(&mut self) {
        *self = match self {
            Self::PlainText => Self::Markdown,
            Self::Markdown => Self::PlainText,
        };
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportPrompt {
    pub format: ExportFormatUi,
    pub path_input: String,
    pub error: Option<String>,
}

impl ExportPrompt {
    fn new(date: NaiveDate) -> Self {
        let format = ExportFormatUi::PlainText;
        Self {
            format,
            path_input: default_export_path(date, format).display().to_string(),
            error: None,
        }
    }

    fn with_preset(date: NaiveDate, format: ExportFormatUi, path_input: String) -> Self {
        let path_input = if path_input.trim().is_empty() {
            default_export_path(date, format).display().to_string()
        } else {
            path_input
        };
        Self {
            format,
            path_input,
            error: None,
        }
    }

    fn toggle_format(&mut self, date: NaiveDate) {
        let old_extension = self.format.extension();
        self.format.toggle();
        let new_extension = self.format.extension();
        if self.path_input.ends_with(&format!(".{old_extension}")) {
            self.path_input
                .truncate(self.path_input.len().saturating_sub(old_extension.len()));
            self.path_input.push_str(new_extension);
        } else if self.path_input.trim().is_empty() {
            self.path_input = default_export_path(date, self.format).display().to_string();
        }
        self.error = None;
    }

    fn wipe(&mut self) {
        self.path_input.zeroize();
        zeroize_optional_string(&mut self.error);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexState {
    pub all_items: Vec<IndexEntry>,
    pub items: Vec<IndexEntry>,
    pub selected: usize,
    pub filter_input: String,
    pub sort_oldest_first: bool,
    pub favorites_only: bool,
    pub conflicts_only: bool,
    pub favorite_dates: BTreeSet<NaiveDate>,
}

impl IndexState {
    fn new(
        items: Vec<IndexEntry>,
        selected_date: NaiveDate,
        favorite_dates: BTreeSet<NaiveDate>,
    ) -> Self {
        let mut state = Self {
            all_items: items.clone(),
            items,
            selected: 0,
            filter_input: String::new(),
            sort_oldest_first: false,
            favorites_only: false,
            conflicts_only: false,
            favorite_dates,
        };
        state.refresh(selected_date);
        state
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

    fn toggle_sort(&mut self, selected_date: NaiveDate) {
        self.sort_oldest_first = !self.sort_oldest_first;
        self.refresh(selected_date);
    }

    fn toggle_favorites_only(&mut self, selected_date: NaiveDate) {
        self.favorites_only = !self.favorites_only;
        self.refresh(selected_date);
    }

    fn toggle_conflicts_only(&mut self, selected_date: NaiveDate) {
        self.conflicts_only = !self.conflicts_only;
        self.refresh(selected_date);
    }

    fn push_filter_char(&mut self, ch: char, selected_date: NaiveDate) {
        self.filter_input.push(ch);
        self.refresh(selected_date);
    }

    fn pop_filter_char(&mut self, selected_date: NaiveDate) {
        self.filter_input.pop();
        self.refresh(selected_date);
    }

    fn refresh(&mut self, selected_date: NaiveDate) {
        let needle = self.filter_input.trim().to_ascii_lowercase();
        self.items = self
            .all_items
            .iter()
            .filter(|entry| {
                (!self.favorites_only || self.favorite_dates.contains(&entry.date))
                    && (!self.conflicts_only || entry.has_conflict)
                    && index_matches_filter(entry, &needle)
            })
            .cloned()
            .collect();
        if self.sort_oldest_first {
            self.items.reverse();
        }
        self.selected = self
            .items
            .iter()
            .position(|entry| entry.date == selected_date)
            .unwrap_or_else(|| self.selected.min(self.items.len().saturating_sub(1)));
        if self.items.is_empty() {
            self.selected = 0;
        }
    }

    fn wipe(&mut self) {
        for item in &mut self.all_items {
            item.entry_number.zeroize();
            item.preview.zeroize();
            zeroize_entry_metadata(&mut item.metadata);
        }
        self.all_items.clear();
        for item in &mut self.items {
            item.entry_number.zeroize();
            item.preview.zeroize();
            zeroize_entry_metadata(&mut item.metadata);
        }
        self.items.clear();
        self.selected = 0;
        self.filter_input.zeroize();
        self.sort_oldest_first = false;
        self.favorites_only = false;
        self.conflicts_only = false;
        self.favorite_dates.clear();
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncPhase {
    Pending,
    Running,
    Complete {
        pulled: usize,
        pushed: usize,
        conflicts: Vec<NaiveDate>,
        integrity_ok: bool,
        integrity_issue_count: usize,
    },
    Error {
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncStatusOverlay {
    pub backend_label: String,
    pub target_label: String,
    pub draft_notice: bool,
    pub phase: SyncPhase,
}

impl SyncStatusOverlay {
    fn pending(backend_label: String, target_label: String, draft_notice: bool) -> Self {
        Self {
            backend_label,
            target_label,
            draft_notice,
            phase: SyncPhase::Pending,
        }
    }

    fn mark_running(&mut self) {
        self.phase = SyncPhase::Running;
    }

    fn set_complete(
        &mut self,
        report: vault::SyncReport,
        integrity_status: Option<&IntegrityStatus>,
    ) {
        let (integrity_ok, integrity_issue_count) = integrity_status
            .map(|status| (status.ok, status.issue_count))
            .unwrap_or((true, 0));
        self.phase = SyncPhase::Complete {
            pulled: report.pulled,
            pushed: report.pushed,
            conflicts: report.conflicts,
            integrity_ok,
            integrity_issue_count,
        };
    }

    fn set_error(&mut self, message: String) {
        self.phase = SyncPhase::Error { message };
    }

    fn can_close(&self) -> bool {
        !matches!(self.phase, SyncPhase::Pending | SyncPhase::Running)
    }

    fn wipe(&mut self) {
        self.backend_label.zeroize();
        self.target_label.zeroize();
        if let SyncPhase::Error { message } = &mut self.phase {
            message.zeroize();
        }
        self.phase = SyncPhase::Pending;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuId {
    File,
    Edit,
    Search,
    Go,
    Tools,
    Setup,
    Help,
}

impl MenuId {
    pub fn title(self) -> &'static str {
        match self {
            MenuId::File => "FILE",
            MenuId::Edit => "EDIT",
            MenuId::Search => "SEARCH",
            MenuId::Go => "GO",
            MenuId::Tools => "TOOLS",
            MenuId::Setup => "SETUP",
            MenuId::Help => "HELP",
        }
    }

    pub fn all() -> &'static [Self] {
        const MENUS: [MenuId; 7] = [
            MenuId::File,
            MenuId::Edit,
            MenuId::Search,
            MenuId::Go,
            MenuId::Tools,
            MenuId::Setup,
            MenuId::Help,
        ];
        &MENUS
    }

    fn from_hotkey(ch: char) -> Option<Self> {
        match ch.to_ascii_lowercase() {
            'f' => Some(Self::File),
            'e' => Some(Self::Edit),
            's' => Some(Self::Search),
            'g' => Some(Self::Go),
            't' => Some(Self::Tools),
            'u' => Some(Self::Setup),
            'h' => Some(Self::Help),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingField {
    VaultPath,
    SyncTargetPath,
    DeviceNickname,
    Clock12h,
    ShowSeconds,
    ShowRuler,
    ShowFooterLegend,
    SoundtrackSource,
    DailyWordGoal,
    BackupDaily,
    BackupWeekly,
    BackupMonthly,
}

impl SettingField {
    pub fn key(self) -> &'static str {
        match self {
            SettingField::VaultPath => "vault_path",
            SettingField::SyncTargetPath => "sync_target_path",
            SettingField::DeviceNickname => "device_nickname",
            SettingField::Clock12h => "clock_12h",
            SettingField::ShowSeconds => "show_seconds",
            SettingField::ShowRuler => "show_ruler",
            SettingField::ShowFooterLegend => "show_footer_legend",
            SettingField::SoundtrackSource => "soundtrack_source",
            SettingField::DailyWordGoal => "daily_word_goal",
            SettingField::BackupDaily => "backup_retention.daily",
            SettingField::BackupWeekly => "backup_retention.weekly",
            SettingField::BackupMonthly => "backup_retention.monthly",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SettingField::VaultPath => "Vault Path",
            SettingField::SyncTargetPath => "Sync Folder",
            SettingField::DeviceNickname => "Device Name",
            SettingField::Clock12h => "12-Hour Clock",
            SettingField::ShowSeconds => "Show Seconds",
            SettingField::ShowRuler => "Show Ruler",
            SettingField::ShowFooterLegend => "Footer Legend",
            SettingField::SoundtrackSource => "Soundtrack URL/Path",
            SettingField::DailyWordGoal => "Daily Word Goal",
            SettingField::BackupDaily => "Daily Backups",
            SettingField::BackupWeekly => "Weekly Backups",
            SettingField::BackupMonthly => "Monthly Backups",
        }
    }

    pub fn prompt(self) -> &'static str {
        match self {
            SettingField::VaultPath => "Set vault path:",
            SettingField::SyncTargetPath => "Set folder sync path (blank clears):",
            SettingField::DeviceNickname => "Set device nickname:",
            SettingField::Clock12h => "Use 12-hour clock (true/false):",
            SettingField::ShowSeconds => "Show seconds in header clock (true/false):",
            SettingField::ShowRuler => "Show DOS ruler above editor (true/false):",
            SettingField::ShowFooterLegend => "Show function-key footer legend (true/false):",
            SettingField::SoundtrackSource => "Set soundtrack URL or file path:",
            SettingField::DailyWordGoal => "Set daily word goal (blank clears):",
            SettingField::BackupDaily => "Keep daily backups:",
            SettingField::BackupWeekly => "Keep weekly backups:",
            SettingField::BackupMonthly => "Keep monthly backups:",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            SettingField::VaultPath => "Changing the path relocks into the selected vault.",
            SettingField::SyncTargetPath => "Folder sync target for iCloud / Dropbox style sync.",
            SettingField::DeviceNickname => "Shown in devices/<deviceId>.json and conflicts.",
            SettingField::Clock12h => "Changes header clock formatting across the app.",
            SettingField::ShowSeconds => "Useful for tight writing sessions and save timing.",
            SettingField::ShowRuler => "Turns the WordPerfect-style ruler line on or off.",
            SettingField::ShowFooterLegend => "Hide when you want a cleaner writing footer.",
            SettingField::SoundtrackSource => {
                "Used by TOOLS -> Toggle Soundtrack. Leave blank to disable quick playback."
            }
            SettingField::DailyWordGoal => "Shown as live progress in the footer and dashboard.",
            SettingField::BackupDaily
            | SettingField::BackupWeekly
            | SettingField::BackupMonthly => "Retention count; use a non-negative integer.",
        }
    }

    fn current_input(self, config: &AppConfig) -> String {
        match self {
            SettingField::VaultPath => config.vault_path.display().to_string(),
            SettingField::SyncTargetPath => config
                .sync_target_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            SettingField::DeviceNickname => config.device_nickname.clone(),
            SettingField::Clock12h => config.clock_12h.to_string(),
            SettingField::ShowSeconds => config.show_seconds.to_string(),
            SettingField::ShowRuler => config.show_ruler.to_string(),
            SettingField::ShowFooterLegend => config.show_footer_legend.to_string(),
            SettingField::SoundtrackSource => config.soundtrack_source.clone(),
            SettingField::DailyWordGoal => config
                .daily_word_goal
                .map(|goal| goal.to_string())
                .unwrap_or_default(),
            SettingField::BackupDaily => config.backup_retention.daily.to_string(),
            SettingField::BackupWeekly => config.backup_retention.weekly.to_string(),
            SettingField::BackupMonthly => config.backup_retention.monthly.to_string(),
        }
    }

    fn current_label(self, config: &AppConfig) -> String {
        match self {
            SettingField::SyncTargetPath => config
                .sync_target_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "[unset]".to_string()),
            SettingField::SoundtrackSource => {
                if config.soundtrack_source.trim().is_empty() {
                    "[unset]".to_string()
                } else {
                    config.soundtrack_source.clone()
                }
            }
            _ => self.current_input(config),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingPrompt {
    pub field: SettingField,
    pub input: String,
    pub error: Option<String>,
}

impl SettingPrompt {
    fn new(field: SettingField, config: &AppConfig) -> Self {
        Self {
            field,
            input: field.current_input(config),
            error: None,
        }
    }

    fn wipe(&mut self) {
        self.input.zeroize();
        zeroize_optional_string(&mut self.error);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetadataField {
    Tags,
    People,
    Project,
    Mood,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataPrompt {
    pub tags_input: String,
    pub people_input: String,
    pub project_input: String,
    pub mood_input: String,
    pub active_field: MetadataField,
    pub error: Option<String>,
}

impl MetadataPrompt {
    fn new(metadata: &EntryMetadata) -> Self {
        Self {
            tags_input: metadata.tags.join(", "),
            people_input: metadata.people.join(", "),
            project_input: metadata.project.clone().unwrap_or_default(),
            mood_input: metadata
                .mood
                .map(|mood| mood.to_string())
                .unwrap_or_default(),
            active_field: MetadataField::Tags,
            error: None,
        }
    }

    fn active_input_mut(&mut self) -> &mut String {
        match self.active_field {
            MetadataField::Tags => &mut self.tags_input,
            MetadataField::People => &mut self.people_input,
            MetadataField::Project => &mut self.project_input,
            MetadataField::Mood => &mut self.mood_input,
        }
    }

    fn cycle(&mut self) {
        self.active_field = match self.active_field {
            MetadataField::Tags => MetadataField::People,
            MetadataField::People => MetadataField::Project,
            MetadataField::Project => MetadataField::Mood,
            MetadataField::Mood => MetadataField::Tags,
        };
    }

    fn parse(&self) -> Result<EntryMetadata, String> {
        let tags = parse_metadata_list(&self.tags_input);
        let people = parse_metadata_list(&self.people_input);
        let project = normalize_overlay_text(&self.project_input);
        let mood = if self.mood_input.trim().is_empty() {
            None
        } else {
            let value = self
                .mood_input
                .trim()
                .parse::<u8>()
                .map_err(|_| "Mood must be a number from 0 to 9.".to_string())?;
            if value > 9 {
                return Err("Mood must be a number from 0 to 9.".to_string());
            }
            Some(value)
        };
        Ok(EntryMetadata {
            tags,
            people,
            project,
            mood,
        })
    }

    fn wipe(&mut self) {
        self.tags_input.zeroize();
        self.people_input.zeroize();
        self.project_input.zeroize();
        self.mood_input.zeroize();
        zeroize_optional_string(&mut self.error);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreStage {
    SelectBackup,
    TargetPath,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestorePrompt {
    pub backups: Vec<vault::BackupEntry>,
    pub selected: usize,
    pub target_input: String,
    pub stage: RestoreStage,
    pub error: Option<String>,
}

impl RestorePrompt {
    fn new(backups: Vec<vault::BackupEntry>, selected_date: NaiveDate) -> Self {
        let default_target = dirs::document_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."))
            .join(format!(
                "BlueScreenJournal-Restore-{}",
                selected_date.format("%Y%m%d")
            ));
        Self {
            backups,
            selected: 0,
            target_input: default_target.display().to_string(),
            stage: RestoreStage::SelectBackup,
            error: None,
        }
    }

    fn with_selected_backup(
        backups: Vec<vault::BackupEntry>,
        selected_date: NaiveDate,
        selected_backup_path: &Path,
    ) -> Self {
        let mut prompt = Self::new(backups, selected_date);
        if let Some(index) = prompt
            .backups
            .iter()
            .position(|backup| backup.path == selected_backup_path)
        {
            prompt.selected = index;
        }
        prompt
    }

    fn move_selection(&mut self, delta: isize) {
        if self.backups.is_empty() {
            self.selected = 0;
            return;
        }
        let next = self.selected as isize + delta;
        self.selected = next.clamp(0, self.backups.len() as isize - 1) as usize;
    }

    fn selected_backup(&self) -> Option<&vault::BackupEntry> {
        self.backups.get(self.selected)
    }

    pub fn window(&self, max_rows: usize) -> (usize, usize) {
        if self.backups.is_empty() || max_rows == 0 {
            return (0, 0);
        }
        let max_rows = max_rows.max(1);
        let mut start = self.selected.saturating_sub(max_rows / 2);
        let max_start = self.backups.len().saturating_sub(max_rows);
        if start > max_start {
            start = max_start;
        }
        let end = (start + max_rows).min(self.backups.len());
        (start, end)
    }

    fn wipe(&mut self) {
        for backup in &mut self.backups {
            if let Some(path) = backup.path.to_str() {
                let mut owned = path.to_string();
                owned.zeroize();
            }
        }
        self.backups.clear();
        self.target_input.zeroize();
        zeroize_optional_string(&mut self.error);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    CommandPalette,
    Save,
    Export,
    QuickExportText,
    QuickExportMarkdown,
    ExportHistory,
    BackupHistory,
    BackupCleanupPreview,
    BackupPolicy,
    BackupPruneNow,
    Backup,
    RestoreBackup,
    Dashboard,
    SyncCenter,
    ToggleSoundtrack,
    ReviewMode,
    CheckUpdates,
    DoctorReport,
    Lock,
    Quit,
    Find,
    ClearFind,
    Replace,
    Metadata,
    ClosingThought,
    ToggleFavorite,
    ToggleReveal,
    ToggleTypewriter,
    DuplicateLine,
    DeleteLine,
    MoveLineUp,
    MoveLineDown,
    InsertTimeStamp,
    InsertDateStamp,
    InsertDateTimeStamp,
    InsertDivider,
    InsertBlankAbove,
    InsertBlankBelow,
    JumpTop,
    JumpBottom,
    InsertStatsStamp,
    InsertMetadataStamp,
    GlobalSearch,
    SearchHistory,
    SearchScopeToday,
    SearchScopeWeek,
    SearchScopeMonth,
    SearchScopeYear,
    SearchScopeAll,
    SearchClearFilters,
    FindNext,
    FindPrevious,
    PreviousParagraph,
    NextParagraph,
    RebuildSearchIndex,
    SearchCacheStatus,
    Dates,
    RecentEntries,
    FavoriteEntries,
    PreviousFavorite,
    NextFavorite,
    RandomEntry,
    PreviousEntry,
    NextEntry,
    NewEntry,
    Today,
    Index,
    Sync,
    Verify,
    IntegrityDetails,
    SettingsSummary,
    ReviewPrompts,
    SyncHistory,
    SessionReset,
    About,
    HelpTopics,
    ToggleKeychainMemory,
    QuickStart,
    EditSetting(SettingField),
    Help,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuItem {
    pub label: String,
    pub detail: String,
    pub action: MenuAction,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuState {
    pub selected_menu: MenuId,
    pub selected_item: usize,
}

impl MenuState {
    fn new(selected_menu: MenuId, app: &App) -> Self {
        let mut state = Self {
            selected_menu,
            selected_item: 0,
        };
        state.clamp_selection(app);
        state
    }

    fn move_menu(&mut self, delta: isize, app: &App) {
        let menus = MenuId::all();
        let current = menus
            .iter()
            .position(|menu| *menu == self.selected_menu)
            .unwrap_or(0);
        let next = (current as isize + delta).rem_euclid(menus.len() as isize) as usize;
        self.selected_menu = menus[next];
        self.selected_item = 0;
        self.clamp_selection(app);
    }

    fn jump_to_menu(&mut self, menu: MenuId, app: &App) {
        self.selected_menu = menu;
        self.selected_item = 0;
        self.clamp_selection(app);
    }

    fn move_item(&mut self, delta: isize, app: &App) {
        let items = app.menu_items(self.selected_menu);
        let len = items.len();
        if len == 0 {
            self.selected_item = 0;
            return;
        }
        let direction = if delta >= 0 { 1isize } else { -1isize };
        let mut next = (self.selected_item as isize + delta).clamp(0, len as isize - 1);
        while next >= 0 && next < len as isize {
            if items[next as usize].enabled {
                self.selected_item = next as usize;
                return;
            }
            next += direction;
        }
    }

    fn clamp_selection(&mut self, app: &App) {
        let items = app.menu_items(self.selected_menu);
        let len = items.len();
        if len == 0 {
            self.selected_item = 0;
        } else if self.selected_item >= len {
            self.selected_item = len - 1;
        }

        if !items
            .get(self.selected_item)
            .map(|item| item.enabled)
            .unwrap_or(false)
            && let Some(enabled_idx) = items.iter().position(|item| item.enabled)
        {
            self.selected_item = enabled_idx;
        }
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct IntegrityStatus {
    ok: bool,
    issue_count: usize,
}

impl IntegrityStatus {
    fn label(&self) -> String {
        if self.ok {
            "VERIFY OK".to_string()
        } else if self.issue_count > 0 {
            format!("VERIFY BROKEN {}", self.issue_count)
        } else {
            "VERIFY BROKEN".to_string()
        }
    }
}

#[derive(Clone, Debug)]
enum SyncRequest {
    Folder {
        remote_root: PathBuf,
        target_label: String,
    },
    S3 {
        target_label: String,
    },
    WebDav {
        target_label: String,
    },
}

impl SyncRequest {
    fn backend_label(&self) -> &'static str {
        match self {
            SyncRequest::Folder { .. } => "FOLDER",
            SyncRequest::S3 { .. } => "S3",
            SyncRequest::WebDav { .. } => "WEBDAV",
        }
    }

    fn target_label(&self) -> &str {
        match self {
            SyncRequest::Folder { target_label, .. }
            | SyncRequest::S3 { target_label }
            | SyncRequest::WebDav { target_label } => target_label,
        }
    }
}

impl ReplaceConfirm {
    fn wipe(&mut self) {
        self.find_text.zeroize();
        self.replace_text.zeroize();
        self.matches.clear();
        self.current_idx = 0;
    }
}

impl SearchJump {
    fn wipe(&mut self) {
        self.match_text.zeroize();
    }
}

impl MergeContext {
    fn wipe(&mut self) {
        self.primary_hash.zeroize();
        for hash in &mut self.merged_hashes {
            hash.zeroize();
        }
        self.merged_hashes.clear();
    }
}

#[derive(Clone, Copy, Debug)]
enum SaveKind {
    Saved,
    Autosaved,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct DocumentStats {
    lines: usize,
    words: usize,
    chars: usize,
}

impl From<BufferStats> for DocumentStats {
    fn from(value: BufferStats) -> Self {
        Self {
            lines: value.lines,
            words: value.words,
            chars: value.chars,
        }
    }
}

pub struct App {
    config: AppConfig,
    buffer: TextBuffer,
    closing_thought: Option<String>,
    entry_metadata: EntryMetadata,
    selected_date: NaiveDate,
    scroll_row: usize,
    menu: Option<MenuState>,
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
    integrity_status: Option<IntegrityStatus>,
    search_index: Option<SearchIndex>,
    pending_search_jump: Option<SearchJump>,
    pending_conflict: Option<vault::ConflictState>,
    pending_recovery_closing: Option<Option<String>>,
    pending_recovery_metadata: Option<EntryMetadata>,
    merge_context: Option<MergeContext>,
    pending_sync_request: Option<SyncRequest>,
    reveal_codes: bool,
    last_viewport_height: usize,
    last_viewport_width: usize,
    last_autosave_check: Instant,
    session_started_at: Instant,
    recent_dates: Vec<NaiveDate>,
    search_history: Vec<String>,
    search_scope_from: String,
    search_scope_to: String,
    document_stats: DocumentStats,
    menu_coach_shown: bool,
    soundtrack_child: Option<Child>,
    soundtrack_loop_enabled: bool,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.stop_soundtrack_playback();
    }
}

impl App {
    pub fn new() -> Self {
        Self::with_initial_date(None)
    }

    pub fn with_initial_date(initial_date: Option<NaiveDate>) -> Self {
        let config = AppConfig::load_or_default();
        let initial_buffer = TextBuffer::new();
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

        let mut app = Self {
            config,
            buffer: initial_buffer,
            closing_thought: None,
            entry_metadata: EntryMetadata::default(),
            selected_date: initial_date.unwrap_or_else(|| Local::now().date_naive()),
            scroll_row: 0,
            menu: None,
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
            integrity_status: None,
            search_index: None,
            pending_search_jump: None,
            pending_conflict: None,
            pending_recovery_closing: None,
            pending_recovery_metadata: None,
            merge_context: None,
            pending_sync_request: None,
            reveal_codes: false,
            last_viewport_height: 23,
            last_viewport_width: 80,
            last_autosave_check: Instant::now(),
            session_started_at: Instant::now(),
            recent_dates: Vec::new(),
            search_history: Vec::new(),
            search_scope_from: String::new(),
            search_scope_to: String::new(),
            document_stats: DocumentStats::from(BufferStats {
                lines: 1,
                words: 0,
                chars: 0,
            }),
            menu_coach_shown: false,
            soundtrack_child: None,
            soundtrack_loop_enabled: false,
        };
        app.try_keychain_auto_unlock();
        app
    }

    pub fn buffer(&self) -> &TextBuffer {
        &self.buffer
    }

    pub fn closing_thought(&self) -> Option<&str> {
        self.closing_thought.as_deref()
    }

    pub fn reveal_codes_enabled(&self) -> bool {
        self.reveal_codes
    }

    pub fn scroll_row(&self) -> usize {
        self.scroll_row
    }

    pub fn overlay(&self) -> Option<&Overlay> {
        self.overlay.as_ref()
    }

    pub fn menu(&self) -> Option<&MenuState> {
        self.menu.as_ref()
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

    pub fn header_time_label(&self) -> String {
        let time_format = match (self.config.clock_12h, self.config.show_seconds) {
            (true, true) => "%I:%M:%S %p",
            (true, false) => "%I:%M %p",
            (false, true) => "%H:%M:%S",
            (false, false) => "%H:%M",
        };
        Local::now().format(time_format).to_string()
    }

    pub fn app_version_label(&self) -> String {
        if let Some(sha) = option_env!("BSJ_GIT_SHA") {
            format!("v{}+{}", env!("CARGO_PKG_VERSION"), sha)
        } else {
            format!("v{}", env!("CARGO_PKG_VERSION"))
        }
    }

    pub fn header_entry_focus_label(&self) -> String {
        let today = Local::now().date_naive();
        if self.selected_date == today {
            format!("TODAY {}", self.selected_date.format("%Y-%m-%d"))
        } else if self.selected_date > today {
            format!("NEXT {}", self.selected_date.format("%Y-%m-%d"))
        } else {
            format!("ARCHIVE {}", self.selected_date.format("%Y-%m-%d"))
        }
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
        if self.dirty {
            return match (self.last_save_kind, self.last_save_time) {
                (Some(SaveKind::Autosaved), Some(time)) => {
                    format!("UNSAVED | DRAFT {}", time.format("%H:%M:%S"))
                }
                _ => "UNSAVED CHANGES".to_string(),
            };
        }

        match (self.last_save_kind, self.last_save_time) {
            (Some(SaveKind::Saved), Some(time)) => {
                format!("REVISION SAVED {}", time.format("%H:%M:%S"))
            }
            (Some(SaveKind::Autosaved), Some(time)) => {
                format!("DRAFT AUTOSAVED {}", time.format("%H:%M:%S"))
            }
            _ => "READY".to_string(),
        }
    }

    pub fn draft_recovered_label(&self) -> &'static str {
        if self.draft_recovered {
            "DRAFT RECOVERED"
        } else {
            ""
        }
    }

    pub fn soundtrack_status_label(&self) -> &'static str {
        if self.soundtrack_loop_enabled {
            "THEME ON"
        } else {
            "THEME OFF"
        }
    }

    pub fn enable_soundtrack_autoplay(&mut self) {
        if self.config.soundtrack_source.trim().is_empty() || self.soundtrack_loop_enabled {
            return;
        }
        match self.start_soundtrack_playback() {
            Ok(()) => {
                self.soundtrack_loop_enabled = true;
            }
            Err(error) => {
                self.soundtrack_loop_enabled = false;
                log::warn!("soundtrack autoplay failed: {error}");
            }
        }
    }

    pub fn integrity_status_label(&self) -> String {
        self.integrity_status
            .as_ref()
            .map(IntegrityStatus::label)
            .unwrap_or_default()
    }

    pub fn footer_mode_label(&self) -> &'static str {
        if self.menu.is_some() {
            "MENU"
        } else {
            match self.overlay.as_ref() {
                Some(Overlay::SetupWizard(_)) => "SETUP",
                Some(Overlay::UnlockPrompt { .. }) => "UNLOCK",
                Some(Overlay::Help) => "HELP",
                Some(Overlay::DatePicker(_)) => "DATES",
                Some(Overlay::FindPrompt { .. }) => "FIND",
                Some(Overlay::ClosingPrompt { .. }) => "CLOSING",
                Some(Overlay::ConflictChoice(_)) => "CONFLICT",
                Some(Overlay::MergeDiff(_)) => "MERGE",
                Some(Overlay::Search(_)) => "SEARCH",
                Some(Overlay::ReplacePrompt(_)) | Some(Overlay::ReplaceConfirm(_)) => "REPLACE",
                Some(Overlay::MetadataPrompt(_)) => "META",
                Some(Overlay::ExportPrompt(_)) => "EXPORT",
                Some(Overlay::SettingPrompt(_)) => "SETUP",
                Some(Overlay::Index(_)) => "INDEX",
                Some(Overlay::SyncStatus(_)) => "SYNC",
                Some(Overlay::RestorePrompt(_)) => "RESTORE",
                Some(Overlay::Info(_)) => "INFO",
                Some(Overlay::Picker(_)) => "PICK",
                Some(Overlay::RecoverDraft { .. }) => "RECOVER",
                Some(Overlay::QuitConfirm) => "QUIT",
                None if self.vault.is_some() => "EDIT",
                None => "LOCKED",
            }
        }
    }

    pub fn footer_stats_label(&self) -> String {
        if let Some(goal) = self.config.daily_word_goal {
            format!("Words {}/{}", self.document_stats.words, goal)
        } else {
            format!("Words {}", self.document_stats.words)
        }
    }

    pub fn cursor_status_label(&self) -> String {
        let (row, col) = self.buffer.cursor();
        format!("Line {}, Col {}", row + 1, col + 1)
    }

    pub fn footer_context_label(&self) -> String {
        if let Some(menu) = &self.menu {
            let items = self.menu_items(menu.selected_menu);
            if items.is_empty() {
                return menu.selected_menu.title().to_string();
            }
            return format!(
                "{} menu {}/{}",
                menu.selected_menu.title(),
                menu.selected_item + 1,
                items.len()
            );
        }

        match self.overlay.as_ref() {
            Some(Overlay::DatePicker(picker)) => format!(
                "{} {}",
                if picker.jump_input.trim().is_empty() {
                    picker.selected_date.format("%Y-%m-%d").to_string()
                } else {
                    format!("JUMP {}", picker.jump_input)
                },
                if !picker.jump_input.trim().is_empty() {
                    "INPUT"
                } else if picker.has_entry(picker.selected_date) {
                    "SAVED"
                } else {
                    "BLANK"
                }
            ),
            Some(Overlay::Search(search)) if !search.results.is_empty() => {
                format!(
                    "RESULT {}/{} {}",
                    search.selected + 1,
                    search.results.len(),
                    search.range_label()
                )
            }
            Some(Overlay::Index(index)) if !index.items.is_empty() => {
                format!(
                    "ENTRY {}/{}{}{}",
                    index.selected + 1,
                    index.items.len(),
                    if index.favorites_only { " FAV" } else { "" },
                    if index.conflicts_only {
                        " CONFLICT"
                    } else {
                        ""
                    }
                )
            }
            Some(Overlay::Info(info)) => format!(
                "LINES {}-{}",
                info.scroll + 1,
                (info.scroll + 1).min(info.lines.len())
            ),
            Some(Overlay::Picker(picker)) => {
                let filtered_len = picker.filtered_indices().len();
                if filtered_len == 0 {
                    "NO MATCHES".to_string()
                } else {
                    format!("CHOICE {}/{}", picker.selected + 1, filtered_len)
                }
            }
            _ if !self.find_matches.is_empty() => {
                format!(
                    "FIND {}/{}",
                    self.current_match_idx + 1,
                    self.find_matches.len()
                )
            }
            _ => self.cursor_status_label(),
        }
    }

    pub fn document_stats_label(&self) -> String {
        let stats = self.document_stats;
        if let Some(goal) = self.config.daily_word_goal {
            format!("L{} W{}/{goal} C{}", stats.lines, stats.words, stats.chars)
        } else {
            format!("L{} W{} C{}", stats.lines, stats.words, stats.chars)
        }
    }

    pub fn session_status_label(&self) -> String {
        let elapsed_minutes = self.session_started_at.elapsed().as_secs() / 60;
        format!("SESSION {:>2}M", elapsed_minutes)
    }

    pub fn word_goal_status_label(&self) -> String {
        let Some(goal) = self.config.daily_word_goal else {
            return String::new();
        };
        let words = self.document_word_count();
        format!("GOAL {words}/{goal}")
    }

    pub fn favorite_marker(&self) -> &'static str {
        if self.is_favorite_date(self.selected_date) {
            "*"
        } else {
            ""
        }
    }

    pub fn vault_path_label(&self) -> String {
        self.vault_path.display().to_string()
    }

    pub fn keychain_memory_enabled(&self) -> bool {
        self.config.remember_passphrase_in_keychain
    }

    pub fn empty_state_lines(&self) -> [&'static str; 7] {
        [
            "START TYPING TO WRITE TODAY'S ENTRY",
            "F2 saves a revision. Header changes to 'REVISION SAVED <time>'.",
            "Alt+Right next day  Alt+N next blank new entry",
            "Open older entries through F7 Index or F3 Calendar.",
            "Esc opens menus  Alt+F/E/S/G/T/U/H opens a menu directly",
            "F3 calendar  F7 index  F5 search vault",
            "F10 quit  F12 lock",
        ]
    }

    pub fn show_ruler_enabled(&self) -> bool {
        self.config.show_ruler
    }

    pub fn show_footer_legend_enabled(&self) -> bool {
        self.config.show_footer_legend
    }

    fn setting_menu_item(&self, field: SettingField) -> MenuItem {
        MenuItem {
            label: field.label().to_string(),
            detail: field.current_label(&self.config),
            action: MenuAction::EditSetting(field),
            enabled: true,
        }
    }

    pub fn menu_items(&self, menu: MenuId) -> Vec<MenuItem> {
        match menu {
            MenuId::File => vec![
                MenuItem {
                    label: "Save Entry".to_string(),
                    detail: "F2".to_string(),
                    action: MenuAction::Save,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Export Current".to_string(),
                    detail: "TXT/MD".to_string(),
                    action: MenuAction::Export,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Quick Export Text".to_string(),
                    detail: "AUTO".to_string(),
                    action: MenuAction::QuickExportText,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Quick Export Markdown".to_string(),
                    detail: "AUTO".to_string(),
                    action: MenuAction::QuickExportMarkdown,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Export History".to_string(),
                    detail: "LAST 16".to_string(),
                    action: MenuAction::ExportHistory,
                    enabled: !self.config.export_history.is_empty(),
                },
                MenuItem {
                    label: "Backup Snapshot".to_string(),
                    detail: "NOW".to_string(),
                    action: MenuAction::Backup,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Backup History".to_string(),
                    detail: "LIST".to_string(),
                    action: MenuAction::BackupHistory,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Backup Policy".to_string(),
                    detail: "KEEP".to_string(),
                    action: MenuAction::BackupPolicy,
                    enabled: true,
                },
                MenuItem {
                    label: "Backup Cleanup Preview".to_string(),
                    detail: "KEEP".to_string(),
                    action: MenuAction::BackupCleanupPreview,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Prune Old Backups".to_string(),
                    detail: "APPLY".to_string(),
                    action: MenuAction::BackupPruneNow,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Restore Backup".to_string(),
                    detail: "SAFE".to_string(),
                    action: MenuAction::RestoreBackup,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Lock Vault".to_string(),
                    detail: "F12".to_string(),
                    action: MenuAction::Lock,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Quit Program".to_string(),
                    detail: "F10".to_string(),
                    action: MenuAction::Quit,
                    enabled: true,
                },
            ],
            MenuId::Edit => vec![
                MenuItem {
                    label: "Find in Entry".to_string(),
                    detail: "F4".to_string(),
                    action: MenuAction::Find,
                    enabled: true,
                },
                MenuItem {
                    label: "Clear Find".to_string(),
                    detail: "CLEAR".to_string(),
                    action: MenuAction::ClearFind,
                    enabled: self.find_query.is_some(),
                },
                MenuItem {
                    label: "Replace in Entry".to_string(),
                    detail: "F6".to_string(),
                    action: MenuAction::Replace,
                    enabled: true,
                },
                MenuItem {
                    label: "Entry Metadata".to_string(),
                    detail: "TAGS".to_string(),
                    action: MenuAction::Metadata,
                    enabled: true,
                },
                MenuItem {
                    label: "Closing Thought".to_string(),
                    detail: "F9".to_string(),
                    action: MenuAction::ClosingThought,
                    enabled: true,
                },
                MenuItem {
                    label: "Toggle Favorite".to_string(),
                    detail: if self.is_favorite_date(self.selected_date) {
                        "STAR"
                    } else {
                        "OFF"
                    }
                    .to_string(),
                    action: MenuAction::ToggleFavorite,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Reveal Codes".to_string(),
                    detail: if self.reveal_codes {
                        "ON".to_string()
                    } else {
                        "OFF".to_string()
                    },
                    action: MenuAction::ToggleReveal,
                    enabled: true,
                },
                MenuItem {
                    label: "Typewriter Mode".to_string(),
                    detail: if self.config.typewriter_mode {
                        "ON".to_string()
                    } else {
                        "OFF".to_string()
                    },
                    action: MenuAction::ToggleTypewriter,
                    enabled: true,
                },
                MenuItem {
                    label: "Duplicate Line".to_string(),
                    detail: "COPY".to_string(),
                    action: MenuAction::DuplicateLine,
                    enabled: true,
                },
                MenuItem {
                    label: "Delete Line".to_string(),
                    detail: "DROP".to_string(),
                    action: MenuAction::DeleteLine,
                    enabled: true,
                },
                MenuItem {
                    label: "Move Line Up".to_string(),
                    detail: "SHIFT".to_string(),
                    action: MenuAction::MoveLineUp,
                    enabled: true,
                },
                MenuItem {
                    label: "Move Line Down".to_string(),
                    detail: "SHIFT".to_string(),
                    action: MenuAction::MoveLineDown,
                    enabled: true,
                },
                MenuItem {
                    label: "Insert Time".to_string(),
                    detail: "STAMP".to_string(),
                    action: MenuAction::InsertTimeStamp,
                    enabled: true,
                },
                MenuItem {
                    label: "Insert Date".to_string(),
                    detail: "STAMP".to_string(),
                    action: MenuAction::InsertDateStamp,
                    enabled: true,
                },
                MenuItem {
                    label: "Insert Date+Time".to_string(),
                    detail: "STAMP".to_string(),
                    action: MenuAction::InsertDateTimeStamp,
                    enabled: true,
                },
                MenuItem {
                    label: "Insert Divider".to_string(),
                    detail: "RULE".to_string(),
                    action: MenuAction::InsertDivider,
                    enabled: true,
                },
                MenuItem {
                    label: "Blank Line Above".to_string(),
                    detail: "OPEN".to_string(),
                    action: MenuAction::InsertBlankAbove,
                    enabled: true,
                },
                MenuItem {
                    label: "Blank Line Below".to_string(),
                    detail: "OPEN".to_string(),
                    action: MenuAction::InsertBlankBelow,
                    enabled: true,
                },
                MenuItem {
                    label: "Insert Stats Stamp".to_string(),
                    detail: "L/W/C".to_string(),
                    action: MenuAction::InsertStatsStamp,
                    enabled: true,
                },
                MenuItem {
                    label: "Insert Metadata Stamp".to_string(),
                    detail: "META".to_string(),
                    action: MenuAction::InsertMetadataStamp,
                    enabled: true,
                },
                MenuItem {
                    label: "Jump To Top".to_string(),
                    detail: "START".to_string(),
                    action: MenuAction::JumpTop,
                    enabled: true,
                },
                MenuItem {
                    label: "Jump To Bottom".to_string(),
                    detail: "END".to_string(),
                    action: MenuAction::JumpBottom,
                    enabled: true,
                },
                MenuItem {
                    label: "Previous Paragraph".to_string(),
                    detail: "CTRL+P".to_string(),
                    action: MenuAction::PreviousParagraph,
                    enabled: true,
                },
                MenuItem {
                    label: "Next Paragraph".to_string(),
                    detail: "CTRL+N".to_string(),
                    action: MenuAction::NextParagraph,
                    enabled: true,
                },
            ],
            MenuId::Search => vec![
                MenuItem {
                    label: "Search Vault".to_string(),
                    detail: "F5".to_string(),
                    action: MenuAction::GlobalSearch,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Recent Queries".to_string(),
                    detail: "HIST".to_string(),
                    action: MenuAction::SearchHistory,
                    enabled: !self.search_history.is_empty(),
                },
                MenuItem {
                    label: "Search Today".to_string(),
                    detail: "RANGE".to_string(),
                    action: MenuAction::SearchScopeToday,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Search Last 7 Days".to_string(),
                    detail: "RANGE".to_string(),
                    action: MenuAction::SearchScopeWeek,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Search This Month".to_string(),
                    detail: "RANGE".to_string(),
                    action: MenuAction::SearchScopeMonth,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Search This Year".to_string(),
                    detail: "RANGE".to_string(),
                    action: MenuAction::SearchScopeYear,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Search All Time".to_string(),
                    detail: "RANGE".to_string(),
                    action: MenuAction::SearchScopeAll,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Clear Search Filters".to_string(),
                    detail: "RESET".to_string(),
                    action: MenuAction::SearchClearFilters,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Find Next".to_string(),
                    detail: "DOWN".to_string(),
                    action: MenuAction::FindNext,
                    enabled: self.find_query.is_some(),
                },
                MenuItem {
                    label: "Find Previous".to_string(),
                    detail: "UP".to_string(),
                    action: MenuAction::FindPrevious,
                    enabled: self.find_query.is_some(),
                },
                MenuItem {
                    label: "Rebuild Search Cache".to_string(),
                    detail: "RAM".to_string(),
                    action: MenuAction::RebuildSearchIndex,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Search Cache Status".to_string(),
                    detail: "INFO".to_string(),
                    action: MenuAction::SearchCacheStatus,
                    enabled: self.vault.is_some(),
                },
            ],
            MenuId::Go => vec![
                MenuItem {
                    label: "Open Calendar".to_string(),
                    detail: "F3".to_string(),
                    action: MenuAction::Dates,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Next New Entry".to_string(),
                    detail: "ALT+N".to_string(),
                    action: MenuAction::NewEntry,
                    enabled: true,
                },
                MenuItem {
                    label: "Recent Entries".to_string(),
                    detail: "HIST".to_string(),
                    action: MenuAction::RecentEntries,
                    enabled: !self.recent_dates.is_empty(),
                },
                MenuItem {
                    label: "Favorite Dates".to_string(),
                    detail: "STAR".to_string(),
                    action: MenuAction::FavoriteEntries,
                    enabled: !self.favorite_dates().is_empty(),
                },
                MenuItem {
                    label: "Previous Favorite".to_string(),
                    detail: "STAR".to_string(),
                    action: MenuAction::PreviousFavorite,
                    enabled: !self.favorite_dates().is_empty(),
                },
                MenuItem {
                    label: "Next Favorite".to_string(),
                    detail: "STAR".to_string(),
                    action: MenuAction::NextFavorite,
                    enabled: !self.favorite_dates().is_empty(),
                },
                MenuItem {
                    label: "Random Saved Entry".to_string(),
                    detail: "RND".to_string(),
                    action: MenuAction::RandomEntry,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Previous Saved Entry".to_string(),
                    detail: "OLDER".to_string(),
                    action: MenuAction::PreviousEntry,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Next Saved Entry".to_string(),
                    detail: "NEWER".to_string(),
                    action: MenuAction::NextEntry,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Jump to Today".to_string(),
                    detail: "NOW".to_string(),
                    action: MenuAction::Today,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Index Timeline".to_string(),
                    detail: "F7".to_string(),
                    action: MenuAction::Index,
                    enabled: self.vault.is_some(),
                },
            ],
            MenuId::Tools => vec![
                MenuItem {
                    label: "Command Palette".to_string(),
                    detail: "CTRL+K".to_string(),
                    action: MenuAction::CommandPalette,
                    enabled: true,
                },
                MenuItem {
                    label: "Status Dashboard".to_string(),
                    detail: "LIVE".to_string(),
                    action: MenuAction::Dashboard,
                    enabled: true,
                },
                MenuItem {
                    label: "Sync Center".to_string(),
                    detail: "PLAN".to_string(),
                    action: MenuAction::SyncCenter,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Sync Vault".to_string(),
                    detail: "F8".to_string(),
                    action: MenuAction::Sync,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Soundtrack Source".to_string(),
                    detail: if self.config.soundtrack_source.trim().is_empty() {
                        "SET".to_string()
                    } else {
                        "READY".to_string()
                    },
                    action: MenuAction::EditSetting(SettingField::SoundtrackSource),
                    enabled: true,
                },
                MenuItem {
                    label: "Toggle Soundtrack".to_string(),
                    detail: if self.soundtrack_loop_enabled {
                        "ON"
                    } else if self.config.soundtrack_source.trim().is_empty() {
                        "SET"
                    } else {
                        "OFF"
                    }
                    .to_string(),
                    action: MenuAction::ToggleSoundtrack,
                    enabled: true,
                },
                MenuItem {
                    label: "Verify Integrity".to_string(),
                    detail: self.integrity_status_label(),
                    action: MenuAction::Verify,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Integrity Details".to_string(),
                    detail: "CHAIN".to_string(),
                    action: MenuAction::IntegrityDetails,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Review Mode".to_string(),
                    detail: "LOOK".to_string(),
                    action: MenuAction::ReviewMode,
                    enabled: self.vault.is_some(),
                },
                MenuItem {
                    label: "Writing Prompts".to_string(),
                    detail: "INSERT".to_string(),
                    action: MenuAction::ReviewPrompts,
                    enabled: true,
                },
                MenuItem {
                    label: "Sync History".to_string(),
                    detail: "LAST 10".to_string(),
                    action: MenuAction::SyncHistory,
                    enabled: !self.config.sync_history.is_empty(),
                },
                MenuItem {
                    label: "Check for Updates".to_string(),
                    detail: "GITHUB".to_string(),
                    action: MenuAction::CheckUpdates,
                    enabled: true,
                },
                MenuItem {
                    label: "Doctor Report".to_string(),
                    detail: "CHECK".to_string(),
                    action: MenuAction::DoctorReport,
                    enabled: true,
                },
                MenuItem {
                    label: "Reset Session Timer".to_string(),
                    detail: "ZERO".to_string(),
                    action: MenuAction::SessionReset,
                    enabled: true,
                },
            ],
            MenuId::Setup => vec![
                MenuItem {
                    label: "Settings Summary".to_string(),
                    detail: "REPORT".to_string(),
                    action: MenuAction::SettingsSummary,
                    enabled: true,
                },
                MenuItem {
                    label: "Remember Passphrase".to_string(),
                    detail: if self.config.remember_passphrase_in_keychain {
                        "KEYCHAIN"
                    } else {
                        "OFF"
                    }
                    .to_string(),
                    action: MenuAction::ToggleKeychainMemory,
                    enabled: true,
                },
                self.setting_menu_item(SettingField::VaultPath),
                self.setting_menu_item(SettingField::SyncTargetPath),
                self.setting_menu_item(SettingField::DeviceNickname),
                self.setting_menu_item(SettingField::DailyWordGoal),
                self.setting_menu_item(SettingField::Clock12h),
                self.setting_menu_item(SettingField::ShowSeconds),
                self.setting_menu_item(SettingField::ShowRuler),
                self.setting_menu_item(SettingField::ShowFooterLegend),
                self.setting_menu_item(SettingField::SoundtrackSource),
                self.setting_menu_item(SettingField::BackupDaily),
                self.setting_menu_item(SettingField::BackupWeekly),
                self.setting_menu_item(SettingField::BackupMonthly),
            ],
            MenuId::Help => vec![
                MenuItem {
                    label: "About BlueScreen Journal".to_string(),
                    detail: self.app_version_label(),
                    action: MenuAction::About,
                    enabled: true,
                },
                MenuItem {
                    label: "Key and Menu Guide".to_string(),
                    detail: "F1".to_string(),
                    action: MenuAction::Help,
                    enabled: true,
                },
                MenuItem {
                    label: "Guide Topics".to_string(),
                    detail: "DOCS".to_string(),
                    action: MenuAction::HelpTopics,
                    enabled: true,
                },
                MenuItem {
                    label: "Quick Start".to_string(),
                    detail: "START".to_string(),
                    action: MenuAction::QuickStart,
                    enabled: true,
                },
            ],
        }
    }

    fn open_setting_prompt(&mut self, field: SettingField) {
        self.overlay = Some(Overlay::SettingPrompt(SettingPrompt::new(
            field,
            &self.config,
        )));
    }

    pub fn editor_viewport_height(&self, body_rows: usize) -> usize {
        let reserved_rows = 2usize + usize::from(self.reveal_codes);
        body_rows.saturating_sub(reserved_rows).max(1)
    }

    pub fn reveal_codes_line(&self) -> String {
        let entry_number = self.entry_number_label();
        format_reveal_codes(
            self.selected_date,
            &entry_number,
            &self.entry_metadata,
            &self.buffer.to_text(),
            self.closing_thought.as_deref(),
        )
    }

    pub fn tick(&mut self) {
        self.reap_soundtrack_process();
        if self.soundtrack_loop_enabled
            && self.soundtrack_child.is_none()
            && let Err(error) = self.start_soundtrack_playback()
        {
            log::warn!("soundtrack restart failed: {error}");
            self.soundtrack_loop_enabled = false;
            self.flash_status(&error);
        }

        if self.pending_sync_request.is_some() {
            self.run_pending_sync();
        }

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

    #[cfg(test)]
    pub fn handle_event(&mut self, event: Event, viewport_height: usize) {
        self.handle_event_with_viewport(event, viewport_height, self.last_viewport_width);
    }

    pub fn handle_event_with_viewport(
        &mut self,
        event: Event,
        viewport_height: usize,
        viewport_width: usize,
    ) {
        self.last_viewport_height = viewport_height.max(1);
        self.last_viewport_width = viewport_width.max(1);
        match event {
            Event::Key(key) => self.handle_key(key, viewport_height),
            Event::Resize(width, height) => {
                log::debug!("terminal resized to {}x{}", width, height);
                self.last_viewport_width = width.max(1) as usize;
                self.ensure_cursor_visible(viewport_height);
            }
            _ => {}
        }
    }

    fn handle_key(&mut self, key: KeyEvent, viewport_height: usize) {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return;
        }

        if Self::is_ctrl_char(&key, 'q') {
            self.should_quit = true;
            return;
        }

        if key.code == KeyCode::F(12) {
            self.lock_vault();
            return;
        }

        if self.menu.is_some() {
            self.handle_menu_key(key, viewport_height);
            return;
        }

        if self.overlay.is_some() {
            self.handle_overlay_key(key, viewport_height);
            return;
        }

        if let Some(menu) = Self::menu_ctrl_hotkey(&key) {
            self.open_menu(menu);
            return;
        }

        if let Some(menu) = Self::menu_hotkey(&key) {
            self.open_menu(menu);
            return;
        }

        if key.modifiers.contains(KeyModifiers::ALT) {
            match key.code {
                KeyCode::Right => {
                    self.open_date(self.selected_date + ChronoDuration::days(1));
                    self.flash_status(&format!("DATE {}.", self.selected_date.format("%Y-%m-%d")));
                    return;
                }
                KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&'n') => {
                    self.open_next_new_entry();
                    return;
                }
                KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&'m') => {
                    self.toggle_soundtrack_playback();
                    return;
                }
                _ => {}
            }
        }

        if key.code == KeyCode::Esc || Self::is_ctrl_char(&key, 'g') {
            self.open_menu(MenuId::File);
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
        if key.code == KeyCode::F(9) {
            self.open_closing_prompt();
            return;
        }
        if key.code == KeyCode::F(8) {
            self.begin_sync();
            return;
        }
        if Self::is_ctrl_char(&key, 'k') {
            self.open_command_palette();
            return;
        }
        if Self::is_ctrl_char(&key, 'p') {
            self.buffer.move_paragraph_up();
            self.ensure_cursor_visible(viewport_height);
            return;
        }
        if Self::is_ctrl_char(&key, 'n') {
            self.buffer.move_paragraph_down();
            self.ensure_cursor_visible(viewport_height);
            return;
        }
        if key.code == KeyCode::F(11) || Self::is_ctrl_char(&key, 'r') {
            self.toggle_reveal_codes(viewport_height);
            return;
        }
        if self.try_run_macro(&key, viewport_height) {
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
                self.buffer.insert_text(TAB_INSERT_TEXT);
                mutated = true;
            }
            KeyCode::Char(ch) if Self::is_text_input_key(&key) => {
                if ch == '\t' {
                    self.buffer.insert_text(TAB_INSERT_TEXT);
                } else {
                    self.buffer.insert_char(ch);
                }
                mutated = true;
            }
            _ => {}
        }

        if mutated {
            self.wrap_cursor_line();
            self.dirty = true;
            self.refresh_document_stats();
            self.refresh_find_matches();
        }
        self.ensure_cursor_visible(viewport_height);
    }

    fn handle_menu_key(&mut self, key: KeyEvent, viewport_height: usize) {
        let Some(mut menu) = self.menu.take() else {
            return;
        };
        let mut keep_menu = true;

        match key.code {
            KeyCode::Esc => keep_menu = false,
            KeyCode::Left => menu.move_menu(-1, self),
            KeyCode::Right => menu.move_menu(1, self),
            KeyCode::Tab => menu.move_menu(1, self),
            KeyCode::BackTab => menu.move_menu(-1, self),
            KeyCode::Up => menu.move_item(-1, self),
            KeyCode::Down => menu.move_item(1, self),
            KeyCode::Home => menu.selected_item = 0,
            KeyCode::End => {
                let len = self.menu_items(menu.selected_menu).len();
                if len > 0 {
                    menu.selected_item = len - 1;
                }
            }
            KeyCode::Enter => {
                if let Some(item) = self
                    .menu_items(menu.selected_menu)
                    .get(menu.selected_item)
                    .cloned()
                {
                    if item.enabled {
                        keep_menu = false;
                        self.perform_menu_action(item.action, viewport_height);
                    } else {
                        self.flash_status("UNAVAILABLE.");
                    }
                }
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(target_menu) = MenuId::from_hotkey(ch) {
                    menu.jump_to_menu(target_menu, self);
                }
            }
            _ => {}
        }

        if keep_menu {
            menu.clamp_selection(self);
            self.menu = Some(menu);
        }
    }

    fn handle_overlay_key(&mut self, key: KeyEvent, viewport_height: usize) {
        let Some(mut overlay) = self.overlay.take() else {
            return;
        };
        let mut keep_overlay = true;
        let mut picker_action = None;

        match &mut overlay {
            Overlay::SetupWizard(wizard) => match key.code {
                KeyCode::Esc => {
                    self.flash_status("SETUP ACTIVE. CTRL+Q QUITS.");
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
                    self.flash_status("UNLOCK PROMPT ACTIVE. CTRL+Q QUITS.");
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
                KeyCode::Char('[') => {
                    if let Some(date) =
                        previous_entry_date(&picker.entry_dates, picker.selected_date)
                    {
                        picker.selected_date = date;
                        picker.month = calendar::month_start(date);
                    }
                }
                KeyCode::Char(']') => {
                    if let Some(date) = next_entry_date(&picker.entry_dates, picker.selected_date) {
                        picker.selected_date = date;
                        picker.month = calendar::month_start(date);
                    }
                }
                KeyCode::Char('<') => {
                    if let Some(date) =
                        previous_entry_month(&picker.entry_dates, picker.selected_date)
                    {
                        picker.selected_date = date;
                        picker.month = calendar::month_start(date);
                    }
                }
                KeyCode::Char('>') => {
                    if let Some(date) = next_entry_month(&picker.entry_dates, picker.selected_date)
                    {
                        picker.selected_date = date;
                        picker.month = calendar::month_start(date);
                    }
                }
                KeyCode::Backspace => {
                    if !picker.jump_input.is_empty() {
                        picker.jump_input.pop();
                    }
                }
                KeyCode::Home => {
                    picker.selected_date = calendar::month_start(picker.selected_date);
                    picker.month = calendar::month_start(picker.selected_date);
                }
                KeyCode::End => {
                    picker.selected_date = calendar::shift_date_by_months(
                        calendar::month_start(picker.selected_date),
                        1,
                    ) - ChronoDuration::days(1);
                    picker.month = calendar::month_start(picker.selected_date);
                }
                KeyCode::Char('t') | KeyCode::Char('T') => {
                    picker.selected_date = Local::now().date_naive();
                    picker.month = calendar::month_start(picker.selected_date);
                    picker.jump_input.clear();
                }
                KeyCode::Enter => {
                    let date = match picker.apply_jump_input() {
                        Ok(date) => date,
                        Err(error) => {
                            self.flash_status(&error);
                            picker.selected_date
                        }
                    };
                    self.open_date(date);
                    self.flash_status("DATE SET.");
                    keep_overlay = false;
                }
                KeyCode::Char(ch)
                    if Self::is_text_input_key(&key) && (ch.is_ascii_digit() || ch == '-') =>
                {
                    picker.jump_input.push(ch);
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
            Overlay::ClosingPrompt { input } => match key.code {
                KeyCode::Esc | KeyCode::F(9) => keep_overlay = false,
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Enter => {
                    self.set_closing_thought_from_input(input);
                    keep_overlay = false;
                }
                KeyCode::Char(ch) if Self::is_text_input_key(&key) => {
                    input.push(ch);
                }
                _ => {}
            },
            Overlay::Search(search) => match key.code {
                KeyCode::Esc | KeyCode::F(5) => keep_overlay = false,
                KeyCode::Char('n') if Self::is_ctrl_char(&key, 'n') => {
                    if !search.results.is_empty() {
                        search.active_field = SearchField::Results;
                        search.move_selection(1);
                    }
                }
                KeyCode::Char('p') if Self::is_ctrl_char(&key, 'p') => {
                    if !search.results.is_empty() {
                        search.active_field = SearchField::Results;
                        search.move_selection(-1);
                    }
                }
                KeyCode::Char('r') if Self::is_ctrl_char(&key, 'r') => {
                    if let Some(query) = self.search_history.first().cloned() {
                        search.query_input = query;
                        search.error = None;
                        self.maybe_live_run_global_search(search);
                        self.flash_status("QUERY RECALLED.");
                    } else {
                        self.flash_status("NO QUERY HISTORY.");
                    }
                }
                KeyCode::Char('l') if Self::is_ctrl_char(&key, 'l') => {
                    search.query_input.clear();
                    search.clear_filters();
                    search.clear_results();
                    search.error = None;
                    search.active_field = SearchField::Query;
                    self.remember_search_scope(search);
                    self.flash_status("SEARCH CLEARED.");
                }
                KeyCode::Tab => {
                    search.cycle_field();
                    search.error = None;
                }
                KeyCode::Char('T') => {
                    search.apply_today_scope(self.selected_date);
                    self.maybe_live_run_global_search(search);
                    self.remember_search_scope(search);
                }
                KeyCode::Char('W') => {
                    search.apply_week_scope(self.selected_date);
                    self.maybe_live_run_global_search(search);
                    self.remember_search_scope(search);
                }
                KeyCode::Char('M') => {
                    search.apply_month_scope(self.selected_date);
                    self.maybe_live_run_global_search(search);
                    self.remember_search_scope(search);
                }
                KeyCode::Char('Y') => {
                    search.apply_year_scope(self.selected_date);
                    self.maybe_live_run_global_search(search);
                    self.remember_search_scope(search);
                }
                KeyCode::Char('A') => {
                    search.clear_filters();
                    self.maybe_live_run_global_search(search);
                    self.remember_search_scope(search);
                }
                KeyCode::Char('C') => {
                    search.clear_filters();
                    search.clear_results();
                    self.remember_search_scope(search);
                }
                KeyCode::Backspace => {
                    if let Some(input) = search.active_input_mut() {
                        input.pop();
                        search.clear_results();
                        search.error = None;
                        self.maybe_live_run_global_search(search);
                        self.remember_search_scope(search);
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
                        self.remember_search_query(&search.query_input);
                        self.run_global_search(search, true);
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
                        self.maybe_live_run_global_search(search);
                        self.remember_search_scope(search);
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
                            let find_text = prompt.find_input.trim().to_string();
                            let replace_text = prompt.replace_input.clone();
                            prompt.wipe();
                            self.buffer.set_cursor(
                                matches[current_idx].row,
                                matches[current_idx].start_col,
                            );
                            self.ensure_cursor_visible(viewport_height);
                            overlay = Overlay::ReplaceConfirm(ReplaceConfirm {
                                find_text,
                                replace_text,
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
                    self.wrap_all_lines();
                    self.dirty = replaced > 0;
                    if replaced > 0 {
                        self.refresh_document_stats();
                    }
                    self.refresh_find_matches();
                    self.flash_status(&format!("REPLACED {replaced}."));
                    keep_overlay = false;
                }
                _ => {}
            },
            Overlay::MetadataPrompt(prompt) => match key.code {
                KeyCode::Esc => keep_overlay = false,
                KeyCode::Tab => {
                    prompt.cycle();
                    prompt.error = None;
                }
                KeyCode::Backspace => {
                    prompt.active_input_mut().pop();
                    prompt.error = None;
                }
                KeyCode::Enter => {
                    if self.apply_metadata_prompt(prompt) {
                        keep_overlay = false;
                    }
                }
                KeyCode::Char(ch) if Self::is_text_input_key(&key) => {
                    let accepts_char = match prompt.active_field {
                        MetadataField::Mood => ch.is_ascii_digit(),
                        _ => true,
                    };
                    if accepts_char {
                        prompt.active_input_mut().push(ch);
                        prompt.error = None;
                    }
                }
                _ => {}
            },
            Overlay::ExportPrompt(prompt) => match key.code {
                KeyCode::Esc => keep_overlay = false,
                KeyCode::Tab => prompt.toggle_format(self.selected_date),
                KeyCode::Backspace => {
                    prompt.path_input.pop();
                    prompt.error = None;
                }
                KeyCode::Enter => {
                    if self.apply_export_prompt(prompt) {
                        keep_overlay = false;
                    }
                }
                KeyCode::Char(ch) if Self::is_text_input_key(&key) => {
                    prompt.path_input.push(ch);
                    prompt.error = None;
                }
                _ => {}
            },
            Overlay::SettingPrompt(prompt) => match key.code {
                KeyCode::Esc => keep_overlay = false,
                KeyCode::Backspace => {
                    prompt.input.pop();
                    prompt.error = None;
                }
                KeyCode::Enter => {
                    if self.apply_setting_prompt(prompt) {
                        keep_overlay = false;
                    }
                }
                KeyCode::Char(ch) if Self::is_text_input_key(&key) => {
                    let accepts_char = match prompt.field {
                        SettingField::DailyWordGoal
                        | SettingField::BackupDaily
                        | SettingField::BackupWeekly
                        | SettingField::BackupMonthly => ch.is_ascii_digit(),
                        _ => true,
                    };
                    if accepts_char {
                        prompt.input.push(ch);
                        prompt.error = None;
                    }
                }
                _ => {}
            },
            Overlay::Index(index) => match key.code {
                KeyCode::Esc | KeyCode::F(7) => keep_overlay = false,
                KeyCode::Backspace => index.pop_filter_char(self.selected_date),
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
                KeyCode::Char('t') | KeyCode::Char('T') => {
                    if let Some(position) = index
                        .items
                        .iter()
                        .position(|entry| entry.date == Local::now().date_naive())
                    {
                        index.selected = position;
                    }
                }
                KeyCode::Char('S') => {
                    let selected_date = index.selected_date().unwrap_or(self.selected_date);
                    index.toggle_sort(selected_date);
                }
                KeyCode::Char('F') => {
                    let selected_date = index.selected_date().unwrap_or(self.selected_date);
                    index.toggle_favorites_only(selected_date);
                }
                KeyCode::Char('C') => {
                    let selected_date = index.selected_date().unwrap_or(self.selected_date);
                    index.toggle_conflicts_only(selected_date);
                }
                KeyCode::Char(ch) if Self::is_text_input_key(&key) && !ch.is_control() => {
                    index.push_filter_char(ch, self.selected_date);
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
            Overlay::SyncStatus(sync_status) => match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::F(8) if sync_status.can_close() => {
                    keep_overlay = false;
                }
                _ => {}
            },
            Overlay::RestorePrompt(prompt) => match key.code {
                KeyCode::Esc => keep_overlay = false,
                KeyCode::Tab => {
                    prompt.stage = match prompt.stage {
                        RestoreStage::SelectBackup => RestoreStage::TargetPath,
                        RestoreStage::TargetPath => RestoreStage::SelectBackup,
                    };
                    prompt.error = None;
                }
                KeyCode::Up if prompt.stage == RestoreStage::SelectBackup => {
                    prompt.move_selection(-1)
                }
                KeyCode::Down if prompt.stage == RestoreStage::SelectBackup => {
                    prompt.move_selection(1)
                }
                KeyCode::PageUp if prompt.stage == RestoreStage::SelectBackup => {
                    prompt.move_selection(-(viewport_height.saturating_sub(8) as isize))
                }
                KeyCode::PageDown if prompt.stage == RestoreStage::SelectBackup => {
                    prompt.move_selection(viewport_height.saturating_sub(8) as isize)
                }
                KeyCode::Backspace if prompt.stage == RestoreStage::TargetPath => {
                    prompt.target_input.pop();
                    prompt.error = None;
                }
                KeyCode::Enter => {
                    if prompt.stage == RestoreStage::SelectBackup {
                        prompt.stage = RestoreStage::TargetPath;
                    } else if self.apply_restore_prompt(prompt) {
                        keep_overlay = false;
                    }
                }
                KeyCode::Char(ch)
                    if Self::is_text_input_key(&key)
                        && prompt.stage == RestoreStage::TargetPath =>
                {
                    prompt.target_input.push(ch);
                    prompt.error = None;
                }
                _ => {}
            },
            Overlay::Info(info) => match key.code {
                KeyCode::Esc | KeyCode::Enter => keep_overlay = false,
                KeyCode::Up => info.move_scroll(-1),
                KeyCode::Down => info.move_scroll(1),
                KeyCode::PageUp => info.page_up(viewport_height.saturating_sub(4)),
                KeyCode::PageDown => info.page_down(viewport_height.saturating_sub(4)),
                KeyCode::Home => info.scroll = 0,
                KeyCode::End => {
                    if !info.lines.is_empty() {
                        info.scroll = info.lines.len() - 1;
                    }
                }
                _ => {}
            },
            Overlay::Picker(picker) => match key.code {
                KeyCode::Esc => keep_overlay = false,
                KeyCode::Backspace => picker.pop_filter_char(),
                KeyCode::Up => picker.move_selection(-1),
                KeyCode::Down => picker.move_selection(1),
                KeyCode::PageUp => picker.page_up(viewport_height.saturating_sub(6)),
                KeyCode::PageDown => picker.page_down(viewport_height.saturating_sub(6)),
                KeyCode::Home => picker.selected = 0,
                KeyCode::End => {
                    let filtered_len = picker.filtered_indices().len();
                    if filtered_len > 0 {
                        picker.selected = filtered_len - 1;
                    }
                }
                KeyCode::Enter => {
                    picker_action = picker.selected_item().map(|item| item.action.clone());
                    keep_overlay = false;
                }
                KeyCode::Char(ch) if Self::is_text_input_key(&key) && !ch.is_control() => {
                    picker.push_filter_char(ch);
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

        if let Some(action) = picker_action {
            self.apply_picker_action(action, viewport_height);
        }

        if keep_overlay {
            self.overlay = Some(overlay);
        } else {
            wipe_overlay(&mut overlay);
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
                log::warn!("vault setup failed");
                wizard.error = Some(format!("Setup failed: {error}"));
                return false;
            }
        };

        let mut config = self.config.clone();
        config.vault_path = vault_path.clone();
        config.local_device_id = Some(metadata.device_id.clone());
        if config.device_nickname.trim().is_empty() {
            config.device_nickname = "This Mac".to_string();
        }

        match vault::unlock_vault_with_device(&vault_path, &secret, metadata.device_id.clone()) {
            Ok(unlocked) => {
                log::info!("vault created and unlocked");
                self.vault = Some(unlocked);
                if self.config.remember_passphrase_in_keychain {
                    let _ = platform::store_passphrase(&vault_path, &secret);
                }
                self.search_index = None;
                self.refresh_integrity_status();
                self.vault_path = vault_path.clone();
                self.config = config.clone();
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
                log::warn!("vault unlock after setup failed");
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
        let mut config = self.config.clone();
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
                log::info!("vault unlocked");
                if let Err(register_error) =
                    vault::register_device(&self.vault_path, &device_id, &config.device_nickname)
                {
                    log::warn!("device registration failed");
                    *error = Some(format!("Device registration failed: {register_error}"));
                    return false;
                }
                if config_dirty {
                    config.vault_path = self.vault_path.clone();
                    if let Err(save_error) = config.save() {
                        self.flash_status(&format!("DEVICE CONFIG WARNING: {save_error}"));
                    }
                }
                self.config = config;
                self.vault = Some(unlocked);
                if self.config.remember_passphrase_in_keychain {
                    let _ = platform::store_passphrase(&self.vault_path, &secret);
                }
                self.search_index = None;
                self.refresh_integrity_status();
                self.flash_status("UNLOCKED.");
                self.load_selected_date();
                true
            }
            Err(_) => {
                log::warn!("vault unlock failed");
                *error = Some("Unlock failed. Check passphrase.".to_string());
                false
            }
        }
    }

    fn load_selected_date(&mut self) {
        self.menu = None;
        self.wipe_overlay_state();
        self.wipe_entry_buffer();
        self.wipe_pending_state();
        self.draft_recovered = false;
        self.last_autosave_check = Instant::now();
        self.last_save_kind = None;
        self.last_save_time = None;
        self.remember_recent_date(self.selected_date);

        let Some(vault) = &self.vault else {
            self.buffer = TextBuffer::new();
            self.refresh_document_stats();
            return;
        };

        match vault.load_date_state(self.selected_date) {
            Ok(state) => {
                self.buffer = TextBuffer::from_text(state.revision_text.as_deref().unwrap_or(""));
                self.refresh_document_stats();
                self.closing_thought = state.revision_closing_thought;
                self.entry_metadata = state.revision_entry_metadata;
                self.scroll_row = 0;
                self.dirty = false;
                self.refresh_find_matches();
                self.pending_conflict = state.conflict.clone();
                if let Some(draft_text) = state.recovery_draft_text {
                    self.pending_recovery_closing = Some(state.recovery_draft_closing_thought);
                    self.pending_recovery_metadata = state.recovery_draft_entry_metadata;
                    self.overlay = Some(Overlay::RecoverDraft { draft_text });
                } else if let Some(conflict) = state.conflict {
                    self.overlay = Some(Overlay::ConflictChoice(ConflictOverlay::new(conflict)));
                } else if self.should_show_first_run_guide() {
                    self.mark_first_run_guide_shown();
                    self.open_first_run_guide_overlay();
                }
            }
            Err(_) => {
                self.buffer = TextBuffer::new();
                self.refresh_document_stats();
                self.closing_thought = None;
                self.entry_metadata = EntryMetadata::default();
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
        self.menu = None;
        let entry_dates = self.load_entry_dates();
        self.overlay = Some(Overlay::DatePicker(DatePicker::new(
            self.selected_date,
            entry_dates,
        )));
    }

    fn open_search_overlay(&mut self) {
        self.open_search_overlay_with_query(self.find_query.clone());
    }

    fn open_search_overlay_with_query(&mut self, query: Option<String>) {
        self.menu = None;
        let initial_query = query.or_else(|| self.search_history.first().cloned());
        let mut overlay = SearchOverlay::new(initial_query);
        overlay.from_input = self.search_scope_from.clone();
        overlay.to_input = self.search_scope_to.clone();
        self.overlay = Some(Overlay::Search(overlay));
    }

    fn open_export_prompt(&mut self) {
        self.menu = None;
        self.overlay = Some(Overlay::ExportPrompt(ExportPrompt::new(self.selected_date)));
    }

    fn open_export_prompt_with_preset(&mut self, format: ExportFormatUi, path: String) {
        self.menu = None;
        self.overlay = Some(Overlay::ExportPrompt(ExportPrompt::with_preset(
            self.selected_date,
            format,
            path,
        )));
    }

    fn open_index_overlay(&mut self) {
        self.menu = None;
        let items = self.load_index_entries();
        self.overlay = Some(Overlay::Index(IndexState::new(
            items,
            self.selected_date,
            self.favorite_dates(),
        )));
    }

    fn open_closing_prompt(&mut self) {
        self.menu = None;
        self.overlay = Some(Overlay::ClosingPrompt {
            input: self.closing_thought.clone().unwrap_or_default(),
        });
    }

    fn open_metadata_prompt(&mut self) {
        self.menu = None;
        self.overlay = Some(Overlay::MetadataPrompt(MetadataPrompt::new(
            &self.entry_metadata,
        )));
    }

    fn open_menu(&mut self, menu: MenuId) {
        self.menu = Some(MenuState::new(menu, self));
        if !self.menu_coach_shown {
            self.menu_coach_shown = true;
            self.flash_status("MENU OPEN. ARROWS MOVE, ENTER SELECTS.");
        }
    }

    fn open_info_overlay(&mut self, title: impl Into<String>, text: String) {
        self.menu = None;
        self.overlay = Some(Overlay::Info(InfoOverlay::from_text(title, text)));
    }

    fn open_picker_overlay(&mut self, overlay: PickerOverlay) {
        self.menu = None;
        self.overlay = Some(Overlay::Picker(overlay));
    }

    fn document_word_count(&self) -> usize {
        self.document_stats.words
    }

    fn refresh_document_stats(&mut self) {
        self.document_stats = self.buffer.stats().into();
    }

    fn favorite_dates(&self) -> BTreeSet<NaiveDate> {
        self.config
            .favorite_dates
            .iter()
            .filter_map(|date| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
            .collect()
    }

    fn save_favorite_dates(&mut self, favorites: BTreeSet<NaiveDate>) {
        self.config.favorite_dates = favorites
            .into_iter()
            .map(|date| date.format("%Y-%m-%d").to_string())
            .collect();
        let _ = self.config.save();
    }

    fn is_favorite_date(&self, date: NaiveDate) -> bool {
        self.favorite_dates().contains(&date)
    }

    fn toggle_favorite_current_date(&mut self) {
        let mut favorites = self.favorite_dates();
        let message = if favorites.contains(&self.selected_date) {
            favorites.remove(&self.selected_date);
            "FAVORITE CLEARED."
        } else {
            favorites.insert(self.selected_date);
            "FAVORITE SAVED."
        };
        self.save_favorite_dates(favorites);
        self.flash_status(message);
    }

    fn remember_recent_date(&mut self, date: NaiveDate) {
        self.recent_dates.retain(|existing| *existing != date);
        self.recent_dates.insert(0, date);
        self.recent_dates.truncate(16);
    }

    fn remember_search_query(&mut self, query: &str) {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return;
        }
        self.search_history.retain(|existing| existing != trimmed);
        self.search_history.insert(0, trimmed.to_string());
        self.search_history.truncate(12);
    }

    fn remember_search_scope(&mut self, search: &SearchOverlay) {
        self.search_scope_from.zeroize();
        self.search_scope_to.zeroize();
        self.search_scope_from = search.from_input.clone();
        self.search_scope_to = search.to_input.clone();
    }

    fn record_export_history(&mut self, format: ExportFormatUi, path: &Path) {
        let record = RecentExportInfo {
            timestamp: Local::now().to_rfc3339(),
            date: self.selected_date.format("%Y-%m-%d").to_string(),
            format: format.label().to_ascii_lowercase(),
            path: path.display().to_string(),
        };
        self.config
            .export_history
            .retain(|existing| existing.path != record.path);
        self.config.export_history.insert(0, record);
        self.config.export_history.truncate(16);
        let _ = self.config.save();
    }

    fn finish_buffer_menu_edit(&mut self, viewport_height: usize, status: &str) {
        self.wrap_cursor_and_previous_line();
        self.dirty = true;
        self.refresh_document_stats();
        self.refresh_find_matches();
        self.ensure_cursor_visible(viewport_height);
        self.flash_status(status);
    }

    fn current_time_string(&self) -> String {
        if self.config.clock_12h {
            Local::now().format("%I:%M %p").to_string()
        } else {
            Local::now().format("%H:%M").to_string()
        }
    }

    fn current_timestamp_string(&self) -> String {
        if self.config.clock_12h {
            Local::now().format("%Y-%m-%d %I:%M %p").to_string()
        } else {
            Local::now().format("%Y-%m-%d %H:%M").to_string()
        }
    }

    fn metadata_stamp(&self) -> String {
        let mut parts = Vec::new();
        if !self.entry_metadata.tags.is_empty() {
            parts.push(format!("tags={}", self.entry_metadata.tags.join(",")));
        }
        if !self.entry_metadata.people.is_empty() {
            parts.push(format!("people={}", self.entry_metadata.people.join(",")));
        }
        if let Some(project) = self.entry_metadata.project.as_deref() {
            parts.push(format!("project={project}"));
        }
        if let Some(mood) = self.entry_metadata.mood {
            parts.push(format!("mood={mood}"));
        }
        if parts.is_empty() {
            "[metadata: none]\n".to_string()
        } else {
            format!("[metadata: {}]\n", parts.join(" | "))
        }
    }

    fn insert_text_snippet(&mut self, text: &str, viewport_height: usize, status: &str) {
        self.buffer.insert_text(text);
        self.finish_buffer_menu_edit(viewport_height, status);
    }

    fn wrap_cursor_line(&mut self) {
        self.buffer
            .wrap_current_line(self.last_viewport_width.max(1));
    }

    fn wrap_cursor_and_previous_line(&mut self) {
        let row = self.buffer.cursor_row();
        if row > 0 {
            self.wrap_line_at(row - 1);
        }
        self.wrap_cursor_line();
    }

    fn wrap_line_at(&mut self, row: usize) {
        if self.buffer.line_count() == 0 {
            return;
        }
        let width = self.last_viewport_width.max(1);
        let (saved_row, saved_col) = self.buffer.cursor();
        let target_row = row.min(self.buffer.line_count().saturating_sub(1));
        let target_col = self
            .buffer
            .line(target_row)
            .map(|line| line.chars().count())
            .unwrap_or(0);
        self.buffer.set_cursor(target_row, target_col);
        self.buffer.wrap_current_line(width);
        let clamped_row = saved_row.min(self.buffer.line_count().saturating_sub(1));
        self.buffer.set_cursor(clamped_row, saved_col);
    }

    fn wrap_all_lines(&mut self) {
        let width = self.last_viewport_width.max(1);
        let (saved_row, saved_col) = self.buffer.cursor();
        let mut row = 0usize;
        while row < self.buffer.line_count() {
            let line_len = self
                .buffer
                .line(row)
                .map(|line| line.chars().count())
                .unwrap_or(0);
            if line_len > width {
                self.buffer.set_cursor(row, line_len);
                self.buffer.wrap_current_line(width);
            } else {
                row += 1;
            }
        }
        let clamped_row = saved_row.min(self.buffer.line_count().saturating_sub(1));
        self.buffer.set_cursor(clamped_row, saved_col);
    }

    fn quick_export_current(&mut self, format: ExportFormatUi) {
        if self.vault.is_none() {
            self.flash_status("LOCKED.");
            return;
        }
        let path = default_export_path(self.selected_date, format);
        let entry_number = self.entry_number_label();
        let rendered = match format {
            ExportFormatUi::PlainText => {
                vault::format_export_text(&self.buffer.to_text(), self.closing_thought.as_deref())
            }
            ExportFormatUi::Markdown => render_markdown_export(
                self.selected_date,
                &entry_number,
                &self.entry_metadata,
                &self.buffer.to_text(),
                self.closing_thought.as_deref(),
            ),
        };

        match secure_fs::atomic_write_restricted(&path, rendered.as_bytes()) {
            Ok(()) => {
                self.record_export_history(format, &path);
                let file_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("export");
                self.flash_status(&format!("PLAINTEXT EXPORTED {file_name}."));
            }
            Err(error) => self.flash_status(&format!("EXPORT FAILED: {error}")),
        }
    }

    fn open_export_history_overlay(&mut self) {
        if self.config.export_history.is_empty() {
            self.flash_status("NO EXPORT HISTORY.");
            return;
        }
        let items = self
            .config
            .export_history
            .iter()
            .map(|entry| {
                let file_name = Path::new(&entry.path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("export");
                let format = parse_export_history_format(&entry.format);
                PickerItem {
                    title: format!("{file_name} {}", format.label()),
                    detail: format!("{} {}", entry.date, truncate_for_picker(&entry.path, 30)),
                    keywords: format!(
                        "export history {} {} {} {}",
                        entry.date, entry.timestamp, entry.format, entry.path
                    ),
                    action: PickerAction::OpenExportPrompt {
                        format,
                        path: entry.path.clone(),
                    },
                }
            })
            .collect::<Vec<_>>();
        self.open_picker_overlay(PickerOverlay::new(
            "Export History",
            items,
            "No exports recorded yet.",
        ));
    }

    fn open_adjacent_favorite(&mut self, delta: isize) {
        let favorites = self.favorite_dates().into_iter().collect::<Vec<_>>();
        if favorites.is_empty() {
            self.flash_status("NO FAVORITES.");
            return;
        }

        let current_idx = favorites
            .iter()
            .position(|date| *date == self.selected_date)
            .map(|idx| idx as isize)
            .unwrap_or(if delta < 0 {
                favorites.len() as isize
            } else {
                -1
            });

        let next_idx = current_idx + delta;
        if !(0..favorites.len() as isize).contains(&next_idx) {
            self.flash_status("NO MORE FAVORITES.");
            return;
        }

        self.open_date(favorites[next_idx as usize]);
        self.flash_status("FAVORITE OPENED.");
    }

    fn open_random_saved_entry(&mut self) {
        let Some(vault) = &self.vault else {
            self.flash_status("LOCKED.");
            return;
        };
        let dates = match vault.list_entry_dates() {
            Ok(dates) => dates,
            Err(error) => {
                self.flash_status(&format!("INDEX FAILED: {error}"));
                return;
            }
        };
        if dates.is_empty() {
            self.flash_status("NO SAVED ENTRIES.");
            return;
        }
        let index = (Local::now().timestamp_subsec_nanos() as usize) % dates.len();
        self.open_date(dates[index]);
        self.flash_status("RANDOM ENTRY.");
    }

    fn build_command_palette_items(&self) -> Vec<PickerItem> {
        let mut items = Vec::new();
        for menu in MenuId::all() {
            for item in self
                .menu_items(*menu)
                .into_iter()
                .filter(|item| item.enabled)
            {
                items.push(PickerItem {
                    title: item.label,
                    detail: format!("{} {}", menu.title(), item.detail),
                    keywords: format!("{} {}", menu.title(), item.detail),
                    action: PickerAction::Menu(item.action),
                });
            }
        }
        items
    }

    fn open_command_palette(&mut self) {
        let items = self.build_command_palette_items();
        self.open_picker_overlay(PickerOverlay::new(
            "Command Palette",
            items,
            "No commands match.",
        ));
    }

    fn open_recent_entries_overlay(&mut self) {
        if self.recent_dates.is_empty() {
            self.flash_status("NO RECENT DATES.");
            return;
        }
        let items = self
            .recent_dates
            .iter()
            .map(|date| PickerItem {
                title: date.format("%Y-%m-%d").to_string(),
                detail: if *date == self.selected_date {
                    "CURRENT".to_string()
                } else {
                    "RECENT".to_string()
                },
                keywords: "recent date history".to_string(),
                action: PickerAction::OpenDate(*date),
            })
            .collect::<Vec<_>>();
        self.open_picker_overlay(PickerOverlay::new(
            "Recent Entries",
            items,
            "No recent entries yet.",
        ));
    }

    fn open_favorite_entries_overlay(&mut self) {
        let favorites = self.favorite_dates();
        if favorites.is_empty() {
            self.flash_status("NO FAVORITES.");
            return;
        }

        let items = if let Some(vault) = &self.vault {
            match vault.list_index_entries(INDEX_PREVIEW_CHARS) {
                Ok(entries) => entries
                    .into_iter()
                    .filter(|entry| favorites.contains(&entry.date))
                    .map(|entry| PickerItem {
                        title: entry.date.format("%Y-%m-%d").to_string(),
                        detail: format!("{} {}", entry.entry_number, entry.preview),
                        keywords: "favorite starred bookmarked".to_string(),
                        action: PickerAction::OpenDate(entry.date),
                    })
                    .collect::<Vec<_>>(),
                Err(_) => favorites
                    .iter()
                    .rev()
                    .map(|date| PickerItem {
                        title: date.format("%Y-%m-%d").to_string(),
                        detail: "FAVORITE".to_string(),
                        keywords: "favorite starred bookmarked".to_string(),
                        action: PickerAction::OpenDate(*date),
                    })
                    .collect::<Vec<_>>(),
            }
        } else {
            favorites
                .iter()
                .rev()
                .map(|date| PickerItem {
                    title: date.format("%Y-%m-%d").to_string(),
                    detail: "FAVORITE".to_string(),
                    keywords: "favorite starred bookmarked".to_string(),
                    action: PickerAction::OpenDate(*date),
                })
                .collect::<Vec<_>>()
        };

        self.open_picker_overlay(PickerOverlay::new(
            "Favorite Dates",
            items,
            "No favorites yet.",
        ));
    }

    fn open_search_history_overlay(&mut self) {
        if self.search_history.is_empty() {
            self.flash_status("NO SEARCH HISTORY.");
            return;
        }
        let items = self
            .search_history
            .iter()
            .map(|query| PickerItem {
                title: query.clone(),
                detail: "REUSE QUERY".to_string(),
                keywords: "search query history".to_string(),
                action: PickerAction::OpenSearch(query.clone()),
            })
            .collect::<Vec<_>>();
        self.open_picker_overlay(PickerOverlay::new(
            "Recent Queries",
            items,
            "No search history yet.",
        ));
    }

    fn open_review_prompts_overlay(&mut self) {
        let items = vec![
            (
                "Morning Inventory",
                "What is demanding attention right now?",
                "What is demanding attention right now?\n\n",
            ),
            (
                "One Honest Paragraph",
                "Write the thing you are avoiding in one paragraph.",
                "What am I avoiding saying to myself?\n\n",
            ),
            (
                "Three Small Facts",
                "Capture three concrete details from today.",
                "Three small facts from today:\n1. \n2. \n3. \n\n",
            ),
            (
                "State of Mind",
                "Name the mood, cause, and what would help next.",
                "Mood:\nCause:\nWhat would help next:\n\n",
            ),
            (
                "Closing Pass",
                "Wrap the entry and leave yourself a handoff.",
                "Before I stop tonight, I want tomorrow-me to remember:\n\n",
            ),
        ]
        .into_iter()
        .map(|(title, detail, text)| PickerItem {
            title: title.to_string(),
            detail: detail.to_string(),
            keywords: "prompt review journal writing".to_string(),
            action: PickerAction::InsertText(text.to_string()),
        })
        .collect::<Vec<_>>();

        self.open_picker_overlay(PickerOverlay::new(
            "Writing Prompts",
            items,
            "No prompts available.",
        ));
    }

    fn open_sync_history_overlay(&mut self) {
        if self.config.sync_history.is_empty() {
            self.flash_status("NO SYNC HISTORY.");
            return;
        }
        let items = self
            .config
            .sync_history
            .iter()
            .map(|entry| {
                let detail = if let Some(error) = &entry.error {
                    format!(
                        "{} ERR {}",
                        entry.backend,
                        error.chars().take(24).collect::<String>()
                    )
                } else {
                    format!(
                        "{} P{} U{} C{}",
                        entry.backend, entry.pulled, entry.pushed, entry.conflicts
                    )
                };
                let text = [
                    format!("When      : {}", entry.timestamp),
                    format!("Backend   : {}", entry.backend),
                    format!("Target    : {}", entry.target),
                    format!("Pulled    : {}", entry.pulled),
                    format!("Pushed    : {}", entry.pushed),
                    format!("Conflicts : {}", entry.conflicts),
                    format!(
                        "Integrity : {}",
                        if entry.integrity_ok { "OK" } else { "BROKEN" }
                    ),
                    format!(
                        "Status    : {}",
                        entry.error.as_deref().unwrap_or("success")
                    ),
                ]
                .join("\n");
                PickerItem {
                    title: entry.timestamp.clone(),
                    detail,
                    keywords: format!("sync history {} {}", entry.backend, entry.target),
                    action: PickerAction::ShowInfo {
                        title: "Sync Record".to_string(),
                        text,
                    },
                }
            })
            .collect::<Vec<_>>();
        self.open_picker_overlay(PickerOverlay::new(
            "Sync History",
            items,
            "No sync history yet.",
        ));
    }

    fn open_quickstart_overlay(&mut self) {
        self.open_info_overlay("Quick Start", help::render_quickstart_guide());
    }

    fn open_about_overlay(&mut self) {
        let soundtrack_source = if self.config.soundtrack_source.trim().is_empty() {
            "[not configured]".to_string()
        } else {
            self.config.soundtrack_source.clone()
        };
        let text = [
            format!("BlueScreen Journal {}", self.app_version_label()),
            String::new(),
            "(c) 2026 Awassee LLC and Sean Heiney".to_string(),
            "sean@sean.net".to_string(),
            String::new(),
            "Menus".to_string(),
            "  HELP  -> About BlueScreen Journal".to_string(),
            "  TOOLS -> Soundtrack Source".to_string(),
            "  TOOLS -> Toggle Soundtrack".to_string(),
            String::new(),
            format!("Soundtrack source: {soundtrack_source}"),
            format!("Soundtrack state : {}", self.soundtrack_status_label()),
            "Shortcut        : Alt+M toggles soundtrack".to_string(),
            String::new(),
            "Tip: if soundtrack is not configured, selecting Toggle Soundtrack opens setup."
                .to_string(),
        ]
        .join("\n");
        self.open_info_overlay("About", text);
    }

    fn open_help_topics_overlay(&mut self) {
        let items = vec![
            (
                "About",
                "Version, credits, and controls",
                [
                    format!("BlueScreen Journal {}", self.app_version_label()),
                    "(c) 2026 Awassee LLC and Sean Heiney".to_string(),
                    "sean@sean.net".to_string(),
                    String::new(),
                    "Help path: HELP -> About BlueScreen Journal".to_string(),
                    "Soundtrack: TOOLS -> Soundtrack Source / Toggle Soundtrack".to_string(),
                    "Shortcut: Alt+M".to_string(),
                ]
                .join("\n"),
            ),
            (
                "Quick Start",
                "Daily use in five minutes",
                help::render_quickstart_guide(),
            ),
            ("Docs Hub", "Top-level guide map", help::render_docs_hub()),
            (
                "Setup",
                "Install and first-run setup",
                help::render_setup_guide(
                    &config::config_file_path()
                        .unwrap_or_else(|_| PathBuf::from("~/.config/bsj/config.json")),
                    &default_vault_path(),
                    &logging::log_file_path(),
                ),
            ),
            (
                "Sync",
                "Folder, S3, and WebDAV guidance",
                help::render_sync_guide(),
            ),
            (
                "Backup",
                "Backup and restore workflow",
                help::render_backup_restore_guide(),
            ),
            (
                "Macros",
                "Macro definitions and built-ins",
                help::render_macro_guide(),
            ),
            (
                "Privacy",
                "Data handling and trust model",
                help::render_privacy_guide(),
            ),
            (
                "Terminal",
                "Terminal.app and iTerm2 usage",
                help::render_terminal_guide(),
            ),
            (
                "Troubleshooting",
                "Recovery and problem solving",
                help::render_troubleshooting_guide(),
            ),
            (
                "Product",
                "Feature and value overview",
                help::render_product_guide(),
            ),
            ("Datasheet", "Capability snapshot", help::render_datasheet()),
            ("FAQ", "Common objections and answers", help::render_faq()),
            ("Support", "How to get help", help::render_support()),
            (
                "Distribution",
                "Packaging and release notes",
                help::render_distribution_guide(),
            ),
        ]
        .into_iter()
        .map(|(title, detail, text)| PickerItem {
            title: title.to_string(),
            detail: detail.to_string(),
            keywords: format!("guide help docs {title} {detail}"),
            action: PickerAction::ShowInfo {
                title: title.to_string(),
                text,
            },
        })
        .collect::<Vec<_>>();
        self.open_picker_overlay(PickerOverlay::new(
            "Help Topics",
            items,
            "No guide topics available.",
        ));
    }

    fn open_first_run_guide_overlay(&mut self) {
        let lines = vec![
            "WELCOME TO BLUESCREEN JOURNAL".to_string(),
            String::new(),
            "First 2 minutes:".to_string(),
            "1. Type immediately into today's entry.".to_string(),
            "2. Save with F2 or FILE -> Save Entry.".to_string(),
            "3. Press Esc to open menus if you don't remember keys.".to_string(),
            "4. Press Alt+N to jump to the next blank new-entry date.".to_string(),
            "5. Use GO -> Open Calendar or Index Timeline for older entries.".to_string(),
            "6. Use SEARCH -> Search Vault to find older entries fast.".to_string(),
            "7. Use FILE -> Backup Snapshot before major travel or changes.".to_string(),
            String::new(),
            "The app autosaves encrypted drafts, but manual Save creates history.".to_string(),
            "F1 opens the key cheatsheet. HELP has first-run and operator guides.".to_string(),
        ];
        self.open_info_overlay("First 2 Minutes", lines.join("\n"));
    }

    fn open_settings_summary_overlay(&mut self) {
        let config_path = match config::config_file_path() {
            Ok(path) => path,
            Err(error) => {
                self.flash_status(&format!("SETTINGS PATH ERROR: {error}"));
                return;
            }
        };
        let config_exists = config_path.exists();
        let log_path = logging::log_file_path();
        let env = EnvironmentSettings::capture();
        let (vault_metadata, _) = if vault::vault_exists(&self.config.vault_path) {
            match vault::load_vault_metadata(&self.config.vault_path) {
                Ok(metadata) => (Some(metadata), None::<String>),
                Err(_) => (None, None),
            }
        } else {
            (None, None)
        };

        let report = help::render_settings_report(
            &config_path,
            config_exists,
            &self.config,
            &log_path,
            &env,
            vault_metadata.as_ref(),
        );
        self.open_info_overlay("Settings Summary", report);
    }

    fn open_doctor_overlay(&mut self) {
        let config_path = match config::config_file_path() {
            Ok(path) => path,
            Err(error) => {
                self.flash_status(&format!("DOCTOR CONFIG ERROR: {error}"));
                return;
            }
        };
        let config_exists = config_path.exists();
        let log_path = logging::log_file_path();
        let env = EnvironmentSettings::capture();
        let (vault_metadata, vault_metadata_error) = if vault::vault_exists(&self.config.vault_path)
        {
            match vault::load_vault_metadata(&self.config.vault_path) {
                Ok(metadata) => (Some(metadata), None),
                Err(error) => (None, Some(error.to_string())),
            }
        } else {
            (None, None)
        };

        let (integrity_report, unlock_error, entry_count, backup_count, conflict_count) =
            if let Some(vault) = &self.vault {
                let integrity = vault.verify_integrity().ok();
                let entries = vault.list_entry_dates().ok().map(|dates| dates.len());
                let backups = vault.list_backups().ok().map(|entries| entries.len());
                let conflicts = vault.list_conflicted_dates().ok().map(|dates| dates.len());
                (integrity, None, entries, backups, conflicts)
            } else if vault::vault_exists(&self.config.vault_path) {
                (
                    None,
                    Some("vault locked; unlock to run integrity and content checks".to_string()),
                    None,
                    None,
                    None,
                )
            } else {
                (None, None, None, None, None)
            };

        let report = doctor::build_report(doctor::DoctorInputs {
            config_path: &config_path,
            config_exists,
            config_error: None,
            config: &self.config,
            log_path: &log_path,
            env: &env,
            vault_exists: vault::vault_exists(&self.config.vault_path),
            vault_metadata: vault_metadata.as_ref(),
            vault_metadata_error: vault_metadata_error.as_deref(),
            integrity_report: integrity_report.as_ref(),
            unlock_error: unlock_error.as_deref(),
            entry_count,
            backup_count,
            conflict_count,
        });

        self.open_info_overlay("Doctor Report", doctor::render_text(&report));
    }

    fn open_sync_center_overlay(&mut self) {
        let Some(vault) = &self.vault else {
            self.flash_status("LOCKED.");
            return;
        };

        let conflicts = vault
            .list_conflicted_dates()
            .map(|dates| dates.len())
            .unwrap_or(0);
        let dirty_state = if self.dirty {
            "draft dirty"
        } else {
            "draft clean"
        };
        let (backend_label, target_label, preview_detail, preview_text) =
            match self.resolve_sync_request() {
                Ok(request) => match self.sync_preview_report(&request) {
                    Ok(preview) => (
                        request.backend_label().to_string(),
                        request.target_label().to_string(),
                        format!(
                            "up {} down {} shared {}",
                            preview.local_only_revisions,
                            preview.remote_only_revisions,
                            preview.shared_revisions
                        ),
                        [
                            format!("Backend      : {}", request.backend_label()),
                            format!("Target       : {}", request.target_label()),
                            format!("Local revs   : {}", preview.local_revisions),
                            format!("Remote revs  : {}", preview.remote_revisions),
                            format!("Upload queue : {}", preview.local_only_revisions),
                            format!("Download q   : {}", preview.remote_only_revisions),
                            format!("Shared revs  : {}", preview.shared_revisions),
                            format!("Conflicts    : {conflicts}"),
                            format!("Dirty draft  : {}", if self.dirty { "YES" } else { "NO" }),
                            format!(
                                "Last sync    : {}",
                                self.config
                                    .last_sync
                                    .as_ref()
                                    .map(|sync| format!(
                                        "{} {} +{} / -{}",
                                        sync.timestamp, sync.backend, sync.pushed, sync.pulled
                                    ))
                                    .unwrap_or_else(|| "never".to_string())
                            ),
                        ]
                        .join("\n"),
                    ),
                    Err(error) => (
                        request.backend_label().to_string(),
                        request.target_label().to_string(),
                        format!("preview unavailable: {error}"),
                        [
                            format!("Backend      : {}", request.backend_label()),
                            format!("Target       : {}", request.target_label()),
                            format!("Preview      : {error}"),
                            format!("Conflicts    : {conflicts}"),
                            format!("Dirty draft  : {}", if self.dirty { "YES" } else { "NO" }),
                        ]
                        .join("\n"),
                    ),
                },
                Err(error) => (
                    "UNCONFIGURED".to_string(),
                    "Set a sync target first".to_string(),
                    "configure a target".to_string(),
                    [
                        format!("Backend      : {error}"),
                        format!("Conflicts    : {conflicts}"),
                        format!("Dirty draft  : {}", if self.dirty { "YES" } else { "NO" }),
                    ]
                    .join("\n"),
                ),
            };
        let last_sync_detail = self
            .config
            .last_sync
            .as_ref()
            .map(|sync| {
                format!(
                    "{} {} +{} / -{}",
                    sync.timestamp, sync.backend, sync.pushed, sync.pulled
                )
            })
            .unwrap_or_else(|| "never".to_string());
        let items = vec![
            PickerItem {
                title: "Run Encrypted Sync".to_string(),
                detail: format!("{backend_label} {preview_detail}"),
                keywords: format!("sync run upload download {backend_label} {target_label}"),
                action: PickerAction::Menu(MenuAction::Sync),
            },
            PickerItem {
                title: "Show Sync Snapshot".to_string(),
                detail: format!("{dirty_state} | conflicts {conflicts}"),
                keywords: format!("sync preview snapshot conflicts {target_label}"),
                action: PickerAction::ShowInfo {
                    title: "Sync Snapshot".to_string(),
                    text: preview_text,
                },
            },
            PickerItem {
                title: "Recent Sync Runs".to_string(),
                detail: truncate_for_picker(&last_sync_detail, 40),
                keywords: "sync history runs last sync".to_string(),
                action: PickerAction::Menu(MenuAction::SyncHistory),
            },
            PickerItem {
                title: "Doctor Report".to_string(),
                detail: format!("{conflicts} conflict(s) | {dirty_state}"),
                keywords: "doctor diagnostics sync health".to_string(),
                action: PickerAction::Menu(MenuAction::DoctorReport),
            },
            PickerItem {
                title: "Settings Summary".to_string(),
                detail: truncate_for_picker(&target_label, 40),
                keywords: "settings sync target backend".to_string(),
                action: PickerAction::Menu(MenuAction::SettingsSummary),
            },
            PickerItem {
                title: "Verify Integrity".to_string(),
                detail: self.integrity_status_label(),
                keywords: "verify integrity hashchain".to_string(),
                action: PickerAction::Menu(MenuAction::IntegrityDetails),
            },
        ];
        self.open_picker_overlay(PickerOverlay::new(
            "Sync Center",
            items,
            "No sync actions available.",
        ));
    }

    fn open_review_overlay(&mut self) {
        let Some(vault) = &self.vault else {
            self.flash_status("LOCKED.");
            return;
        };
        match vault.review_summary(Local::now().date_naive()) {
            Ok(review) => {
                let all_index_entries = vault.list_index_entries(42).unwrap_or_default();
                let recent_entries = all_index_entries
                    .iter()
                    .take(5)
                    .cloned()
                    .collect::<Vec<_>>();
                let mood_counts = top_mood_counts(
                    all_index_entries
                        .iter()
                        .filter_map(|entry| entry.metadata.mood),
                );
                let mut lines = vec![
                    format!("Total entries : {}", review.total_entries),
                    format!("Streak        : {} day(s)", review.streak_days),
                    format!("This week     : {}", review.entries_this_week),
                    format!("This month    : {}", review.entries_this_month),
                    format!("Favorites     : {}", self.favorite_dates().len()),
                    String::new(),
                    "On This Day".to_string(),
                ];
                if review.on_this_day.is_empty() {
                    lines.push("  No prior entries on this date.".to_string());
                } else {
                    for hit in review.on_this_day.iter().take(6) {
                        lines.push(format!(
                            "  {}  {}  {}",
                            hit.date.format("%Y-%m-%d"),
                            hit.entry_number,
                            hit.preview
                        ));
                    }
                }
                lines.push(String::new());
                lines.push("Top Tags".to_string());
                lines.extend(render_rank_lines(&review.top_tags));
                lines.push(String::new());
                lines.push("Top People".to_string());
                lines.extend(render_rank_lines(&review.top_people));
                lines.push(String::new());
                lines.push("Top Projects".to_string());
                lines.extend(render_rank_lines(&review.top_projects));
                lines.push(String::new());
                lines.push("Mood Distribution".to_string());
                if mood_counts.is_empty() {
                    lines.push("  No moods tagged yet.".to_string());
                } else {
                    for (mood, count) in mood_counts {
                        lines.push(format!("  {mood}: {count}"));
                    }
                }
                lines.push(String::new());
                lines.push("Recent Entries".to_string());
                if recent_entries.is_empty() {
                    lines.push("  No saved entries yet.".to_string());
                } else {
                    for entry in recent_entries {
                        lines.push(format!(
                            "  {}  {}  {}",
                            entry.date.format("%Y-%m-%d"),
                            entry.entry_number,
                            entry.preview
                        ));
                    }
                }
                self.open_info_overlay("Review Mode", lines.join("\n"));
            }
            Err(error) => self.open_info_overlay("Review Mode", format!("Review failed: {error}")),
        }
    }

    fn open_update_check_overlay(&mut self) {
        match platform::check_for_updates(env!("CARGO_PKG_VERSION")) {
            Ok(info) => {
                let command = platform::updater_command_preview(&info.latest_tag).unwrap_or_else(
                    |_| {
                        "curl -fsSL https://raw.githubusercontent.com/Awassee/bluescreenjournal/main/install.sh | bash"
                            .to_string()
                    },
                );
                let mut lines = vec![
                    format!("Current : {}", info.current_version),
                    format!("Latest  : {}", info.latest_tag),
                    format!(
                        "Status  : {}",
                        if info.newer_available {
                            "Update available"
                        } else {
                            "You are up to date"
                        }
                    ),
                    format!("Release : {}", info.html_url),
                    String::new(),
                    "Install command".to_string(),
                    command.clone(),
                ];
                if !info.asset_names.is_empty() {
                    lines.push(String::new());
                    lines.push("Assets".to_string());
                    for asset in info.asset_names.iter().take(6) {
                        lines.push(format!("  {asset}"));
                    }
                }
                let details = lines.join("\n");
                if info.newer_available {
                    self.open_picker_overlay(PickerOverlay::new(
                        "Updates",
                        vec![
                            PickerItem {
                                title: format!("Install {}", info.latest_tag),
                                detail: "BACKGROUND".to_string(),
                                keywords: format!(
                                    "update upgrade install {} {}",
                                    info.latest_tag, info.current_version
                                ),
                                action: PickerAction::InstallUpdate {
                                    tag: info.latest_tag.clone(),
                                    release_url: info.html_url.clone(),
                                    command: command.clone(),
                                },
                            },
                            PickerItem {
                                title: "View release details".to_string(),
                                detail: "INFO".to_string(),
                                keywords: "update release notes assets changelog".to_string(),
                                action: PickerAction::ShowInfo {
                                    title: "Updates".to_string(),
                                    text: details.clone(),
                                },
                            },
                        ],
                        "No update actions available.",
                    ));
                    self.flash_status("UPDATE AVAILABLE.");
                } else {
                    self.open_info_overlay("Updates", details);
                }
            }
            Err(error) => {
                self.open_info_overlay("Updates", format!("Update check failed: {error}"))
            }
        }
    }

    fn open_backup_history_overlay(&mut self) {
        let Some(vault) = &self.vault else {
            self.flash_status("LOCKED.");
            return;
        };

        match vault.list_backups() {
            Ok(backups) if backups.is_empty() => self.flash_status("NO BACKUPS."),
            Ok(backups) => {
                let items = backups
                    .into_iter()
                    .map(|backup| {
                        let file_name = backup
                            .path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("backup")
                            .to_string();
                        let detail = format!(
                            "{}  {}",
                            backup.created_at.format("%Y-%m-%d %H:%M"),
                            human_bytes(backup.size_bytes)
                        );
                        PickerItem {
                            title: file_name,
                            detail,
                            keywords: format!(
                                "backup restore {} {}",
                                backup.created_at.format("%Y-%m-%d %H:%M:%S"),
                                backup.path.display()
                            ),
                            action: PickerAction::OpenRestorePrompt {
                                backup_path: backup.path,
                            },
                        }
                    })
                    .collect::<Vec<_>>();
                self.open_picker_overlay(PickerOverlay::new(
                    "Backup History",
                    items,
                    "No encrypted backups yet.",
                ));
            }
            Err(error) => self.flash_status(&format!("BACKUP LIST FAILED: {error}")),
        }
    }

    fn open_backup_cleanup_preview_overlay(&mut self) {
        let Some(vault) = &self.vault else {
            self.flash_status("LOCKED.");
            return;
        };

        let output = match vault.preview_backup_prune(&self.config.backup_retention) {
            Ok(backups) if backups.is_empty() => {
                "Retention would keep all encrypted backups right now.".to_string()
            }
            Ok(backups) => {
                let mut lines = vec![
                    "Retention cleanup preview".to_string(),
                    format!(
                        "Keep daily={} weekly={} monthly={}",
                        self.config.backup_retention.daily,
                        self.config.backup_retention.weekly,
                        self.config.backup_retention.monthly
                    ),
                    String::new(),
                    "These encrypted backups would be removed by bsj backup prune --apply:"
                        .to_string(),
                    String::new(),
                ];
                for backup in backups.into_iter().take(14) {
                    let file_name = backup
                        .path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("backup");
                    lines.push(format!(
                        "{}  {:>8}  {}",
                        backup.created_at.format("%Y-%m-%d %H:%M"),
                        human_bytes(backup.size_bytes),
                        file_name
                    ));
                }
                lines.join("\n")
            }
            Err(error) => format!("Failed to preview backup cleanup: {error}"),
        };

        self.open_info_overlay("Backup Cleanup", output);
    }

    fn open_backup_policy_overlay(&mut self) {
        let mut lines = vec![
            "Backup Policy".to_string(),
            String::new(),
            format!("Daily   : {}", self.config.backup_retention.daily),
            format!("Weekly  : {}", self.config.backup_retention.weekly),
            format!("Monthly : {}", self.config.backup_retention.monthly),
        ];

        if let Some(vault) = &self.vault {
            match vault.list_backups() {
                Ok(backups) => lines.push(format!("Current backups : {}", backups.len())),
                Err(error) => lines.push(format!("Current backups : error ({error})")),
            }
            match vault.preview_backup_prune(&self.config.backup_retention) {
                Ok(backups) => lines.push(format!("Would prune     : {}", backups.len())),
                Err(error) => lines.push(format!("Would prune     : error ({error})")),
            }
        } else {
            lines.push("Current backups : vault locked".to_string());
        }

        lines.push(String::new());
        lines.push("FILE -> Backup Snapshot creates a new encrypted archive.".to_string());
        lines.push("FILE -> Prune Old Backups applies this retention policy.".to_string());
        self.open_info_overlay("Backup Policy", lines.join("\n"));
    }

    fn open_restore_prompt(&mut self) {
        let Some(vault) = &self.vault else {
            self.flash_status("LOCKED.");
            return;
        };
        match vault.list_backups() {
            Ok(backups) if backups.is_empty() => self.flash_status("NO BACKUPS."),
            Ok(backups) => {
                self.menu = None;
                self.overlay = Some(Overlay::RestorePrompt(RestorePrompt::new(
                    backups,
                    self.selected_date,
                )));
            }
            Err(error) => self.flash_status(&format!("RESTORE LIST FAILED: {error}")),
        }
    }

    fn open_restore_prompt_with_selected_backup(&mut self, selected_backup_path: &Path) {
        let Some(vault) = &self.vault else {
            self.flash_status("LOCKED.");
            return;
        };
        match vault.list_backups() {
            Ok(backups) if backups.is_empty() => self.flash_status("NO BACKUPS."),
            Ok(backups) => {
                self.menu = None;
                self.overlay = Some(Overlay::RestorePrompt(RestorePrompt::with_selected_backup(
                    backups,
                    self.selected_date,
                    selected_backup_path,
                )));
            }
            Err(error) => self.flash_status(&format!("RESTORE LIST FAILED: {error}")),
        }
    }

    fn open_dashboard_overlay(&mut self) {
        let vault_state = if self.vault.is_some() {
            "UNLOCKED"
        } else {
            "LOCKED"
        };
        let sync_target = self
            .config
            .sync_target_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "[unset]".to_string());
        let soundtrack_source = if self.config.soundtrack_source.trim().is_empty() {
            "[unset]".to_string()
        } else {
            self.config.soundtrack_source.clone()
        };
        let integrity = self.integrity_status_label();
        let dirty = if self.dirty { "modified" } else { "clean" };
        let save_state = self.save_status_label();
        let search_cache_summary = if let Some(vault) = &self.vault {
            let cache = vault.search_cache_status();
            if !cache.exists {
                "missing".to_string()
            } else if cache.valid {
                format!(
                    "encrypted {} entries",
                    cache.entry_count.unwrap_or_default()
                )
            } else {
                "invalid".to_string()
            }
        } else {
            "locked".to_string()
        };
        let export_summary = self
            .config
            .export_history
            .first()
            .map(|entry| format!("{} {}", entry.date, entry.format.to_ascii_uppercase()))
            .unwrap_or_else(|| "none".to_string());

        let (entry_count, conflict_count, backup_count) = if let Some(vault) = &self.vault {
            (
                vault
                    .list_entry_dates()
                    .ok()
                    .map(|dates| dates.len())
                    .unwrap_or(0),
                vault
                    .list_conflicted_dates()
                    .ok()
                    .map(|dates| dates.len())
                    .unwrap_or(0),
                vault
                    .list_backups()
                    .ok()
                    .map(|items| items.len())
                    .unwrap_or(0),
            )
        } else {
            (0, 0, 0)
        };

        let output = [
            "BlueScreen Journal Dashboard".to_string(),
            String::new(),
            format!("Version     : {}", self.app_version_label()),
            format!("Vault       : {vault_state}"),
            format!("Date        : {}", self.selected_date.format("%Y-%m-%d")),
            format!("Entry No.   : {}", self.entry_number_label()),
            format!(
                "Favorite    : {}",
                if self.is_favorite_date(self.selected_date) {
                    "STARRED"
                } else {
                    "NO"
                }
            ),
            format!("Editor      : {dirty} | {save_state}"),
            format!("Document    : {}", self.document_stats_label()),
            format!("Goal        : {}", self.word_goal_status_label()),
            format!("Session     : {}", self.session_status_label()),
            format!(
                "Cursor      : {}",
                self.cursor_status_label()
                    .replace("LN ", "line ")
                    .replace(" COL ", ", col ")
            ),
            format!(
                "Integrity   : {}",
                if integrity.is_empty() {
                    "n/a"
                } else {
                    &integrity
                }
            ),
            format!("Entries     : {entry_count}"),
            format!("Conflicts   : {conflict_count}"),
            format!("Backups     : {backup_count}"),
            format!("Recents     : {}", self.recent_dates.len()),
            format!("Favorites   : {}", self.favorite_dates().len()),
            format!("Sync target : {sync_target}"),
            format!(
                "Soundtrack  : {} ({soundtrack_source})",
                self.soundtrack_status_label()
            ),
            format!("Sync runs   : {}", self.config.sync_history.len()),
            format!("Search cache: {search_cache_summary}"),
            format!("Last export : {export_summary}"),
            format!(
                "Backup keep : d{} w{} m{}",
                self.config.backup_retention.daily,
                self.config.backup_retention.weekly,
                self.config.backup_retention.monthly
            ),
            format!("Logs        : {}", logging::log_file_path().display()),
            String::new(),
            "Use FILE for export/backups, GO for dates/index, TOOLS for sync/verify/admin."
                .to_string(),
        ]
        .join("\n");

        self.open_info_overlay("Dashboard", output);
    }

    fn open_integrity_details_overlay(&mut self) {
        let Some(vault) = &self.vault else {
            self.flash_status("LOCKED.");
            return;
        };
        match vault.verify_integrity() {
            Ok(report) if report.ok => self.open_info_overlay(
                "Integrity Details",
                "Verify result: OK\n\nAll visible revision chains are intact.".to_string(),
            ),
            Ok(report) => {
                let mut lines = vec![
                    format!("Verify result: BROKEN ({})", report.issues.len()),
                    String::new(),
                ];
                for issue in report.issues.iter().take(20) {
                    let date = issue
                        .date
                        .map(|date| date.format("%Y-%m-%d").to_string())
                        .unwrap_or_else(|| "vault".to_string());
                    lines.push(format!("{date}: {}", issue.message));
                }
                self.open_info_overlay("Integrity Details", lines.join("\n"));
            }
            Err(error) => {
                self.open_info_overlay("Integrity Details", format!("Verify failed: {error}"))
            }
        }
    }

    fn open_search_cache_status_overlay(&mut self) {
        let Some(vault) = &self.vault else {
            self.flash_status("LOCKED.");
            return;
        };

        let cache = vault.search_cache_status();
        let output = [
            "Search Cache".to_string(),
            String::new(),
            format!("Path       : {}", cache.path.display()),
            format!("Exists     : {}", if cache.exists { "yes" } else { "no" }),
            format!("Valid      : {}", if cache.valid { "yes" } else { "no" }),
            format!(
                "Entries    : {}",
                cache
                    .entry_count
                    .map(|count| count.to_string())
                    .unwrap_or_else(|| "n/a".to_string())
            ),
            format!("Size       : {}", human_bytes(cache.size_bytes)),
            format!(
                "Modified   : {}",
                cache
                    .modified_at
                    .map(|value| value.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                    .unwrap_or_else(|| "n/a".to_string())
            ),
            format!(
                "In memory  : {}",
                if self.search_index.is_some() {
                    "yes"
                } else {
                    "no"
                }
            ),
            format!(
                "Issue      : {}",
                cache.issue.unwrap_or_else(|| "none".to_string())
            ),
        ]
        .join("\n");
        self.open_info_overlay("Search Cache", output);
    }

    fn open_adjacent_saved_entry(&mut self, delta: isize) {
        let Some(vault) = &self.vault else {
            self.flash_status("LOCKED.");
            return;
        };

        let mut dates = match vault.list_entry_dates() {
            Ok(dates) => dates,
            Err(_) => {
                self.flash_status("INDEX LOAD FAILED.");
                return;
            }
        };
        dates.sort_unstable();
        let Some(current_idx) = dates.iter().position(|date| *date == self.selected_date) else {
            if let Some(target) = if delta < 0 {
                dates.last()
            } else {
                dates.first()
            } {
                self.open_date(*target);
                self.flash_status(&format!("ENTRY {}.", target.format("%Y-%m-%d")));
            } else {
                self.flash_status("NO SAVED ENTRIES.");
            }
            return;
        };

        let next_idx = current_idx as isize + delta;
        if !(0..dates.len() as isize).contains(&next_idx) {
            self.flash_status("NO MORE SAVED ENTRIES.");
            return;
        }

        let target = dates[next_idx as usize];
        self.open_date(target);
        self.flash_status(&format!("ENTRY {}.", target.format("%Y-%m-%d")));
    }

    fn open_next_new_entry(&mut self) {
        let mut target = self.selected_date + ChronoDuration::days(1);
        if let Some(vault) = &self.vault {
            let existing = match vault.list_entry_dates() {
                Ok(dates) => dates.into_iter().collect::<BTreeSet<_>>(),
                Err(_) => {
                    self.flash_status("INDEX LOAD FAILED.");
                    return;
                }
            };
            while existing.contains(&target) {
                target += ChronoDuration::days(1);
            }
        }
        self.open_date(target);
        self.flash_status(&format!("NEW ENTRY {}.", target.format("%Y-%m-%d")));
    }

    fn perform_menu_action(&mut self, action: MenuAction, viewport_height: usize) {
        match action {
            MenuAction::CommandPalette => self.open_command_palette(),
            MenuAction::Save => self.save_current_date(),
            MenuAction::Export => self.open_export_prompt(),
            MenuAction::QuickExportText => self.quick_export_current(ExportFormatUi::PlainText),
            MenuAction::QuickExportMarkdown => self.quick_export_current(ExportFormatUi::Markdown),
            MenuAction::ExportHistory => self.open_export_history_overlay(),
            MenuAction::BackupHistory => self.open_backup_history_overlay(),
            MenuAction::BackupCleanupPreview => self.open_backup_cleanup_preview_overlay(),
            MenuAction::BackupPolicy => self.open_backup_policy_overlay(),
            MenuAction::BackupPruneNow => self.prune_backups_now(),
            MenuAction::Backup => self.create_backup_now(),
            MenuAction::RestoreBackup => self.open_restore_prompt(),
            MenuAction::Dashboard => self.open_dashboard_overlay(),
            MenuAction::SyncCenter => self.open_sync_center_overlay(),
            MenuAction::ToggleSoundtrack => self.toggle_soundtrack_playback(),
            MenuAction::ReviewMode => self.open_review_overlay(),
            MenuAction::CheckUpdates => self.open_update_check_overlay(),
            MenuAction::DoctorReport => self.open_doctor_overlay(),
            MenuAction::Lock => self.lock_vault(),
            MenuAction::Quit => self.overlay = Some(Overlay::QuitConfirm),
            MenuAction::Find => {
                self.overlay = Some(Overlay::FindPrompt {
                    input: self.find_query.clone().unwrap_or_default(),
                    error: None,
                });
            }
            MenuAction::ClearFind => self.clear_find_state(),
            MenuAction::Replace => {
                self.overlay = Some(Overlay::ReplacePrompt(ReplacePrompt::new()))
            }
            MenuAction::Metadata => self.open_metadata_prompt(),
            MenuAction::ClosingThought => self.open_closing_prompt(),
            MenuAction::ToggleFavorite => self.toggle_favorite_current_date(),
            MenuAction::ToggleReveal => self.toggle_reveal_codes(viewport_height),
            MenuAction::ToggleTypewriter => self.toggle_typewriter_mode(),
            MenuAction::DuplicateLine => {
                self.buffer.duplicate_current_line();
                self.finish_buffer_menu_edit(viewport_height, "LINE DUPLICATED.");
            }
            MenuAction::DeleteLine => {
                self.buffer.delete_current_line();
                self.finish_buffer_menu_edit(viewport_height, "LINE DELETED.");
            }
            MenuAction::MoveLineUp => {
                self.buffer.move_line_up();
                self.finish_buffer_menu_edit(viewport_height, "LINE MOVED UP.");
            }
            MenuAction::MoveLineDown => {
                self.buffer.move_line_down();
                self.finish_buffer_menu_edit(viewport_height, "LINE MOVED DOWN.");
            }
            MenuAction::InsertTimeStamp => self.insert_text_snippet(
                &format!("{}\n", self.current_time_string()),
                viewport_height,
                "TIME INSERTED.",
            ),
            MenuAction::InsertDateStamp => self.insert_text_snippet(
                &format!("{}\n", self.selected_date.format("%Y-%m-%d")),
                viewport_height,
                "DATE INSERTED.",
            ),
            MenuAction::InsertDateTimeStamp => self.insert_text_snippet(
                &format!("{}\n", self.current_timestamp_string()),
                viewport_height,
                "TIMESTAMP INSERTED.",
            ),
            MenuAction::InsertDivider => self.insert_text_snippet(
                "----------------------------------------\n",
                viewport_height,
                "DIVIDER INSERTED.",
            ),
            MenuAction::InsertBlankAbove => {
                self.buffer.insert_blank_line_above();
                self.finish_buffer_menu_edit(viewport_height, "LINE OPENED ABOVE.");
            }
            MenuAction::InsertBlankBelow => {
                self.buffer.insert_blank_line_below();
                self.finish_buffer_menu_edit(viewport_height, "LINE OPENED BELOW.");
            }
            MenuAction::JumpTop => {
                self.buffer.move_to_top();
                self.ensure_cursor_visible(viewport_height);
                self.flash_status("TOP OF ENTRY.");
            }
            MenuAction::JumpBottom => {
                self.buffer.move_to_bottom();
                self.ensure_cursor_visible(viewport_height);
                self.flash_status("BOTTOM OF ENTRY.");
            }
            MenuAction::InsertStatsStamp => self.insert_text_snippet(
                &format!("[stats {}]\n", self.document_stats_label()),
                viewport_height,
                "STATS INSERTED.",
            ),
            MenuAction::InsertMetadataStamp => self.insert_text_snippet(
                &self.metadata_stamp(),
                viewport_height,
                "METADATA INSERTED.",
            ),
            MenuAction::GlobalSearch => self.open_search_overlay(),
            MenuAction::SearchHistory => self.open_search_history_overlay(),
            MenuAction::SearchScopeToday => {
                let mut search = SearchOverlay::new(self.find_query.clone());
                search.apply_today_scope(self.selected_date);
                self.remember_search_scope(&search);
                self.maybe_live_run_global_search(&mut search);
                self.menu = None;
                self.overlay = Some(Overlay::Search(search));
            }
            MenuAction::SearchScopeWeek => {
                let mut search = SearchOverlay::new(self.find_query.clone());
                search.apply_week_scope(self.selected_date);
                self.remember_search_scope(&search);
                self.maybe_live_run_global_search(&mut search);
                self.menu = None;
                self.overlay = Some(Overlay::Search(search));
            }
            MenuAction::SearchScopeMonth => {
                let mut search = SearchOverlay::new(self.find_query.clone());
                search.apply_month_scope(self.selected_date);
                self.remember_search_scope(&search);
                self.maybe_live_run_global_search(&mut search);
                self.menu = None;
                self.overlay = Some(Overlay::Search(search));
            }
            MenuAction::SearchScopeYear => {
                let mut search = SearchOverlay::new(self.find_query.clone());
                search.apply_year_scope(self.selected_date);
                self.remember_search_scope(&search);
                self.maybe_live_run_global_search(&mut search);
                self.menu = None;
                self.overlay = Some(Overlay::Search(search));
            }
            MenuAction::SearchScopeAll => {
                let mut search = SearchOverlay::new(self.find_query.clone());
                search.clear_filters();
                self.remember_search_scope(&search);
                self.maybe_live_run_global_search(&mut search);
                self.menu = None;
                self.overlay = Some(Overlay::Search(search));
            }
            MenuAction::SearchClearFilters => {
                let mut remembered_scope: Option<(String, String)> = None;
                self.open_search_overlay();
                if let Some(Overlay::Search(search)) = &mut self.overlay {
                    search.clear_filters();
                    remembered_scope = Some((search.from_input.clone(), search.to_input.clone()));
                    self.flash_status("SEARCH FILTERS CLEARED.");
                }
                if let Some((from, to)) = remembered_scope {
                    self.search_scope_from.zeroize();
                    self.search_scope_to.zeroize();
                    self.search_scope_from = from;
                    self.search_scope_to = to;
                }
            }
            MenuAction::FindNext => {
                if self.find_query.is_some() {
                    self.select_next_find_match(viewport_height);
                    self.flash_status("NEXT MATCH.");
                } else {
                    self.overlay = Some(Overlay::FindPrompt {
                        input: String::new(),
                        error: None,
                    });
                }
            }
            MenuAction::FindPrevious => {
                if self.find_query.is_some() {
                    self.select_previous_find_match(viewport_height);
                    self.flash_status("PREVIOUS MATCH.");
                } else {
                    self.overlay = Some(Overlay::FindPrompt {
                        input: String::new(),
                        error: None,
                    });
                }
            }
            MenuAction::PreviousParagraph => {
                self.buffer.move_paragraph_up();
                self.ensure_cursor_visible(viewport_height);
                self.flash_status("PREVIOUS PARAGRAPH.");
            }
            MenuAction::NextParagraph => {
                self.buffer.move_paragraph_down();
                self.ensure_cursor_visible(viewport_height);
                self.flash_status("NEXT PARAGRAPH.");
            }
            MenuAction::RebuildSearchIndex => self.rebuild_search_index(),
            MenuAction::SearchCacheStatus => self.open_search_cache_status_overlay(),
            MenuAction::Dates => self.open_date_picker(),
            MenuAction::RecentEntries => self.open_recent_entries_overlay(),
            MenuAction::FavoriteEntries => self.open_favorite_entries_overlay(),
            MenuAction::PreviousFavorite => self.open_adjacent_favorite(-1),
            MenuAction::NextFavorite => self.open_adjacent_favorite(1),
            MenuAction::RandomEntry => self.open_random_saved_entry(),
            MenuAction::PreviousEntry => self.open_adjacent_saved_entry(-1),
            MenuAction::NextEntry => self.open_adjacent_saved_entry(1),
            MenuAction::NewEntry => self.open_next_new_entry(),
            MenuAction::Today => {
                self.open_date(Local::now().date_naive());
                self.flash_status("JUMPED TO TODAY.");
            }
            MenuAction::Index => self.open_index_overlay(),
            MenuAction::Sync => self.begin_sync(),
            MenuAction::Verify => self.verify_integrity_now(),
            MenuAction::IntegrityDetails => self.open_integrity_details_overlay(),
            MenuAction::SettingsSummary => self.open_settings_summary_overlay(),
            MenuAction::ReviewPrompts => self.open_review_prompts_overlay(),
            MenuAction::SyncHistory => self.open_sync_history_overlay(),
            MenuAction::SessionReset => {
                self.session_started_at = Instant::now();
                self.flash_status("SESSION RESET.");
            }
            MenuAction::About => self.open_about_overlay(),
            MenuAction::HelpTopics => self.open_help_topics_overlay(),
            MenuAction::ToggleKeychainMemory => self.toggle_keychain_memory(),
            MenuAction::QuickStart => self.open_quickstart_overlay(),
            MenuAction::EditSetting(field) => self.open_setting_prompt(field),
            MenuAction::Help => self.overlay = Some(Overlay::Help),
        }
    }

    fn begin_sync(&mut self) {
        if self.vault.is_none() {
            self.flash_status("LOCKED.");
            return;
        }

        let draft_notice = self.dirty;
        if draft_notice {
            self.autosave_current_date();
        }

        let request = match self.resolve_sync_request() {
            Ok(request) => request,
            Err(error) => {
                self.overlay = Some(Overlay::SyncStatus(SyncStatusOverlay {
                    backend_label: "SYNC".to_string(),
                    target_label: "UNCONFIGURED".to_string(),
                    draft_notice,
                    phase: SyncPhase::Error { message: error },
                }));
                self.flash_status("SYNC FAILED.");
                return;
            }
        };

        self.overlay = Some(Overlay::SyncStatus(SyncStatusOverlay::pending(
            request.backend_label().to_string(),
            request.target_label().to_string(),
            draft_notice,
        )));
        self.pending_sync_request = Some(request);
    }

    fn create_backup_now(&mut self) {
        if self.vault.is_none() {
            self.flash_status("LOCKED.");
            return;
        }
        if self.dirty {
            self.autosave_current_date();
        }
        let Some(vault) = &self.vault else {
            self.flash_status("LOCKED.");
            return;
        };
        match vault.create_backup(&self.config.backup_retention) {
            Ok(summary) => {
                let file_name = summary
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("backup");
                if summary.pruned > 0 {
                    self.flash_status(&format!("BACKUP {file_name} (+{} PRUNED).", summary.pruned));
                } else {
                    self.flash_status(&format!("BACKUP {file_name}."));
                }
            }
            Err(error) => {
                log::warn!("backup failed: {error}");
                self.flash_status("BACKUP FAILED.");
            }
        }
    }

    fn prune_backups_now(&mut self) {
        let Some(vault) = &self.vault else {
            self.flash_status("LOCKED.");
            return;
        };
        match vault.prune_backups_now(&self.config.backup_retention) {
            Ok(pruned) if pruned.is_empty() => self.flash_status("NO BACKUPS PRUNED."),
            Ok(pruned) => self.flash_status(&format!("PRUNED {} BACKUP(S).", pruned.len())),
            Err(error) => self.flash_status(&format!("PRUNE FAILED: {error}")),
        }
    }

    fn lock_vault(&mut self) {
        if self.vault.is_none() {
            return;
        }
        if self.dirty {
            self.autosave_current_date();
        }
        log::info!("locking vault and clearing in-memory state");
        self.clear_sensitive_state();
        self.soundtrack_loop_enabled = false;
        self.stop_soundtrack_playback();
        self.vault = None;
        self.integrity_status = None;
        self.overlay = Some(Overlay::UnlockPrompt {
            input: String::new(),
            error: Some("Vault locked. Enter passphrase.".to_string()),
        });
        self.flash_status("LOCKED.");
    }

    fn soundtrack_source_for_playback(&self) -> Result<String, String> {
        let trimmed = self.config.soundtrack_source.trim();
        if trimmed.is_empty() {
            return Err("SOUNDTRACK SOURCE NOT SET.".to_string());
        }
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            let cached = cache_soundtrack_url(trimmed)?;
            return Ok(cached.display().to_string());
        }
        let path = expand_tilde(trimmed);
        if !path.exists() {
            return Err("SOUNDTRACK FILE NOT FOUND.".to_string());
        }
        Ok(path.display().to_string())
    }

    fn start_soundtrack_playback(&mut self) -> Result<(), String> {
        self.reap_soundtrack_process();
        if self.soundtrack_child.is_some() {
            return Ok(());
        }
        let source = self.soundtrack_source_for_playback()?;
        let child = Command::new("afplay")
            .arg(&source)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("SOUNDTRACK START FAILED: {error}"))?;
        self.soundtrack_child = Some(child);
        Ok(())
    }

    fn stop_soundtrack_playback(&mut self) {
        if let Some(mut child) = self.soundtrack_child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn reap_soundtrack_process(&mut self) {
        if let Some(child) = &mut self.soundtrack_child
            && let Ok(Some(_)) = child.try_wait()
        {
            self.soundtrack_child = None;
        }
    }

    fn toggle_soundtrack_playback(&mut self) {
        if self.config.soundtrack_source.trim().is_empty() {
            self.open_setting_prompt(SettingField::SoundtrackSource);
            self.flash_status("SET SOUNDTRACK SOURCE.");
            return;
        }

        if self.soundtrack_loop_enabled {
            self.soundtrack_loop_enabled = false;
            self.stop_soundtrack_playback();
            self.flash_status("SOUNDTRACK OFF.");
            return;
        }

        match self.start_soundtrack_playback() {
            Ok(()) => {
                self.soundtrack_loop_enabled = true;
                self.flash_status("SOUNDTRACK ON.");
            }
            Err(error) => {
                self.soundtrack_loop_enabled = false;
                self.flash_status(&error);
            }
        }
    }

    fn set_closing_thought_from_input(&mut self, input: &str) {
        let normalized = normalize_overlay_text(input);
        let changed = self.closing_thought != normalized;
        self.closing_thought = normalized;
        if changed {
            self.dirty = true;
            self.flash_status(if self.closing_thought.is_some() {
                "CLOSING SET."
            } else {
                "CLOSING CLEARED."
            });
        }
    }

    fn toggle_reveal_codes(&mut self, viewport_height: usize) {
        self.reveal_codes = !self.reveal_codes;
        self.ensure_cursor_visible(viewport_height);
        self.flash_status(if self.reveal_codes {
            "REVEAL CODES ON."
        } else {
            "REVEAL CODES OFF."
        });
    }

    fn verify_integrity_now(&mut self) {
        self.refresh_integrity_status();
        match &self.integrity_status {
            Some(status) if status.ok => self.flash_status("VERIFY OK."),
            Some(status) => self.flash_status(&format!("VERIFY BROKEN {}.", status.issue_count)),
            None => self.flash_status("LOCKED."),
        }
    }

    fn clear_sensitive_state(&mut self) {
        self.menu = None;
        self.wipe_overlay_state();
        self.wipe_entry_buffer();
        self.wipe_pending_state();
        if let Some(index) = &mut self.search_index {
            index.wipe();
        }
        self.search_index = None;
        for query in &mut self.search_history {
            query.zeroize();
        }
        self.search_history.clear();
        self.search_scope_from.zeroize();
        self.search_scope_to.zeroize();
        self.recent_dates.clear();
        self.pending_sync_request = None;
        self.reveal_codes = false;
    }

    fn rebuild_search_index(&mut self) {
        if let Some(index) = &mut self.search_index {
            index.wipe();
        }
        self.search_index = None;
        if let Err(error) = self.ensure_search_index() {
            log::warn!("search index rebuild failed: {error}");
            self.flash_status("SEARCH CACHE FAILED.");
        }
    }

    fn run_pending_sync(&mut self) {
        let Some(request) = self.pending_sync_request.take() else {
            return;
        };

        if let Some(Overlay::SyncStatus(sync_status)) = &mut self.overlay {
            sync_status.mark_running();
        }

        let result = self.execute_sync_request(&request);
        match result {
            Ok(report) => {
                self.refresh_integrity_status();
                let integrity_status = self.integrity_status.clone();
                if let Some(Overlay::SyncStatus(sync_status)) = &mut self.overlay {
                    sync_status.set_complete(report, integrity_status.as_ref());
                }
                self.record_sync_result(&request, None);
                self.flash_status("SYNC COMPLETE.");
            }
            Err(error) => {
                if let Some(Overlay::SyncStatus(sync_status)) = &mut self.overlay {
                    sync_status.set_error(error.clone());
                }
                self.record_sync_result(&request, Some(error.clone()));
                self.flash_status("SYNC FAILED.");
            }
        }
    }

    fn execute_sync_request(&self, request: &SyncRequest) -> Result<vault::SyncReport, String> {
        let vault = self
            .vault
            .as_ref()
            .ok_or_else(|| "Vault locked.".to_string())?;

        match request {
            SyncRequest::Folder { remote_root, .. } => vault
                .sync_folder(remote_root)
                .map_err(|error| format!("sync failed: {error}")),
            SyncRequest::S3 { .. } => {
                let mut backend = sync::S3Backend::from_remote(None)?;
                vault
                    .sync_with_backend(&mut backend)
                    .map_err(|error| format!("sync failed: {error}"))
            }
            SyncRequest::WebDav { .. } => {
                let mut backend = sync::WebDavBackend::from_remote(None)?;
                vault
                    .sync_with_backend(&mut backend)
                    .map_err(|error| format!("sync failed: {error}"))
            }
        }
    }

    fn resolve_sync_request(&self) -> Result<SyncRequest, String> {
        let backend = env::var("BSJ_SYNC_BACKEND")
            .ok()
            .map(|value| value.to_ascii_lowercase());

        match backend.as_deref() {
            Some("s3") => Ok(SyncRequest::S3 {
                target_label: sync_target_label_s3()?,
            }),
            Some("webdav") => Ok(SyncRequest::WebDav {
                target_label: sync_target_label_webdav()?,
            }),
            Some("folder") | None => {
                let remote_root = self
                    .config
                    .sync_target_path
                    .clone()
                    .ok_or_else(|| "No folder sync target configured. Run `bsj sync --backend folder --remote PATH` once, or set BSJ_SYNC_BACKEND=s3|webdav and the corresponding env vars.".to_string())?;
                Ok(SyncRequest::Folder {
                    target_label: remote_root.display().to_string(),
                    remote_root,
                })
            }
            Some(other) => Err(format!(
                "Invalid BSJ_SYNC_BACKEND '{other}'. Expected folder, s3, or webdav."
            )),
        }
    }

    fn sync_preview_report(
        &self,
        request: &SyncRequest,
    ) -> Result<sync::SyncPreviewReport, String> {
        let vault = self
            .vault
            .as_ref()
            .ok_or_else(|| "Vault locked.".to_string())?;
        match request {
            SyncRequest::Folder { remote_root, .. } => sync::preview_root(
                vault.metadata(),
                &self.vault_path,
                &mut sync::FolderBackend::new(remote_root.clone()),
            )
            .map_err(|error| format!("preview failed: {error}")),
            SyncRequest::S3 { .. } => {
                let mut backend = sync::S3Backend::from_remote(None)?;
                sync::preview_root(vault.metadata(), &self.vault_path, &mut backend)
                    .map_err(|error| format!("preview failed: {error}"))
            }
            SyncRequest::WebDav { .. } => {
                let mut backend = sync::WebDavBackend::from_remote(None)?;
                sync::preview_root(vault.metadata(), &self.vault_path, &mut backend)
                    .map_err(|error| format!("preview failed: {error}"))
            }
        }
    }

    fn record_sync_result(&mut self, request: &SyncRequest, error: Option<String>) {
        let (pulled, pushed, conflicts, integrity_ok) = match &self.overlay {
            Some(Overlay::SyncStatus(SyncStatusOverlay {
                phase:
                    SyncPhase::Complete {
                        pulled,
                        pushed,
                        conflicts,
                        integrity_ok,
                        ..
                    },
                ..
            })) => (*pulled, *pushed, conflicts.len(), *integrity_ok),
            _ => (
                0,
                0,
                0,
                self.integrity_status
                    .as_ref()
                    .map(|value| value.ok)
                    .unwrap_or(false),
            ),
        };
        let sync_record = LastSyncInfo {
            timestamp: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            backend: request.backend_label().to_string(),
            target: request.target_label().to_string(),
            pulled,
            pushed,
            conflicts,
            integrity_ok,
            error,
        };
        self.config.last_sync = Some(sync_record.clone());
        self.config.sync_history.retain(|existing| {
            existing.timestamp != sync_record.timestamp || existing.target != sync_record.target
        });
        self.config.sync_history.insert(0, sync_record);
        self.config.sync_history.truncate(10);
        let _ = self.config.save();
    }

    fn should_show_first_run_guide(&self) -> bool {
        self.vault.is_some()
            && !self.config.first_run_coach_completed
            && self.buffer.line_count() == 1
            && self.buffer.line(0).unwrap_or_default().is_empty()
    }

    fn mark_first_run_guide_shown(&mut self) {
        self.config.first_run_coach_completed = true;
        let _ = self.config.save();
    }

    fn apply_metadata_prompt(&mut self, prompt: &mut MetadataPrompt) -> bool {
        match prompt.parse() {
            Ok(metadata) => {
                let changed = self.entry_metadata != metadata;
                self.entry_metadata = metadata;
                if changed {
                    self.dirty = true;
                    self.flash_status("METADATA SAVED.");
                }
                true
            }
            Err(error) => {
                prompt.error = Some(error);
                false
            }
        }
    }

    fn apply_restore_prompt(&mut self, prompt: &mut RestorePrompt) -> bool {
        let Some(vault) = &self.vault else {
            prompt.error = Some("Vault locked.".to_string());
            return false;
        };
        let Some(backup) = prompt.selected_backup() else {
            prompt.error = Some("No backup selected.".to_string());
            return false;
        };
        if prompt.target_input.trim().is_empty() {
            prompt.error = Some("Restore path cannot be blank.".to_string());
            return false;
        }
        let target = expand_tilde(prompt.target_input.trim());
        match vault.restore_backup_into(&backup.path, &target) {
            Ok(()) => {
                let file_name = backup
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("backup");
                self.flash_status(&format!("RESTORED {file_name}."));
                true
            }
            Err(error) => {
                prompt.error = Some(format!("Restore failed: {error}"));
                false
            }
        }
    }

    fn run_update_install(&mut self, tag: &str, release_url: &str, fallback_command: &str) {
        match platform::start_background_update(tag) {
            Ok(launch) => {
                self.open_info_overlay(
                    "Updater",
                    [
                        format!("Installer started in background for {}.", launch.target_tag),
                        String::new(),
                        format!("Install prefix: {}", launch.prefix.display()),
                        format!("Log file      : {}", launch.log_path.display()),
                        String::new(),
                        "Keep writing if you want, then restart bsj after install finishes."
                            .to_string(),
                        "If install fails, run this manually:".to_string(),
                        launch.command_preview,
                    ]
                    .join("\n"),
                );
                self.flash_status("UPDATER STARTED.");
            }
            Err(error) => {
                self.open_info_overlay(
                    "Updater",
                    [
                        format!("Failed to start updater: {error}"),
                        String::new(),
                        format!("Release: {release_url}"),
                        "Manual install command:".to_string(),
                        fallback_command.to_string(),
                    ]
                    .join("\n"),
                );
                self.flash_status("UPDATER FAILED.");
            }
        }
    }

    fn apply_picker_action(&mut self, action: PickerAction, viewport_height: usize) {
        match action {
            PickerAction::Menu(action) => self.perform_menu_action(action, viewport_height),
            PickerAction::OpenDate(date) => {
                self.open_date(date);
                self.flash_status("DATE OPENED.");
            }
            PickerAction::OpenSearch(query) => {
                self.remember_search_query(&query);
                let mut overlay = SearchOverlay::new(Some(query));
                if self.vault.is_some() {
                    self.run_global_search(&mut overlay, true);
                }
                self.menu = None;
                self.overlay = Some(Overlay::Search(overlay));
            }
            PickerAction::OpenExportPrompt { format, path } => {
                self.open_export_prompt_with_preset(format, path);
                self.flash_status("EXPORT PRESET LOADED.");
            }
            PickerAction::OpenRestorePrompt { backup_path } => {
                self.open_restore_prompt_with_selected_backup(&backup_path);
            }
            PickerAction::InstallUpdate {
                tag,
                release_url,
                command,
            } => self.run_update_install(&tag, &release_url, &command),
            PickerAction::InsertText(text) => {
                self.buffer.insert_text(&text);
                self.finish_buffer_menu_edit(viewport_height, "TEXT INSERTED.");
            }
            PickerAction::ShowInfo { title, text } => self.open_info_overlay(title, text),
        }
    }

    fn resolve_conflict_with_head(
        &mut self,
        conflict: &ConflictOverlay,
        head: &vault::ConflictHead,
        label: &str,
    ) {
        let Some(vault) = &self.vault else {
            self.flash_status("LOCKED.");
            return;
        };
        let merged_hashes = conflict
            .conflict
            .heads
            .iter()
            .filter(|candidate| candidate.revision_hash != head.revision_hash)
            .map(|candidate| candidate.revision_hash.clone())
            .collect::<Vec<_>>();
        match vault.save_entry_merge_revision(
            self.selected_date,
            &head.body,
            head.closing_thought.as_deref(),
            &head.entry_metadata,
            &head.revision_hash,
            &merged_hashes,
        ) {
            Ok(()) => {
                self.load_selected_date();
                self.flash_status(&format!("CONFLICT RESOLVED WITH {label}."));
            }
            Err(error) => self.flash_status(&format!("MERGE FAILED: {error}")),
        }
    }

    fn toggle_typewriter_mode(&mut self) {
        self.config.typewriter_mode = !self.config.typewriter_mode;
        let _ = self.config.save();
        self.flash_status(if self.config.typewriter_mode {
            "TYPEWRITER ON."
        } else {
            "TYPEWRITER OFF."
        });
    }

    fn toggle_keychain_memory(&mut self) {
        self.config.remember_passphrase_in_keychain = !self.config.remember_passphrase_in_keychain;
        if !self.config.remember_passphrase_in_keychain {
            let _ = platform::delete_passphrase(&self.vault_path);
        }
        let _ = self.config.save();
        self.flash_status(if self.config.remember_passphrase_in_keychain {
            "KEYCHAIN ON. RELOCK TO STORE."
        } else {
            "KEYCHAIN OFF."
        });
    }

    fn try_keychain_auto_unlock(&mut self) {
        if !self.config.remember_passphrase_in_keychain || !vault::vault_exists(&self.vault_path) {
            return;
        }
        let Ok(Some(secret)) = platform::load_passphrase(&self.vault_path) else {
            return;
        };
        let mut config = self.config.clone();
        let device_id = config
            .local_device_id
            .clone()
            .unwrap_or_else(vault::random_device_id);
        if config.local_device_id.is_none() {
            config.local_device_id = Some(device_id.clone());
        }
        if let Ok(unlocked) = vault::unlock_vault_with_device(&self.vault_path, &secret, device_id)
        {
            self.config = config;
            self.vault = Some(unlocked);
            self.search_index = None;
            self.refresh_integrity_status();
            self.overlay = None;
            self.load_selected_date();
            self.flash_status("UNLOCKED FROM KEYCHAIN.");
        }
    }

    fn refresh_integrity_status(&mut self) {
        let Some(vault) = &self.vault else {
            self.integrity_status = None;
            return;
        };

        self.integrity_status = match vault.verify_integrity() {
            Ok(report) => Some(IntegrityStatus {
                ok: report.ok,
                issue_count: report.issues.len(),
            }),
            Err(error) => {
                log::warn!("integrity verification failed: {error}");
                Some(IntegrityStatus {
                    ok: false,
                    issue_count: 1,
                })
            }
        };
    }

    fn apply_setting_prompt(&mut self, prompt: &mut SettingPrompt) -> bool {
        let mut config = self.config.clone();
        let value =
            if prompt.field == SettingField::SyncTargetPath && prompt.input.trim().is_empty() {
                "unset"
            } else {
                prompt.input.as_str()
            };

        if let Err(error) = config::set_setting_value(&mut config, prompt.field.key(), value) {
            prompt.error = Some(error);
            return false;
        }

        if prompt.field == SettingField::DeviceNickname
            && self.vault.is_some()
            && let Some(device_id) = config.local_device_id.as_deref()
            && let Err(error) =
                vault::register_device(&self.vault_path, device_id, &config.device_nickname)
        {
            prompt.error = Some(format!("Failed to update device file: {error}"));
            return false;
        }

        if let Err(error) = config.save() {
            prompt.error = Some(format!("Failed to save config: {error}"));
            return false;
        }

        self.config = config;
        if prompt.field == SettingField::SoundtrackSource && self.soundtrack_loop_enabled {
            self.stop_soundtrack_playback();
            if let Err(error) = self.start_soundtrack_playback() {
                self.soundtrack_loop_enabled = false;
                self.flash_status(&error);
                return true;
            }
        }
        if prompt.field == SettingField::VaultPath {
            if self.vault.is_some() && self.dirty {
                self.autosave_current_date();
            }
            let new_vault_path = self.config.vault_path.clone();
            self.vault_path = new_vault_path.clone();
            self.clear_sensitive_state();
            self.vault = None;
            self.integrity_status = None;
            self.overlay = if vault::vault_exists(&new_vault_path) {
                Some(Overlay::UnlockPrompt {
                    input: String::new(),
                    error: Some("Vault path changed. Enter passphrase.".to_string()),
                })
            } else {
                Some(Overlay::SetupWizard(SetupWizard::new(&new_vault_path)))
            };
            self.flash_status("VAULT PATH SET.");
            return true;
        }

        self.flash_status(match prompt.field {
            SettingField::VaultPath => "VAULT PATH SET.",
            SettingField::SyncTargetPath => {
                if self.config.sync_target_path.is_some() {
                    "SYNC FOLDER SET."
                } else {
                    "SYNC FOLDER CLEARED."
                }
            }
            SettingField::DeviceNickname => "DEVICE NAME SET.",
            SettingField::Clock12h => "CLOCK FORMAT SET.",
            SettingField::ShowSeconds => "CLOCK SECONDS SET.",
            SettingField::ShowRuler => "RULER SET.",
            SettingField::ShowFooterLegend => "FOOTER LEGEND SET.",
            SettingField::SoundtrackSource => {
                if self.config.soundtrack_source.trim().is_empty() {
                    "SOUNDTRACK SOURCE CLEARED."
                } else {
                    "SOUNDTRACK SOURCE SET."
                }
            }
            SettingField::DailyWordGoal => {
                if self.config.daily_word_goal.is_some() {
                    "WORD GOAL SET."
                } else {
                    "WORD GOAL CLEARED."
                }
            }
            SettingField::BackupDaily
            | SettingField::BackupWeekly
            | SettingField::BackupMonthly => "RETENTION UPDATED.",
        });
        true
    }

    fn apply_export_prompt(&mut self, prompt: &mut ExportPrompt) -> bool {
        if self.vault.is_none() {
            prompt.error = Some("Vault locked.".to_string());
            return false;
        }

        let path_text = prompt.path_input.trim();
        if path_text.is_empty() {
            prompt.error = Some("Export path cannot be blank.".to_string());
            return false;
        }

        let path = expand_tilde(path_text);
        let entry_number = self.entry_number_label();
        let rendered = match prompt.format {
            ExportFormatUi::PlainText => {
                vault::format_export_text(&self.buffer.to_text(), self.closing_thought.as_deref())
            }
            ExportFormatUi::Markdown => render_markdown_export(
                self.selected_date,
                &entry_number,
                &self.entry_metadata,
                &self.buffer.to_text(),
                self.closing_thought.as_deref(),
            ),
        };

        match secure_fs::atomic_write_restricted(&path, rendered.as_bytes()) {
            Ok(()) => {
                self.record_export_history(prompt.format, &path);
                let file_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("export");
                self.flash_status(&format!("PLAINTEXT EXPORTED {file_name}."));
                true
            }
            Err(error) => {
                prompt.error = Some(format!("Failed to write {}: {error}", path.display()));
                false
            }
        }
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
            vault.save_entry_merge_revision(
                self.selected_date,
                &body,
                self.closing_thought.as_deref(),
                &self.entry_metadata,
                &merge_context.primary_hash,
                &merge_context.merged_hashes,
            )
        } else {
            vault.save_entry_revision(
                self.selected_date,
                &body,
                self.closing_thought.as_deref(),
                &self.entry_metadata,
            )
        };

        match save_result {
            Ok(()) => {
                log::info!("manual save completed");
                self.dirty = false;
                self.search_index = None;
                self.wipe_merge_context();
                self.refresh_integrity_status();
                let saved_at = Local::now();
                self.last_save_kind = Some(SaveKind::Saved);
                self.last_save_time = Some(saved_at);
                self.load_selected_date();
                self.last_save_kind = Some(SaveKind::Saved);
                self.last_save_time = Some(saved_at);
                self.flash_status(&format!("REVISION SAVED {}.", saved_at.format("%H:%M:%S")));
            }
            Err(_) => {
                log::warn!("manual save failed");
                self.flash_status("SAVE FAILED.")
            }
        }
    }

    fn autosave_current_date(&mut self) {
        let Some(vault) = &self.vault else {
            return;
        };
        let body = self.buffer.to_text();
        if vault
            .save_entry_draft(
                self.selected_date,
                &body,
                self.closing_thought.as_deref(),
                &self.entry_metadata,
            )
            .is_ok()
        {
            log::debug!("autosave completed");
            self.last_save_kind = Some(SaveKind::Autosaved);
            self.last_save_time = Some(Local::now());
        }
    }

    fn apply_recovery_choice(&mut self, use_draft: bool, draft_text: &str) {
        let (resolved_text, recovered) =
            resolve_recovery_text(use_draft, self.buffer.to_text().as_str(), draft_text);
        self.buffer = TextBuffer::from_text(&resolved_text);
        self.refresh_document_stats();
        if use_draft {
            if let Some(closing_thought) = self.pending_recovery_closing.take() {
                self.closing_thought = closing_thought;
            }
            if let Some(entry_metadata) = self.pending_recovery_metadata.take() {
                self.entry_metadata = entry_metadata;
            }
        } else {
            self.pending_recovery_closing = None;
            self.pending_recovery_metadata = None;
        }
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

    fn run_global_search(&mut self, search: &mut SearchOverlay, focus_results: bool) {
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

        self.remember_search_scope(search);

        if let Err(error) = self.ensure_search_index() {
            search.error = Some(error);
            return;
        }

        let previous_selected = search
            .selected_result()
            .map(|result| (result.date, result.row, result.start_col, result.end_col));

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
        search.selected = previous_selected
            .and_then(|selected| {
                search.results.iter().position(|result| {
                    (result.date, result.row, result.start_col, result.end_col) == selected
                })
            })
            .unwrap_or(0);
        search.error = if search.results.is_empty() {
            Some("No matches.".to_string())
        } else {
            None
        };
        if focus_results && !search.results.is_empty() {
            search.active_field = SearchField::Results;
        } else if search.results.is_empty() {
            search.active_field = SearchField::Query;
        }
    }

    fn maybe_live_run_global_search(&mut self, search: &mut SearchOverlay) {
        if search.query_input.trim().is_empty() {
            search.clear_results();
            search.error = None;
            search.active_field = SearchField::Query;
            return;
        }
        self.run_global_search(search, false);
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
        log::debug!("search index built with {} documents", document_count);
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
                self.replace_editor_contents(
                    &head_a.body,
                    head_a.closing_thought.clone(),
                    head_a.entry_metadata.clone(),
                );
                self.scroll_row = 0;
                self.dirty = false;
                self.wipe_merge_context();
                self.refresh_find_matches();
                self.apply_pending_search_jump(Some(viewport_height));
                self.flash_status("VIEW A.");
            }
            ConflictMode::ViewB => {
                self.replace_editor_contents(
                    &head_b.body,
                    head_b.closing_thought.clone(),
                    head_b.entry_metadata.clone(),
                );
                self.scroll_row = 0;
                self.dirty = false;
                self.wipe_merge_context();
                self.refresh_find_matches();
                self.apply_pending_search_jump(Some(viewport_height));
                self.flash_status("VIEW B.");
            }
            ConflictMode::AcceptA => self.resolve_conflict_with_head(conflict, head_a, "A"),
            ConflictMode::AcceptB => self.resolve_conflict_with_head(conflict, head_b, "B"),
            ConflictMode::Merge => {
                self.replace_editor_contents(
                    &head_a.body,
                    head_a.closing_thought.clone(),
                    head_a.entry_metadata.clone(),
                );
                self.scroll_row = 0;
                self.dirty = false;
                self.refresh_find_matches();
                self.wipe_merge_context();
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

    fn wipe_overlay_state(&mut self) {
        if let Some(mut overlay) = self.overlay.take() {
            wipe_overlay(&mut overlay);
        }
    }

    fn wipe_entry_buffer(&mut self) {
        self.buffer.wipe();
        self.refresh_document_stats();
        zeroize_optional_string(&mut self.closing_thought);
        wipe_entry_metadata(&mut self.entry_metadata);
        self.scroll_row = 0;
        self.dirty = false;
    }

    fn wipe_pending_state(&mut self) {
        zeroize_optional_string(&mut self.find_query);
        self.find_matches.clear();
        self.current_match_idx = 0;
        if let Some(mut jump) = self.pending_search_jump.take() {
            jump.wipe();
        }
        if let Some(mut conflict) = self.pending_conflict.take() {
            wipe_conflict_state(&mut conflict);
        }
        if let Some(mut pending_recovery_closing) = self.pending_recovery_closing.take()
            && let Some(closing_thought) = &mut pending_recovery_closing
        {
            closing_thought.zeroize();
        }
        if let Some(mut entry_metadata) = self.pending_recovery_metadata.take() {
            wipe_entry_metadata(&mut entry_metadata);
        }
        self.wipe_merge_context();
    }

    fn wipe_merge_context(&mut self) {
        if let Some(mut merge_context) = self.merge_context.take() {
            merge_context.wipe();
        }
    }

    fn replace_editor_contents(
        &mut self,
        text: &str,
        closing_thought: Option<String>,
        entry_metadata: EntryMetadata,
    ) {
        self.wipe_entry_buffer();
        self.buffer = TextBuffer::from_text(text);
        self.refresh_document_stats();
        self.closing_thought = closing_thought;
        self.entry_metadata = entry_metadata;
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
        self.wrap_cursor_line();
        self.dirty = true;
        self.refresh_document_stats();
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

    fn try_run_macro(&mut self, key: &KeyEvent, viewport_height: usize) -> bool {
        for macro_config in self.config.macros.clone() {
            if !macro_key_matches(&macro_config.key, key) {
                continue;
            }
            self.run_macro_action(macro_config.action, viewport_height);
            return true;
        }
        false
    }

    fn run_macro_action(&mut self, action: MacroActionConfig, viewport_height: usize) {
        match action {
            MacroActionConfig::InsertTemplate { text } => {
                self.buffer.insert_text(&text);
                self.wrap_cursor_and_previous_line();
                self.dirty = true;
                self.refresh_document_stats();
                self.refresh_find_matches();
                self.ensure_cursor_visible(viewport_height);
                self.flash_status("MACRO INSERTED.");
            }
            MacroActionConfig::Command { command } => match command {
                MacroCommandConfig::InsertDateHeader => {
                    let template = format!(
                        "{}  ENTRY NO. {}\n\n",
                        self.selected_date.format("%A, %B %d, %Y"),
                        self.entry_number_label()
                    );
                    self.buffer.insert_text(&template);
                    self.wrap_cursor_and_previous_line();
                    self.dirty = true;
                    self.refresh_document_stats();
                    self.refresh_find_matches();
                    self.ensure_cursor_visible(viewport_height);
                    self.flash_status("DATE HEADER INSERTED.");
                }
                MacroCommandConfig::InsertClosingLine => self.open_closing_prompt(),
                MacroCommandConfig::JumpToday => {
                    self.open_date(Local::now().date_naive());
                    self.flash_status("JUMPED TO TODAY.");
                }
            },
        }
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
        if self.config.typewriter_mode {
            let centered = cursor_row.saturating_sub(rows / 2);
            let max_start = self.buffer.line_count().saturating_sub(rows);
            self.scroll_row = centered.min(max_start);
            return;
        }
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

    fn menu_hotkey(key: &KeyEvent) -> Option<MenuId> {
        if !key.modifiers.contains(KeyModifiers::ALT) {
            return None;
        }
        let KeyCode::Char(ch) = key.code else {
            return None;
        };
        MenuId::from_hotkey(ch)
    }

    fn menu_ctrl_hotkey(key: &KeyEvent) -> Option<MenuId> {
        if !key.modifiers.contains(KeyModifiers::CONTROL) {
            return None;
        }
        let KeyCode::Char(ch) = key.code else {
            return None;
        };
        match ch.to_ascii_lowercase() {
            'o' | 'g' => Some(MenuId::File),
            'e' => Some(MenuId::Edit),
            'w' => Some(MenuId::Search),
            'y' => Some(MenuId::Go),
            't' => Some(MenuId::Tools),
            'u' => Some(MenuId::Setup),
            'l' => Some(MenuId::Help),
            _ => None,
        }
    }

    fn is_text_input_key(key: &KeyEvent) -> bool {
        key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT
    }
}

fn normalize_overlay_text(input: &str) -> Option<String> {
    let trimmed = input.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn sync_target_label_s3() -> Result<String, String> {
    let bucket = env::var("BSJ_S3_BUCKET")
        .map_err(|_| "BSJ_S3_BUCKET is required for TUI S3 sync.".to_string())?;
    let prefix = env::var("BSJ_S3_PREFIX").unwrap_or_default();
    if prefix.trim().is_empty() {
        Ok(format!("s3://{bucket}"))
    } else {
        Ok(format!("s3://{bucket}/{}", prefix.trim_matches('/')))
    }
}

fn sync_target_label_webdav() -> Result<String, String> {
    env::var("BSJ_WEBDAV_URL")
        .map_err(|_| "BSJ_WEBDAV_URL is required for TUI WebDAV sync.".to_string())
}

fn cache_soundtrack_url(url: &str) -> Result<PathBuf, String> {
    let cache_dir = env::temp_dir().join(SOUNDTRACK_CACHE_DIR_NAME);
    secure_fs::ensure_private_dir(&cache_dir)
        .map_err(|error| format!("SOUNDTRACK CACHE INIT FAILED: {error}"))?;
    let cache_path = soundtrack_cache_path(&cache_dir, url);
    if cache_path.exists()
        && cache_path
            .metadata()
            .map(|metadata| metadata.len() > 0)
            .unwrap_or(false)
    {
        return Ok(cache_path);
    }

    let response = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| format!("SOUNDTRACK HTTP CLIENT FAILED: {error}"))?
        .get(url)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|error| format!("SOUNDTRACK DOWNLOAD FAILED: {error}"))?;

    let bytes = response
        .bytes()
        .map_err(|error| format!("SOUNDTRACK DOWNLOAD FAILED: {error}"))?;
    if bytes.is_empty() {
        return Err("SOUNDTRACK DOWNLOAD FAILED: EMPTY RESPONSE.".to_string());
    }

    secure_fs::atomic_write_private(&cache_path, bytes.as_ref())
        .map_err(|error| format!("SOUNDTRACK CACHE WRITE FAILED: {error}"))?;
    Ok(cache_path)
}

fn soundtrack_cache_path(cache_dir: &Path, url: &str) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    let digest = hex::encode(hasher.finalize());
    let ext = soundtrack_cache_extension(url);
    cache_dir.join(format!("theme-{digest}.{ext}"))
}

fn soundtrack_cache_extension(url: &str) -> String {
    let base = url.split('?').next().unwrap_or(url);
    let candidate = Path::new(base)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("mid")
        .to_ascii_lowercase();

    let sanitized = candidate
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(8)
        .collect::<String>();
    if sanitized.is_empty() {
        "mid".to_string()
    } else {
        sanitized
    }
}

fn default_export_path(date: NaiveDate, format: ExportFormatUi) -> PathBuf {
    let base = dirs::desktop_dir()
        .or_else(dirs::document_dir)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")));
    base.join(format!(
        "bsj-{}.{}",
        date.format("%Y-%m-%d"),
        format.extension()
    ))
}

fn parse_export_history_format(value: &str) -> ExportFormatUi {
    match value.trim().to_ascii_lowercase().as_str() {
        "markdown" | "md" => ExportFormatUi::Markdown,
        _ => ExportFormatUi::PlainText,
    }
}

fn truncate_for_picker(text: &str, max_chars: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_chars {
        return text.to_string();
    }
    if max_chars <= 3 {
        return "...".chars().take(max_chars).collect();
    }
    let keep = max_chars - 3;
    let head = keep / 2;
    let tail = keep - head;
    let prefix = text.chars().take(head).collect::<String>();
    let suffix = text
        .chars()
        .rev()
        .take(tail)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    format!("{prefix}...{suffix}")
}

fn render_markdown_export(
    date: NaiveDate,
    entry_number: &str,
    metadata: &EntryMetadata,
    body: &str,
    closing_thought: Option<&str>,
) -> String {
    let mut out = String::new();
    out.push_str("# BlueScreen Journal Entry\n\n");
    out.push_str(&format!("Date: {}\n", date.format("%Y-%m-%d")));
    out.push_str(&format!("Entry No.: {}\n\n", entry_number));
    if !metadata.tags.is_empty() {
        out.push_str(&format!("Tags: {}\n", metadata.tags.join(", ")));
    }
    if !metadata.people.is_empty() {
        out.push_str(&format!("People: {}\n", metadata.people.join(", ")));
    }
    if let Some(project) = metadata.project.as_deref() {
        out.push_str(&format!("Project: {project}\n"));
    }
    if let Some(mood) = metadata.mood {
        out.push_str(&format!("Mood: {mood}\n"));
    }
    if !metadata.tags.is_empty()
        || !metadata.people.is_empty()
        || metadata.project.is_some()
        || metadata.mood.is_some()
    {
        out.push('\n');
    }
    out.push_str(body.trim_end());
    if let Some(closing_thought) = normalize_overlay_text(closing_thought.unwrap_or_default()) {
        if !body.trim_end().is_empty() {
            out.push_str("\n\n");
        }
        out.push_str("## Closing Thought\n\n");
        out.push_str(&closing_thought);
    }
    out
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

fn zeroize_optional_string(value: &mut Option<String>) {
    if let Some(mut string) = value.take() {
        string.zeroize();
    }
}

fn wipe_search_results(results: &mut Vec<SearchResult>) {
    for result in results.iter_mut() {
        result.entry_number.zeroize();
        result.snippet.text.zeroize();
        result.matched_text.zeroize();
    }
    results.clear();
}

fn wipe_conflict_state(conflict: &mut vault::ConflictState) {
    for head in &mut conflict.heads {
        head.revision_hash.zeroize();
        head.device_id.zeroize();
        head.body.zeroize();
        zeroize_optional_string(&mut head.closing_thought);
        head.preview.zeroize();
    }
    conflict.heads.clear();
}

fn wipe_overlay(overlay: &mut Overlay) {
    match overlay {
        Overlay::SetupWizard(wizard) => wizard.wipe(),
        Overlay::UnlockPrompt { input, error } => {
            input.zeroize();
            zeroize_optional_string(error);
        }
        Overlay::Help | Overlay::DatePicker(_) | Overlay::QuitConfirm => {}
        Overlay::FindPrompt { input, error } => {
            input.zeroize();
            zeroize_optional_string(error);
        }
        Overlay::ClosingPrompt { input } => input.zeroize(),
        Overlay::ConflictChoice(conflict) => conflict.wipe(),
        Overlay::MergeDiff(conflict) => wipe_conflict_state(conflict),
        Overlay::Search(search) => search.wipe(),
        Overlay::ReplacePrompt(prompt) => prompt.wipe(),
        Overlay::ReplaceConfirm(confirm) => confirm.wipe(),
        Overlay::MetadataPrompt(prompt) => prompt.wipe(),
        Overlay::ExportPrompt(prompt) => prompt.wipe(),
        Overlay::SettingPrompt(prompt) => prompt.wipe(),
        Overlay::Index(index) => index.wipe(),
        Overlay::SyncStatus(sync_status) => sync_status.wipe(),
        Overlay::RestorePrompt(prompt) => prompt.wipe(),
        Overlay::Info(info) => info.wipe(),
        Overlay::Picker(picker) => picker.wipe(),
        Overlay::RecoverDraft { draft_text } => draft_text.zeroize(),
    }
}

pub fn format_reveal_codes(
    date: NaiveDate,
    entry_number: &str,
    metadata: &EntryMetadata,
    body: &str,
    closing_thought: Option<&str>,
) -> String {
    let mut codes = vec![
        format!("⟦DATE:{}⟧", date.format("%Y-%m-%d")),
        format!("⟦ENTRY:{}⟧", entry_number),
    ];

    let metadata_tags = if metadata.tags.is_empty() {
        extract_reveal_tags(body)
    } else {
        metadata.tags.clone()
    };
    for tag in metadata_tags {
        codes.push(format!("⟦TAG:{tag}⟧"));
    }
    if !metadata.people.is_empty() {
        codes.push(format!("⟦PEOPLE:{}⟧", metadata.people.join(",")));
    }
    if let Some(project) = metadata.project.as_deref() {
        codes.push(format!("⟦PROJECT:{project}⟧"));
    }
    if let Some(mood) = metadata.mood.or_else(|| extract_reveal_mood(body)) {
        codes.push(format!("⟦MOOD:{mood}⟧"));
    }
    if let Some(closing_thought) = normalize_overlay_text(closing_thought.unwrap_or_default()) {
        codes.push(format!("⟦CLOSE:{closing_thought}⟧"));
    }

    codes.join(" ")
}

fn extract_reveal_tags(body: &str) -> Vec<String> {
    let mut tags = Vec::new();
    for token in body.split_whitespace() {
        let Some(rest) = token.strip_prefix('#') else {
            continue;
        };
        let cleaned = rest
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-')
            .collect::<String>()
            .to_lowercase();
        if cleaned.is_empty() || tags.contains(&cleaned) {
            continue;
        }
        tags.push(cleaned);
        if tags.len() == 3 {
            break;
        }
    }
    tags
}

fn extract_reveal_mood(body: &str) -> Option<u8> {
    for token in body.split_whitespace() {
        let normalized = token
            .trim_matches(|ch: char| ch == ',' || ch == '.' || ch == ';' || ch == ':')
            .to_ascii_lowercase();
        let Some(digits) = normalized
            .strip_prefix("mood:")
            .or_else(|| normalized.strip_prefix("mood="))
        else {
            continue;
        };
        let mood = digits.parse::<u8>().ok()?;
        if mood <= 9 {
            return Some(mood);
        }
    }
    None
}

fn parse_metadata_list(input: &str) -> Vec<String> {
    let mut items = Vec::new();
    for part in input.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty()
            || items
                .iter()
                .any(|item: &String| item.eq_ignore_ascii_case(trimmed))
        {
            continue;
        }
        items.push(trimmed.to_string());
    }
    items
}

fn wipe_entry_metadata(metadata: &mut EntryMetadata) {
    for tag in &mut metadata.tags {
        tag.zeroize();
    }
    metadata.tags.clear();
    for person in &mut metadata.people {
        person.zeroize();
    }
    metadata.people.clear();
    if let Some(project) = &mut metadata.project {
        project.zeroize();
    }
    metadata.project = None;
    metadata.mood = None;
}

fn zeroize_entry_metadata(metadata: &mut EntryMetadata) {
    wipe_entry_metadata(metadata);
}

fn render_rank_lines(items: &[(String, usize)]) -> Vec<String> {
    if items.is_empty() {
        return vec!["  No data yet.".to_string()];
    }
    items
        .iter()
        .map(|(label, count)| format!("  {:<18} {}", label, count))
        .collect()
}

fn top_mood_counts<I>(moods: I) -> Vec<(u8, usize)>
where
    I: IntoIterator<Item = u8>,
{
    let mut counts = std::collections::BTreeMap::<u8, usize>::new();
    for mood in moods {
        *counts.entry(mood).or_insert(0) += 1;
    }
    let mut pairs = counts.into_iter().collect::<Vec<_>>();
    pairs.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    pairs
}

pub(crate) fn index_row_flags(entry: &IndexEntry, favorite_dates: &BTreeSet<NaiveDate>) -> String {
    let favorite = if favorite_dates.contains(&entry.date) {
        '*'
    } else {
        '-'
    };
    let conflict = if entry.has_conflict { '!' } else { '-' };
    let tags = if entry.metadata.tags.is_empty() {
        '-'
    } else {
        '#'
    };
    let people = if entry.metadata.people.is_empty() {
        '-'
    } else {
        '@'
    };
    let project = if entry.metadata.project.is_some() {
        'P'
    } else {
        '-'
    };
    let mood = entry
        .metadata
        .mood
        .map(|mood| char::from_digit(mood as u32, 10).unwrap_or('M'))
        .unwrap_or('-');
    format!("{favorite}{conflict}{tags}{people}{project}{mood}")
}

pub(crate) fn index_detail_summary(
    entry: &IndexEntry,
    favorite_dates: &BTreeSet<NaiveDate>,
) -> String {
    let mut parts = vec![
        if favorite_dates.contains(&entry.date) {
            "FAVORITE".to_string()
        } else {
            "STANDARD".to_string()
        },
        if entry.has_conflict {
            "CONFLICT".to_string()
        } else {
            "CLEAN".to_string()
        },
    ];
    if !entry.metadata.tags.is_empty() {
        parts.push(format!("tags={}", entry.metadata.tags.join(",")));
    }
    if !entry.metadata.people.is_empty() {
        parts.push(format!("people={}", entry.metadata.people.join(",")));
    }
    if let Some(project) = entry.metadata.project.as_deref() {
        parts.push(format!("project={project}"));
    }
    if let Some(mood) = entry.metadata.mood {
        parts.push(format!("mood={mood}"));
    }
    parts.join(" | ")
}

fn macro_key_matches(spec: &str, key: &KeyEvent) -> bool {
    let Some((code, modifiers)) = parse_macro_key_spec(spec) else {
        return false;
    };
    if key.modifiers != modifiers {
        return false;
    }

    match (&key.code, code) {
        (KeyCode::Char(actual), MacroKeyCode::Char(expected)) => {
            actual.eq_ignore_ascii_case(&expected)
        }
        (KeyCode::F(actual), MacroKeyCode::Function(expected)) => *actual == expected,
        (KeyCode::Enter, MacroKeyCode::Enter) => true,
        (KeyCode::Tab, MacroKeyCode::Tab) => true,
        (KeyCode::BackTab, MacroKeyCode::BackTab) => true,
        (KeyCode::Backspace, MacroKeyCode::Backspace) => true,
        (KeyCode::Esc, MacroKeyCode::Esc) => true,
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MacroKeyCode {
    Char(char),
    Function(u8),
    Enter,
    Tab,
    BackTab,
    Backspace,
    Esc,
}

fn parse_macro_key_spec(spec: &str) -> Option<(MacroKeyCode, KeyModifiers)> {
    let normalized = spec.to_ascii_lowercase().replace('+', "-");
    let parts = normalized
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.is_empty() {
        return None;
    }

    let mut modifiers = KeyModifiers::empty();
    let mut code = None;
    for part in parts {
        match part {
            "ctrl" | "control" => modifiers |= KeyModifiers::CONTROL,
            "alt" | "option" => modifiers |= KeyModifiers::ALT,
            "shift" => modifiers |= KeyModifiers::SHIFT,
            "enter" | "return" => code = Some(MacroKeyCode::Enter),
            "tab" => code = Some(MacroKeyCode::Tab),
            "backtab" => code = Some(MacroKeyCode::BackTab),
            "backspace" => code = Some(MacroKeyCode::Backspace),
            "esc" | "escape" => code = Some(MacroKeyCode::Esc),
            value if value.starts_with('f') => {
                let number = value.strip_prefix('f')?.parse::<u8>().ok()?;
                code = Some(MacroKeyCode::Function(number));
            }
            value if value.chars().count() == 1 => {
                code = Some(MacroKeyCode::Char(value.chars().next()?));
            }
            _ => return None,
        }
    }

    code.map(|code| (code, modifiers))
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

fn index_matches_filter(entry: &IndexEntry, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }

    let date = entry.date.format("%Y-%m-%d").to_string();
    let preview = entry.preview.to_ascii_lowercase();
    let tags = entry.metadata.tags.join(" ").to_ascii_lowercase();
    let people = entry.metadata.people.join(" ").to_ascii_lowercase();
    let project = entry
        .metadata
        .project
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mood = entry
        .metadata
        .mood
        .map(|mood| mood.to_string())
        .unwrap_or_default();
    date.to_ascii_lowercase().contains(needle)
        || entry.entry_number.to_ascii_lowercase().contains(needle)
        || preview.contains(needle)
        || tags.contains(needle)
        || people.contains(needle)
        || project.contains(needle)
        || mood.contains(needle)
        || (entry.has_conflict && "conflict".contains(needle))
}

fn previous_entry_date(
    entry_dates: &BTreeSet<NaiveDate>,
    selected: NaiveDate,
) -> Option<NaiveDate> {
    entry_dates.range(..selected).next_back().copied()
}

fn next_entry_date(entry_dates: &BTreeSet<NaiveDate>, selected: NaiveDate) -> Option<NaiveDate> {
    entry_dates
        .range(selected.succ_opt().unwrap_or(selected)..)
        .next()
        .copied()
}

fn previous_entry_month(
    entry_dates: &BTreeSet<NaiveDate>,
    selected: NaiveDate,
) -> Option<NaiveDate> {
    let current_month = (selected.year(), selected.month());
    entry_dates
        .iter()
        .rev()
        .find(|date| (date.year(), date.month()) < current_month)
        .copied()
}

fn next_entry_month(entry_dates: &BTreeSet<NaiveDate>, selected: NaiveDate) -> Option<NaiveDate> {
    let current_month = (selected.year(), selected.month());
    entry_dates
        .iter()
        .find(|date| (date.year(), date.month()) > current_month)
        .copied()
}

#[cfg(test)]
mod tests {
    use super::{
        App, ConflictOverlay, DatePicker, ExportFormatUi, ExportPrompt, IndexState, MenuAction,
        MenuId, Overlay, PickerAction, PickerItem, PickerOverlay, RestorePrompt, SearchField,
        SearchJump, SearchOverlay, SettingField, SettingPrompt, SetupWizard, SyncPhase,
        SyncRequest, SyncStatusOverlay, default_export_path, format_reveal_codes,
        macro_key_matches, parse_optional_overlay_date, resolve_recovery_text,
        soundtrack_cache_extension, soundtrack_cache_path,
    };
    use crate::{
        config::RecentExportInfo,
        search::{SearchDocument, SearchIndex, SearchResult, Snippet},
        tui::buffer::{MatchPos, TextBuffer},
        vault::{self, BackupEntry, ConflictHead, ConflictState, EntryMetadata, IndexEntry},
    };
    use chrono::{Duration, Local, NaiveDate, Utc};
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{Terminal, backend::TestBackend};
    use secrecy::SecretString;
    use std::path::{Path, PathBuf};
    use tempfile::tempdir;

    fn render_into_terminal(terminal: &mut Terminal<TestBackend>, app: &App) -> String {
        terminal
            .draw(|frame| super::super::draw(frame, app))
            .expect("draw");
        let buffer = terminal.backend().buffer();
        let area = *buffer.area();
        let mut lines = Vec::new();
        for y in 0..area.height {
            let mut line = String::new();
            for x in 0..area.width {
                let cell = buffer.cell((x, y)).expect("cell");
                line.push_str(cell.symbol());
            }
            lines.push(line.trim_end().to_string());
        }
        lines.join("\n")
    }

    fn render_app(app: &App, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        render_into_terminal(&mut terminal, app)
    }

    fn assert_editor_invariants(app: &App) {
        let line_count = app.buffer.line_count();
        assert!(
            line_count >= 1,
            "buffer should always contain at least one line"
        );
        let (row, col) = app.buffer.cursor();
        assert!(
            row < line_count,
            "cursor row out of bounds: {row} >= {line_count}"
        );
        let line_len = app
            .buffer
            .line(row)
            .expect("cursor line should exist")
            .chars()
            .count();
        assert!(
            col <= line_len,
            "cursor col out of bounds: {col} > {line_len}"
        );
        assert!(
            app.scroll_row() <= line_count.saturating_sub(1),
            "scroll row out of bounds"
        );
        assert!(
            !app.buffer.to_text().contains('\t'),
            "editor should not store literal tab characters"
        );
    }

    fn send_editor_key(
        app: &mut App,
        code: KeyCode,
        modifiers: KeyModifiers,
        viewport_height: usize,
        viewport_width: usize,
    ) {
        app.handle_event_with_viewport(
            Event::Key(KeyEvent::new(code, modifiers)),
            viewport_height,
            viewport_width,
        );
        assert_editor_invariants(app);
    }

    fn type_editor_text(app: &mut App, text: &str, viewport_height: usize, viewport_width: usize) {
        for ch in text.chars() {
            match ch {
                '\n' => send_editor_key(
                    app,
                    KeyCode::Enter,
                    KeyModifiers::empty(),
                    viewport_height,
                    viewport_width,
                ),
                '\t' => send_editor_key(
                    app,
                    KeyCode::Tab,
                    KeyModifiers::empty(),
                    viewport_height,
                    viewport_width,
                ),
                _ => send_editor_key(
                    app,
                    KeyCode::Char(ch),
                    KeyModifiers::empty(),
                    viewport_height,
                    viewport_width,
                ),
            }
        }
    }

    fn build_unlocked_test_app(vault_root: &Path, start_date: NaiveDate) -> App {
        let passphrase = SecretString::new("test-passphrase".to_string().into_boxed_str());
        let metadata =
            vault::create_vault(vault_root, &passphrase, Some(start_date), "Test Device")
                .expect("create test vault");
        let device_id = metadata.device_id.clone();
        let unlocked = vault::unlock_vault_with_device(vault_root, &passphrase, device_id.clone())
            .expect("unlock test vault");

        let mut app = App::with_initial_date(Some(start_date));
        app.overlay = None;
        app.vault_path = vault_root.to_path_buf();
        app.config.vault_path = vault_root.to_path_buf();
        app.config.local_device_id = Some(device_id);
        app.config.device_nickname = "Test Device".to_string();
        app.vault = Some(unlocked);
        app.load_selected_date();
        assert_editor_invariants(&app);
        app
    }

    #[test]
    fn resize_event_does_not_panic() {
        let mut app = App::new();
        app.handle_event(Event::Resize(80, 25), 23);
        assert_eq!(app.scroll_row(), 0);
    }

    #[test]
    fn human_like_batch_entry_flow_saves_and_reloads_many_days() {
        let temp = tempdir().expect("tempdir");
        let start = NaiveDate::from_ymd_opt(2026, 1, 1).expect("date");
        let mut app = build_unlocked_test_app(&temp.path().join("vault"), start);
        let viewport_height = 20;
        let viewport_width = 80;

        for day in 0..28 {
            if day > 0 {
                send_editor_key(
                    &mut app,
                    KeyCode::Right,
                    KeyModifiers::ALT,
                    viewport_height,
                    viewport_width,
                );
            }
            assert_eq!(app.selected_date, start + Duration::days(day));
            type_editor_text(
                &mut app,
                &format!("Day {day:02} entry\tfocus\nSecond line for realism."),
                viewport_height,
                viewport_width,
            );
            send_editor_key(
                &mut app,
                KeyCode::F(2),
                KeyModifiers::empty(),
                viewport_height,
                viewport_width,
            );
        }

        app.open_date(start + Duration::days(7));
        app.buffer.move_to_bottom();
        app.buffer.move_to_line_end();
        type_editor_text(
            &mut app,
            "\nFollow-up note.",
            viewport_height,
            viewport_width,
        );
        send_editor_key(
            &mut app,
            KeyCode::F(2),
            KeyModifiers::empty(),
            viewport_height,
            viewport_width,
        );

        let vault = app.vault.as_ref().expect("vault");
        let dates = vault.list_entry_dates().expect("list dates");
        assert_eq!(dates.len(), 28);
        assert_eq!(dates.first().copied(), Some(start));
        assert_eq!(dates.last().copied(), Some(start + Duration::days(27)));

        for day in [0, 7, 13, 27] {
            let date = start + Duration::days(day);
            let exported = vault
                .export_entry(date)
                .expect("export entry")
                .expect("entry exists");
            assert!(exported.body.contains(&format!("Day {day:02} entry")));
            assert!(!exported.body.contains('\t'));
        }
        let day_seven = vault
            .export_entry(start + Duration::days(7))
            .expect("export day seven")
            .expect("entry exists");
        assert!(day_seven.body.contains("Follow-up note."));
    }

    #[test]
    fn mixed_input_stress_session_preserves_editor_invariants() {
        let mut app = App::with_initial_date(None);
        app.overlay = None;

        for step in 0..400usize {
            let width = 56 + (step % 35) as u16;
            let height = 18 + (step % 10) as u16;
            let viewport_width = width as usize;
            let viewport_height = app.editor_viewport_height(height.saturating_sub(3) as usize);

            match step % 16 {
                0 => send_editor_key(
                    &mut app,
                    KeyCode::Char((b'a' + (step % 26) as u8) as char),
                    KeyModifiers::empty(),
                    viewport_height,
                    viewport_width,
                ),
                1 => send_editor_key(
                    &mut app,
                    KeyCode::Char(' '),
                    KeyModifiers::empty(),
                    viewport_height,
                    viewport_width,
                ),
                2 => send_editor_key(
                    &mut app,
                    KeyCode::Tab,
                    KeyModifiers::empty(),
                    viewport_height,
                    viewport_width,
                ),
                3 => send_editor_key(
                    &mut app,
                    KeyCode::Char('\t'),
                    KeyModifiers::empty(),
                    viewport_height,
                    viewport_width,
                ),
                4 => send_editor_key(
                    &mut app,
                    KeyCode::Enter,
                    KeyModifiers::empty(),
                    viewport_height,
                    viewport_width,
                ),
                5 => send_editor_key(
                    &mut app,
                    KeyCode::Backspace,
                    KeyModifiers::empty(),
                    viewport_height,
                    viewport_width,
                ),
                6 => send_editor_key(
                    &mut app,
                    KeyCode::Delete,
                    KeyModifiers::empty(),
                    viewport_height,
                    viewport_width,
                ),
                7 => send_editor_key(
                    &mut app,
                    KeyCode::Left,
                    KeyModifiers::empty(),
                    viewport_height,
                    viewport_width,
                ),
                8 => send_editor_key(
                    &mut app,
                    KeyCode::Right,
                    KeyModifiers::empty(),
                    viewport_height,
                    viewport_width,
                ),
                9 => send_editor_key(
                    &mut app,
                    KeyCode::Up,
                    KeyModifiers::empty(),
                    viewport_height,
                    viewport_width,
                ),
                10 => send_editor_key(
                    &mut app,
                    KeyCode::Down,
                    KeyModifiers::empty(),
                    viewport_height,
                    viewport_width,
                ),
                11 => send_editor_key(
                    &mut app,
                    KeyCode::PageUp,
                    KeyModifiers::empty(),
                    viewport_height,
                    viewport_width,
                ),
                12 => send_editor_key(
                    &mut app,
                    KeyCode::PageDown,
                    KeyModifiers::empty(),
                    viewport_height,
                    viewport_width,
                ),
                13 => send_editor_key(
                    &mut app,
                    KeyCode::Home,
                    KeyModifiers::empty(),
                    viewport_height,
                    viewport_width,
                ),
                14 => send_editor_key(
                    &mut app,
                    KeyCode::End,
                    KeyModifiers::empty(),
                    viewport_height,
                    viewport_width,
                ),
                _ => {
                    app.handle_event_with_viewport(
                        Event::Resize(width, height),
                        viewport_height,
                        viewport_width,
                    );
                    assert_editor_invariants(&app);
                }
            }

            if step % 25 == 0 {
                let _ = render_app(&app, width, height);
            }
        }
    }

    #[test]
    fn typing_wraps_when_line_exceeds_viewport_width() {
        let mut app = App::with_initial_date(None);
        app.overlay = None;

        for _ in 0..11 {
            app.handle_event_with_viewport(
                Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::empty())),
                20,
                10,
            );
        }

        assert_eq!(app.buffer.to_text(), "aaaaaaaaaa\na");
        assert_eq!(app.buffer.cursor(), (1, 1));
    }

    #[test]
    fn tab_key_inserts_five_spaces() {
        let mut app = App::with_initial_date(None);
        app.overlay = None;

        app.handle_event_with_viewport(
            Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::empty())),
            20,
            80,
        );

        assert_eq!(app.buffer.to_text(), "     ");
        assert_eq!(app.buffer.cursor(), (0, 5));
    }

    #[test]
    fn tab_key_respects_wrap_width() {
        let mut app = App::with_initial_date(None);
        app.overlay = None;

        app.handle_event_with_viewport(
            Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::empty())),
            20,
            4,
        );

        assert_eq!(app.buffer.to_text(), "    \n ");
        assert_eq!(app.buffer.cursor(), (1, 1));
    }

    #[test]
    fn tab_character_event_inserts_five_spaces() {
        let mut app = App::with_initial_date(None);
        app.overlay = None;

        app.handle_event_with_viewport(
            Event::Key(KeyEvent::new(KeyCode::Char('\t'), KeyModifiers::empty())),
            20,
            80,
        );

        assert_eq!(app.buffer.to_text(), "     ");
        assert_eq!(app.buffer.cursor(), (0, 5));
    }

    #[test]
    fn menu_insert_divider_wraps_previous_line_even_when_cursor_advances() {
        let mut app = App::with_initial_date(None);
        app.overlay = None;
        app.last_viewport_width = 12;
        app.buffer = TextBuffer::from_text("");

        app.perform_menu_action(MenuAction::InsertDivider, 20);

        let first_line_len = app
            .buffer
            .line(0)
            .map(|line| line.chars().count())
            .unwrap_or_default();
        assert!(app.buffer.line_count() >= 2);
        assert!(first_line_len <= 12);
    }

    #[test]
    fn tab_input_never_stores_literal_tab_byte() {
        let mut app = App::with_initial_date(None);
        app.overlay = None;

        app.handle_event_with_viewport(
            Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::empty())),
            20,
            80,
        );
        app.handle_event_with_viewport(
            Event::Key(KeyEvent::new(KeyCode::Char('\t'), KeyModifiers::empty())),
            20,
            80,
        );

        assert_eq!(app.buffer.to_text(), "          ");
        assert!(!app.buffer.to_text().contains('\t'));
        assert_eq!(app.buffer.cursor(), (0, 10));
    }

    #[test]
    fn backspace_after_tab_removes_single_space() {
        let mut app = App::with_initial_date(None);
        app.overlay = None;

        app.handle_event_with_viewport(
            Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::empty())),
            20,
            80,
        );
        app.handle_event_with_viewport(
            Event::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::empty())),
            20,
            80,
        );

        assert_eq!(app.buffer.to_text(), "    ");
        assert_eq!(app.buffer.cursor(), (0, 4));
    }

    #[test]
    fn wrap_uses_latest_viewport_width_after_resize() {
        let mut app = App::with_initial_date(None);
        app.overlay = None;

        for ch in "abcdefghij".chars() {
            app.handle_event_with_viewport(
                Event::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::empty())),
                20,
                10,
            );
        }
        app.handle_event_with_viewport(Event::Resize(6, 25), 20, 6);
        app.handle_event_with_viewport(
            Event::Key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::empty())),
            20,
            6,
        );

        assert_eq!(app.buffer.to_text(), "abcdef\nghijk");
        assert_eq!(app.buffer.cursor(), (1, 5));
    }

    #[test]
    fn typing_wraps_at_word_boundary_in_editor_flow() {
        let mut app = App::with_initial_date(None);
        app.overlay = None;

        for ch in "hello world".chars() {
            app.handle_event_with_viewport(
                Event::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::empty())),
                20,
                10,
            );
        }

        assert_eq!(app.buffer.to_text(), "hello \nworld");
        assert_eq!(app.buffer.cursor(), (1, 5));
    }

    #[test]
    fn wrapped_line_reflows_after_backspace_merge() {
        let mut app = App::with_initial_date(None);
        app.overlay = None;
        app.buffer = TextBuffer::from_text("12345\n6789");
        app.buffer.set_cursor(1, 0);

        app.handle_event_with_viewport(
            Event::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::empty())),
            20,
            6,
        );

        assert_eq!(app.buffer.to_text(), "123456\n789");
        assert_eq!(app.buffer.cursor(), (0, 5));
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
                metadata: EntryMetadata::default(),
            })
            .collect();
        let index = IndexState::new(items, date + Duration::days(5), Default::default());
        assert_eq!(index.window(5), (3, 8));
    }

    #[test]
    fn app_respects_initial_open_date() {
        let initial = NaiveDate::from_ymd_opt(2026, 4, 2).expect("date");
        let app = App::with_initial_date(Some(initial));
        assert!(app.header_entry_focus_label().contains("2026-04-02"));
    }

    #[test]
    fn escape_opens_file_menu_when_editor_is_active() {
        let mut app = App::with_initial_date(None);
        app.overlay = None;

        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty())),
            20,
        );

        assert!(matches!(
            app.menu(),
            Some(menu) if menu.selected_menu == MenuId::File
        ));
    }

    #[test]
    fn alt_letter_opens_target_menu_when_editor_is_active() {
        let mut app = App::with_initial_date(None);
        app.overlay = None;

        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::ALT)),
            20,
        );

        assert!(matches!(
            app.menu(),
            Some(menu) if menu.selected_menu == MenuId::Search
        ));
    }

    #[test]
    fn ctrl_o_opens_file_menu_when_editor_is_active() {
        let mut app = App::with_initial_date(None);
        app.overlay = None;

        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL)),
            20,
        );

        assert!(matches!(
            app.menu(),
            Some(menu) if menu.selected_menu == MenuId::File
        ));
    }

    #[test]
    fn keybinding_function_keys_route_to_expected_actions() {
        let mut app = App::with_initial_date(None);
        app.overlay = None;

        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::F(1), KeyModifiers::empty())),
            20,
        );
        assert!(matches!(app.overlay(), Some(Overlay::Help)));

        app.overlay = None;
        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::F(3), KeyModifiers::empty())),
            20,
        );
        assert!(matches!(app.overlay(), Some(Overlay::DatePicker(_))));

        app.overlay = None;
        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::F(4), KeyModifiers::empty())),
            20,
        );
        assert!(matches!(app.overlay(), Some(Overlay::FindPrompt { .. })));

        app.overlay = None;
        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::F(5), KeyModifiers::empty())),
            20,
        );
        assert!(matches!(app.overlay(), Some(Overlay::Search(_))));

        app.overlay = None;
        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::F(6), KeyModifiers::empty())),
            20,
        );
        assert!(matches!(app.overlay(), Some(Overlay::ReplacePrompt(_))));

        app.overlay = None;
        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::F(7), KeyModifiers::empty())),
            20,
        );
        assert!(matches!(app.overlay(), Some(Overlay::Index(_))));

        app.overlay = None;
        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::F(8), KeyModifiers::empty())),
            20,
        );
        assert_eq!(app.status_text(), Some("LOCKED."));

        app.overlay = None;
        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::F(9), KeyModifiers::empty())),
            20,
        );
        assert!(matches!(app.overlay(), Some(Overlay::ClosingPrompt { .. })));

        app.overlay = None;
        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::F(10), KeyModifiers::empty())),
            20,
        );
        assert!(matches!(app.overlay(), Some(Overlay::QuitConfirm)));

        app.overlay = None;
        let reveal_before = app.reveal_codes_enabled();
        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::F(11), KeyModifiers::empty())),
            20,
        );
        assert_ne!(app.reveal_codes_enabled(), reveal_before);
    }

    #[test]
    fn keybinding_ctrl_fallbacks_route_to_expected_actions() {
        let mut app = App::with_initial_date(None);
        app.overlay = None;

        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL)),
            20,
        );
        assert!(matches!(app.overlay(), Some(Overlay::FindPrompt { .. })));

        app.overlay = None;
        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
            20,
        );
        assert_eq!(app.status_text(), Some("LOCKED."));

        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL)),
            20,
        );
        assert!(matches!(
            app.menu(),
            Some(menu) if menu.selected_menu == MenuId::File
        ));

        app.menu = None;
        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL)),
            20,
        );
        assert!(matches!(app.overlay(), Some(Overlay::Picker(_))));
    }

    #[test]
    fn keybinding_ctrl_menu_hotkeys_open_all_top_menus() {
        let mut app = App::with_initial_date(None);
        app.overlay = None;

        let cases = [
            ('o', MenuId::File),
            ('e', MenuId::Edit),
            ('w', MenuId::Search),
            ('y', MenuId::Go),
            ('t', MenuId::Tools),
            ('u', MenuId::Setup),
            ('l', MenuId::Help),
        ];

        for (key, expected_menu) in cases {
            app.menu = None;
            app.handle_event(
                Event::Key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::CONTROL)),
                20,
            );
            assert!(matches!(
                app.menu(),
                Some(menu) if menu.selected_menu == expected_menu
            ));
        }
    }

    #[test]
    fn keybinding_f12_locks_when_vault_is_unlocked() {
        let temp = tempdir().expect("tempdir");
        let start = NaiveDate::from_ymd_opt(2026, 1, 1).expect("date");
        let mut app = build_unlocked_test_app(&temp.path().join("vault"), start);
        app.overlay = None;
        assert!(app.vault.is_some());

        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::F(12), KeyModifiers::empty())),
            20,
        );

        assert!(app.vault.is_none());
        assert!(matches!(app.overlay(), Some(Overlay::UnlockPrompt { .. })));
        assert_eq!(app.lock_status_label(), "LOCKED");
    }

    #[test]
    fn esc_in_setup_wizard_does_not_quit_app() {
        let mut app = App::with_initial_date(None);
        app.overlay = Some(Overlay::SetupWizard(SetupWizard::new(Path::new(
            "/tmp/bsj-vault",
        ))));

        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty())),
            20,
        );

        assert!(!app.should_quit());
        assert!(matches!(app.overlay(), Some(Overlay::SetupWizard(_))));
        assert_eq!(app.status_text(), Some("SETUP ACTIVE. CTRL+Q QUITS."));
    }

    #[test]
    fn esc_in_unlock_prompt_does_not_quit_app() {
        let mut app = App::with_initial_date(None);
        app.overlay = Some(Overlay::UnlockPrompt {
            input: String::new(),
            error: None,
        });

        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty())),
            20,
        );

        assert!(!app.should_quit());
        assert!(matches!(app.overlay(), Some(Overlay::UnlockPrompt { .. })));
        assert_eq!(
            app.status_text(),
            Some("UNLOCK PROMPT ACTIVE. CTRL+Q QUITS.")
        );
    }

    #[test]
    fn ctrl_q_quits_even_when_overlay_is_open() {
        let mut app = App::with_initial_date(None);
        app.overlay = Some(Overlay::SetupWizard(SetupWizard::new(Path::new(
            "/tmp/bsj-vault",
        ))));

        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL)),
            20,
        );

        assert!(app.should_quit());
    }

    #[test]
    fn alt_n_opens_next_new_entry_date() {
        let start = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");
        let mut app = App::with_initial_date(Some(start));
        app.overlay = None;

        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::ALT)),
            20,
        );

        assert_eq!(app.selected_date, start + Duration::days(1));
        assert_eq!(app.status_text(), Some("NEW ENTRY 2026-03-17."));
    }

    #[test]
    fn alt_n_skips_existing_saved_dates() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().join("vault");
        let passphrase =
            SecretString::new("correct horse battery staple".to_string().into_boxed_str());
        let metadata = vault::create_vault(&root, &passphrase, None, "Test").expect("create");
        let unlocked = vault::unlock_vault_with_device(&root, &passphrase, metadata.device_id)
            .expect("unlock");
        let start = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");
        unlocked
            .save_revision(start + Duration::days(1), "next day")
            .expect("save next day");
        unlocked
            .save_revision(start + Duration::days(2), "day after")
            .expect("save day after");

        let mut app = App::with_initial_date(Some(start));
        app.overlay = None;
        app.vault_path = root;
        app.vault = Some(unlocked);

        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::ALT)),
            20,
        );

        assert_eq!(app.selected_date, start + Duration::days(3));
        assert_eq!(app.status_text(), Some("NEW ENTRY 2026-03-19."));
    }

    #[test]
    fn alt_left_no_longer_changes_selected_date() {
        let start = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");
        let mut app = App::with_initial_date(Some(start));
        app.overlay = None;

        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT)),
            20,
        );

        assert_eq!(app.selected_date, start);
    }

    #[test]
    fn header_focus_label_marks_today_archive_and_next() {
        let today = Local::now().date_naive();
        let mut app = App::with_initial_date(Some(today));
        assert!(app.header_entry_focus_label().starts_with("TODAY "));

        app.selected_date = today - Duration::days(1);
        assert!(app.header_entry_focus_label().starts_with("ARCHIVE "));

        app.selected_date = today + Duration::days(1);
        assert!(app.header_entry_focus_label().starts_with("NEXT "));
    }

    #[test]
    fn menu_navigation_moves_between_sections() {
        let mut app = App::with_initial_date(None);
        app.overlay = None;
        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty())),
            20,
        );
        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::empty())),
            20,
        );

        assert!(matches!(
            app.menu(),
            Some(menu) if menu.selected_menu == MenuId::Edit
        ));
    }

    #[test]
    fn setup_menu_lists_live_settings() {
        let app = App::with_initial_date(None);
        let items = app.menu_items(MenuId::Setup);
        let labels = items
            .iter()
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>();

        assert!(labels.contains(&"Vault Path"));
        assert!(labels.contains(&"Sync Folder"));
        assert!(labels.contains(&"Device Name"));
        assert!(labels.contains(&"Daily Backups"));
    }

    #[test]
    fn file_menu_lists_export_and_backup_history() {
        let app = App::with_initial_date(None);
        let labels = app
            .menu_items(MenuId::File)
            .into_iter()
            .map(|item| item.label)
            .collect::<Vec<_>>();

        assert!(labels.contains(&"Export Current".to_string()));
        assert!(labels.contains(&"Backup History".to_string()));
        assert!(labels.contains(&"Backup Cleanup Preview".to_string()));
    }

    #[test]
    fn tools_menu_lists_dashboard() {
        let app = App::with_initial_date(None);
        let labels = app
            .menu_items(MenuId::Tools)
            .into_iter()
            .map(|item| item.label)
            .collect::<Vec<_>>();

        assert!(labels.contains(&"Status Dashboard".to_string()));
    }

    #[test]
    fn disabled_menu_item_does_not_execute() {
        let mut app = App::with_initial_date(None);
        app.overlay = None;
        app.menu = Some(super::MenuState {
            selected_menu: MenuId::File,
            selected_item: 0,
        });
        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty())),
            20,
        );

        assert!(app.menu().is_some());
        assert_eq!(app.status_text(), Some("UNAVAILABLE."));
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
    fn search_scope_presets_update_overlay_dates() {
        let today = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");
        let mut overlay = SearchOverlay::new(None);
        overlay.apply_today_scope(today);
        assert_eq!(overlay.from_input, "2026-03-16");
        assert_eq!(overlay.to_input, "2026-03-16");

        overlay.apply_week_scope(today);
        assert_eq!(overlay.from_input, "2026-03-10");
        assert_eq!(overlay.to_input, "2026-03-16");

        overlay.apply_month_scope(today);
        assert_eq!(overlay.from_input, "2026-03-01");
        assert_eq!(overlay.to_input, "2026-03-16");

        overlay.apply_year_scope(today);
        assert_eq!(overlay.from_input, "2026-01-01");
        assert_eq!(overlay.to_input, "2026-03-16");

        overlay.clear_filters();
        assert!(overlay.from_input.is_empty());
        assert!(overlay.to_input.is_empty());
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

    #[test]
    fn reveal_codes_format_tags_mood_and_closing() {
        let line = format_reveal_codes(
            NaiveDate::from_ymd_opt(2026, 3, 16).expect("date"),
            "0000016",
            &EntryMetadata::default(),
            "Planning #work mood:7 before dusk",
            Some("See you tomorrow."),
        );
        assert!(line.contains("⟦TAG:work⟧"));
        assert!(line.contains("⟦MOOD:7⟧"));
        assert!(line.contains("⟦CLOSE:See you tomorrow.⟧"));
    }

    #[test]
    fn export_prompt_uses_date_based_default_filename() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");
        let prompt = ExportPrompt::new(date);
        assert!(prompt.path_input.ends_with("bsj-2026-03-16.txt"));
        assert_eq!(
            default_export_path(date, prompt.format)
                .file_name()
                .and_then(|name| name.to_str()),
            Some("bsj-2026-03-16.txt")
        );
    }

    #[test]
    fn date_picker_jump_input_parses_and_selects_date() {
        let mut picker = DatePicker::new(
            NaiveDate::from_ymd_opt(2026, 3, 16).expect("date"),
            Default::default(),
        );
        picker.jump_input = "2026-04-02".to_string();
        let selected = picker.apply_jump_input().expect("jump");
        assert_eq!(selected, NaiveDate::from_ymd_opt(2026, 4, 2).expect("date"));
        assert!(picker.jump_input.is_empty());
    }

    #[test]
    fn index_state_filter_reduces_visible_items() {
        let selected = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");
        let items = vec![
            IndexEntry {
                date: selected,
                entry_number: "0000001".to_string(),
                preview: "Quiet morning".to_string(),
                has_conflict: false,
                metadata: EntryMetadata::default(),
            },
            IndexEntry {
                date: selected + Duration::days(1),
                entry_number: "0000002".to_string(),
                preview: "Conflict review".to_string(),
                has_conflict: true,
                metadata: EntryMetadata {
                    tags: vec!["work".to_string()],
                    people: Vec::new(),
                    project: None,
                    mood: Some(7),
                },
            },
        ];
        let mut index = IndexState::new(items, selected, Default::default());
        index.push_filter_char('c', selected);
        index.push_filter_char('o', selected);

        assert_eq!(index.items.len(), 1);
        assert!(index.items[0].has_conflict);
    }

    #[test]
    fn index_state_sort_toggle_reverses_order() {
        let selected = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");
        let items = vec![
            IndexEntry {
                date: selected + Duration::days(1),
                entry_number: "0000002".to_string(),
                preview: "Later".to_string(),
                has_conflict: false,
                metadata: EntryMetadata::default(),
            },
            IndexEntry {
                date: selected,
                entry_number: "0000001".to_string(),
                preview: "Earlier".to_string(),
                has_conflict: false,
                metadata: EntryMetadata::default(),
            },
        ];
        let mut index = IndexState::new(items, selected + Duration::days(1), Default::default());
        index.toggle_sort(selected + Duration::days(1));

        assert_eq!(index.items.first().map(|entry| entry.date), Some(selected));
        assert!(index.sort_oldest_first);
    }

    #[test]
    fn index_state_can_filter_to_favorites_only() {
        let selected = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");
        let items = vec![
            IndexEntry {
                date: selected,
                entry_number: "0000001".to_string(),
                preview: "Starred".to_string(),
                has_conflict: false,
                metadata: EntryMetadata::default(),
            },
            IndexEntry {
                date: selected + Duration::days(1),
                entry_number: "0000002".to_string(),
                preview: "Plain".to_string(),
                has_conflict: false,
                metadata: EntryMetadata::default(),
            },
        ];
        let mut favorites = std::collections::BTreeSet::new();
        favorites.insert(selected);
        let mut index = IndexState::new(items, selected, favorites);
        index.toggle_favorites_only(selected);

        assert_eq!(index.items.len(), 1);
        assert_eq!(index.items[0].date, selected);
    }

    #[test]
    fn index_state_can_filter_to_conflicts_only() {
        let selected = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");
        let items = vec![
            IndexEntry {
                date: selected,
                entry_number: "0000001".to_string(),
                preview: "Clean".to_string(),
                has_conflict: false,
                metadata: EntryMetadata::default(),
            },
            IndexEntry {
                date: selected + Duration::days(1),
                entry_number: "0000002".to_string(),
                preview: "Conflict".to_string(),
                has_conflict: true,
                metadata: EntryMetadata::default(),
            },
        ];
        let mut index = IndexState::new(items, selected, Default::default());
        index.toggle_conflicts_only(selected);

        assert_eq!(index.items.len(), 1);
        assert!(index.items[0].has_conflict);
    }

    #[test]
    fn footer_context_prefers_find_progress_over_cursor_position() {
        let mut app = App::with_initial_date(None);
        app.overlay = None;
        app.find_matches = vec![
            MatchPos {
                row: 0,
                start_col: 0,
                end_col: 4,
            },
            MatchPos {
                row: 1,
                start_col: 2,
                end_col: 6,
            },
        ];
        app.current_match_idx = 1;

        assert_eq!(app.footer_context_label(), "FIND 2/2");
    }

    #[test]
    fn live_global_search_updates_results_after_query_edit() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");
        let mut app = App::with_initial_date(None);
        app.overlay = None;
        app.search_index = Some(SearchIndex::build(vec![
            SearchDocument {
                date,
                entry_number: "0000001".to_string(),
                body: "quiet morning notes".to_string(),
            },
            SearchDocument {
                date: date + Duration::days(1),
                entry_number: "0000002".to_string(),
                body: "stormy evening".to_string(),
            },
        ]));
        let mut overlay = SearchOverlay::new(None);
        overlay.query_input = "quiet".to_string();

        app.maybe_live_run_global_search(&mut overlay);

        assert_eq!(overlay.results.len(), 1);
        assert_eq!(overlay.active_field, SearchField::Query);
    }

    #[test]
    fn manual_search_enter_focuses_results() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");
        let mut app = App::with_initial_date(None);
        app.overlay = None;
        app.search_index = Some(SearchIndex::build(vec![SearchDocument {
            date,
            entry_number: "0000001".to_string(),
            body: "quiet morning notes".to_string(),
        }]));
        let mut overlay = SearchOverlay::new(Some("quiet".to_string()));

        app.run_global_search(&mut overlay, true);

        assert_eq!(overlay.results.len(), 1);
        assert_eq!(overlay.active_field, SearchField::Results);
    }

    #[test]
    fn run_global_search_keeps_selected_result_when_still_present() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");
        let mut app = App::with_initial_date(None);
        app.overlay = None;
        app.search_index = Some(SearchIndex::build(vec![
            SearchDocument {
                date,
                entry_number: "0000001".to_string(),
                body: "quiet morning notes".to_string(),
            },
            SearchDocument {
                date: date + Duration::days(1),
                entry_number: "0000002".to_string(),
                body: "quiet evening notes".to_string(),
            },
        ]));
        let mut overlay = SearchOverlay::new(Some("quiet".to_string()));
        app.run_global_search(&mut overlay, true);
        overlay.selected = 1;
        overlay.active_field = SearchField::Results;

        app.run_global_search(&mut overlay, true);

        assert_eq!(overlay.results.len(), 2);
        assert_eq!(overlay.selected, 1);
        assert_eq!(overlay.active_field, SearchField::Results);
    }

    #[test]
    fn search_overlay_ctrl_clear_resets_query_filters_and_results() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");
        let mut app = App::with_initial_date(None);
        app.overlay = Some(Overlay::Search(SearchOverlay {
            query_input: "quiet".to_string(),
            from_input: "2026-03-01".to_string(),
            to_input: "2026-03-16".to_string(),
            active_field: SearchField::Query,
            results: vec![SearchResult {
                date,
                entry_number: "0000001".to_string(),
                snippet: Snippet {
                    text: "quiet morning notes".to_string(),
                    highlight_start: 0,
                    highlight_end: 5,
                },
                row: 0,
                start_col: 0,
                end_col: 5,
                matched_text: "quiet".to_string(),
            }],
            selected: 0,
            error: Some("No matches.".to_string()),
        }));

        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL)),
            23,
        );

        match app.overlay() {
            Some(Overlay::Search(search)) => {
                assert!(search.query_input.is_empty());
                assert!(search.from_input.is_empty());
                assert!(search.to_input.is_empty());
                assert!(search.results.is_empty());
                assert_eq!(search.active_field, SearchField::Query);
            }
            other => panic!("expected search overlay, got {other:?}"),
        }
    }

    #[test]
    fn search_overlay_ctrl_recall_uses_recent_query() {
        let mut app = App::with_initial_date(None);
        app.search_history = vec!["stormy".to_string(), "quiet".to_string()];
        app.search_index = Some(SearchIndex::build(vec![SearchDocument {
            date: NaiveDate::from_ymd_opt(2026, 3, 16).expect("date"),
            entry_number: "0000001".to_string(),
            body: "stormy evening".to_string(),
        }]));
        app.overlay = Some(Overlay::Search(SearchOverlay::new(None)));

        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL)),
            23,
        );

        match app.overlay() {
            Some(Overlay::Search(search)) => {
                assert_eq!(search.query_input, "stormy");
                assert_eq!(search.results.len(), 1);
            }
            other => panic!("expected search overlay, got {other:?}"),
        }
    }

    #[test]
    fn macro_key_matcher_handles_ctrl_and_function_keys() {
        let ctrl_j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL);
        let f11 = KeyEvent::new(KeyCode::F(11), KeyModifiers::empty());
        assert!(macro_key_matches("ctrl-j", &ctrl_j));
        assert!(macro_key_matches("f11", &f11));
        assert!(!macro_key_matches("ctrl-k", &ctrl_j));
    }

    #[test]
    fn setup_menu_lists_daily_word_goal_setting() {
        let app = App::with_initial_date(None);
        let labels = app
            .menu_items(MenuId::Setup)
            .into_iter()
            .map(|item| item.label)
            .collect::<Vec<_>>();

        assert!(labels.contains(&"Daily Word Goal".to_string()));
        assert!(labels.contains(&"Soundtrack URL/Path".to_string()));
    }

    #[test]
    fn tools_menu_lists_command_palette_and_prompts() {
        let app = App::with_initial_date(None);
        let labels = app
            .menu_items(MenuId::Tools)
            .into_iter()
            .map(|item| item.label)
            .collect::<Vec<_>>();

        assert!(labels.contains(&"Command Palette".to_string()));
        assert!(labels.contains(&"Writing Prompts".to_string()));
        assert!(labels.contains(&"Soundtrack Source".to_string()));
        assert!(labels.contains(&"Toggle Soundtrack".to_string()));
    }

    #[test]
    fn soundtrack_toggle_requires_source() {
        let mut app = App::with_initial_date(None);
        app.config.soundtrack_source.clear();

        app.toggle_soundtrack_playback();

        assert!(!app.soundtrack_loop_enabled);
        assert_eq!(app.status_text(), Some("SET SOUNDTRACK SOURCE."));
        assert!(matches!(app.overlay(), Some(Overlay::SettingPrompt(_))));
    }

    #[test]
    fn help_menu_lists_about_entry() {
        let app = App::with_initial_date(None);
        let labels = app
            .menu_items(MenuId::Help)
            .into_iter()
            .map(|item| item.label)
            .collect::<Vec<_>>();

        assert!(labels.contains(&"About BlueScreen Journal".to_string()));
    }

    #[test]
    fn about_action_opens_info_overlay() {
        let mut app = App::with_initial_date(None);

        app.perform_menu_action(MenuAction::About, 20);

        match app.overlay() {
            Some(Overlay::Info(info)) => {
                assert_eq!(info.title, "About");
                let rendered = info.lines.join("\n");
                assert!(rendered.contains("Awassee LLC and Sean Heiney"));
                assert!(rendered.contains("About BlueScreen Journal"));
                assert!(rendered.contains("Toggle Soundtrack"));
            }
            other => panic!("expected about info overlay, got {other:?}"),
        }
    }

    #[test]
    fn soundtrack_cache_extension_prefers_url_extension_and_sanitizes() {
        assert_eq!(
            soundtrack_cache_extension("https://example.com/theme.MID?download=1"),
            "mid"
        );
        assert_eq!(
            soundtrack_cache_extension("https://example.com/theme.bad-ext!?q=1"),
            "badext"
        );
        assert_eq!(
            soundtrack_cache_extension("https://example.com/no-extension"),
            "mid"
        );
    }

    #[test]
    fn soundtrack_cache_path_is_stable_for_same_url() {
        let cache_dir = Path::new("/tmp/bsj-soundtrack-cache-test");
        let url = "https://example.com/theme.mid";
        let first = soundtrack_cache_path(cache_dir, url);
        let second = soundtrack_cache_path(cache_dir, url);
        let other = soundtrack_cache_path(cache_dir, "https://example.com/other.mid");

        assert_eq!(first, second);
        assert_ne!(first, other);
        assert_eq!(first.parent(), Some(cache_dir));
    }

    #[test]
    fn picker_overlay_filters_by_title_detail_and_keywords() {
        let mut picker = PickerOverlay::new(
            "Commands",
            vec![
                PickerItem {
                    title: "Command Palette".to_string(),
                    detail: "CTRL+K".to_string(),
                    keywords: "tools commands".to_string(),
                    action: PickerAction::Menu(MenuAction::CommandPalette),
                },
                PickerItem {
                    title: "Sync History".to_string(),
                    detail: "LAST 10".to_string(),
                    keywords: "tools sync".to_string(),
                    action: PickerAction::Menu(MenuAction::SyncHistory),
                },
            ],
            "none",
        );
        picker.push_filter_char('s');
        picker.push_filter_char('y');

        assert_eq!(picker.filtered_indices().len(), 1);
        assert_eq!(
            picker.selected_item().map(|item| item.title.as_str()),
            Some("Sync History")
        );
    }

    #[test]
    fn remember_recent_date_dedupes_and_keeps_latest_first() {
        let mut app = App::with_initial_date(None);
        let first = NaiveDate::from_ymd_opt(2026, 3, 14).expect("date");
        let second = NaiveDate::from_ymd_opt(2026, 3, 15).expect("date");

        app.remember_recent_date(first);
        app.remember_recent_date(second);
        app.remember_recent_date(first);

        assert_eq!(app.recent_dates, vec![first, second]);
    }

    #[test]
    fn remember_search_query_dedupes_and_trims() {
        let mut app = App::with_initial_date(None);
        app.remember_search_query("  weather  ");
        app.remember_search_query("weather");
        app.remember_search_query("mood");

        assert_eq!(
            app.search_history,
            vec!["mood".to_string(), "weather".to_string()]
        );
    }

    #[test]
    fn record_export_history_dedupes_by_path() {
        let mut app = App::with_initial_date(None);
        let path = PathBuf::from("/tmp/export.txt");
        app.record_export_history(ExportFormatUi::PlainText, &path);
        app.record_export_history(ExportFormatUi::PlainText, &path);

        assert_eq!(app.config.export_history.len(), 1);
        assert_eq!(app.config.export_history[0].path, "/tmp/export.txt");
    }

    #[test]
    fn export_history_picker_opens_prefilled_export_prompt() {
        let mut app =
            App::with_initial_date(Some(NaiveDate::from_ymd_opt(2026, 3, 18).expect("date")));
        app.config.export_history = vec![RecentExportInfo {
            timestamp: "2026-03-18T10:15:00-04:00".to_string(),
            date: "2026-03-16".to_string(),
            format: "markdown".to_string(),
            path: "/tmp/exports/quiet.md".to_string(),
        }];

        app.open_export_history_overlay();
        let action = match app.overlay() {
            Some(Overlay::Picker(picker)) => picker.items[0].action.clone(),
            other => panic!("expected export history picker, got {other:?}"),
        };

        app.apply_picker_action(action, 20);

        match app.overlay() {
            Some(Overlay::ExportPrompt(prompt)) => {
                assert_eq!(prompt.format, ExportFormatUi::Markdown);
                assert_eq!(prompt.path_input, "/tmp/exports/quiet.md");
            }
            other => panic!("expected export prompt, got {other:?}"),
        }
    }

    #[test]
    fn word_goal_status_label_reflects_progress() {
        let mut app = App::with_initial_date(None);
        app.config.daily_word_goal = Some(5);
        app.buffer = TextBuffer::from_text("one two three");
        app.refresh_document_stats();

        assert_eq!(app.word_goal_status_label(), "GOAL 3/5");
        assert!(app.document_stats_label().contains("W3/5"));
    }

    #[test]
    fn favorite_marker_reflects_selected_date_config() {
        let mut app =
            App::with_initial_date(Some(NaiveDate::from_ymd_opt(2026, 3, 16).expect("date")));
        app.config.favorite_dates = vec!["2026-03-16".to_string()];

        assert_eq!(app.favorite_marker(), "*");
    }

    #[test]
    fn picker_action_can_open_search_overlay_with_query() {
        let mut app = App::with_initial_date(None);
        app.apply_picker_action(PickerAction::OpenSearch("quiet".to_string()), 20);

        match app.overlay() {
            Some(Overlay::Search(search)) => assert_eq!(search.query_input, "quiet"),
            other => panic!("expected search overlay, got {other:?}"),
        }
    }

    #[test]
    fn backup_history_picker_opens_restore_prompt_with_selected_backup() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().join("vault");
        let passphrase =
            SecretString::new("correct horse battery staple".to_string().into_boxed_str());
        let metadata = vault::create_vault(&root, &passphrase, None, "Test").expect("create");
        let unlocked = vault::unlock_vault_with_device(&root, &passphrase, metadata.device_id)
            .expect("unlock");
        let selected_backup = unlocked
            .create_backup(&crate::config::BackupRetentionConfig::default())
            .expect("backup")
            .path;

        let mut app =
            App::with_initial_date(Some(NaiveDate::from_ymd_opt(2026, 3, 18).expect("date")));
        app.vault_path = root;
        app.vault = Some(unlocked);

        app.open_backup_history_overlay();
        let action = match app.overlay() {
            Some(Overlay::Picker(picker)) => picker
                .items
                .iter()
                .find(|item| matches!(
                    item.action,
                    PickerAction::OpenRestorePrompt { ref backup_path } if *backup_path == selected_backup
                ))
                .map(|item| item.action.clone())
                .expect("restore action"),
            other => panic!("expected backup history picker, got {other:?}"),
        };

        app.apply_picker_action(action, 20);

        match app.overlay() {
            Some(Overlay::RestorePrompt(prompt)) => {
                assert_eq!(
                    prompt.selected_backup().expect("selected").path,
                    selected_backup
                );
            }
            other => panic!("expected restore prompt, got {other:?}"),
        }
    }

    #[test]
    fn sync_center_is_actionable_picker() {
        let dir = tempdir().expect("tempdir");
        let local_root = dir.path().join("local");
        let remote_root = dir.path().join("remote");
        let passphrase =
            SecretString::new("correct horse battery staple".to_string().into_boxed_str());
        let metadata =
            vault::create_vault(&local_root, &passphrase, None, "Local").expect("create local");
        vault::create_vault(&remote_root, &passphrase, None, "Remote").expect("create remote");
        let local_metadata = std::fs::read(local_root.join("vault.json")).expect("local metadata");
        std::fs::write(remote_root.join("vault.json"), local_metadata).expect("align metadata");
        let unlocked =
            vault::unlock_vault_with_device(&local_root, &passphrase, metadata.device_id)
                .expect("unlock");

        let mut app =
            App::with_initial_date(Some(NaiveDate::from_ymd_opt(2026, 3, 18).expect("date")));
        app.vault_path = local_root;
        app.vault = Some(unlocked);
        app.config.sync_target_path = Some(remote_root.clone());

        app.open_sync_center_overlay();

        match app.overlay() {
            Some(Overlay::Picker(picker)) => {
                assert_eq!(picker.title, "Sync Center");
                assert!(picker.items.iter().any(|item| {
                    item.title == "Run Encrypted Sync"
                        && matches!(item.action, PickerAction::Menu(MenuAction::Sync))
                }));
                assert!(picker.items.iter().any(|item| {
                    item.title == "Recent Sync Runs"
                        && matches!(item.action, PickerAction::Menu(MenuAction::SyncHistory))
                }));
            }
            other => panic!("expected sync center picker, got {other:?}"),
        }
    }

    #[test]
    fn lock_vault_clears_editor_and_search_state() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().join("vault");
        let passphrase =
            SecretString::new("correct horse battery staple".to_string().into_boxed_str());
        let metadata = vault::create_vault(&root, &passphrase, None, "Test").expect("create");
        let unlocked = vault::unlock_vault_with_device(&root, &passphrase, metadata.device_id)
            .expect("unlock");
        let date = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");

        let mut app = App::with_initial_date(Some(date));
        app.vault_path = root;
        app.vault = Some(unlocked);
        app.buffer = TextBuffer::from_text("secret body");
        app.refresh_document_stats();
        app.closing_thought = Some("secret closing".to_string());
        app.find_query = Some("secret".to_string());
        app.pending_search_jump = Some(SearchJump {
            match_text: "secret".to_string(),
            row: 0,
            start_col: 0,
        });
        app.search_index = Some(SearchIndex::build(vec![SearchDocument {
            date,
            entry_number: "0000001".to_string(),
            body: "secret body".to_string(),
        }]));
        let mut overlay = SearchOverlay::new(Some("secret".to_string()));
        overlay.results.push(SearchResult {
            date,
            entry_number: "0000001".to_string(),
            snippet: Snippet {
                text: "secret body".to_string(),
                highlight_start: 0,
                highlight_end: 6,
            },
            row: 0,
            start_col: 0,
            end_col: 6,
            matched_text: "secret".to_string(),
        });
        app.overlay = Some(Overlay::Search(overlay));
        app.dirty = true;

        app.lock_vault();

        assert!(app.vault.is_none());
        assert!(app.search_index.is_none());
        assert_eq!(app.buffer.to_text(), "");
        assert!(app.closing_thought.is_none());
        assert!(app.find_query.is_none());
        assert!(app.pending_search_jump.is_none());
        assert!(matches!(app.overlay, Some(Overlay::UnlockPrompt { .. })));
        assert_eq!(app.lock_status_label(), "LOCKED");
    }

    #[test]
    fn pending_folder_sync_updates_overlay_and_integrity_status() {
        let dir = tempdir().expect("tempdir");
        let local_root = dir.path().join("local");
        let remote_root = dir.path().join("remote");
        let passphrase =
            SecretString::new("correct horse battery staple".to_string().into_boxed_str());

        let metadata =
            vault::create_vault(&local_root, &passphrase, None, "Local").expect("create local");
        vault::create_vault(&remote_root, &passphrase, None, "Remote").expect("create remote");
        let local_metadata = std::fs::read(local_root.join("vault.json")).expect("local metadata");
        std::fs::write(remote_root.join("vault.json"), local_metadata).expect("align metadata");

        let unlocked =
            vault::unlock_vault_with_device(&local_root, &passphrase, metadata.device_id.clone())
                .expect("unlock local");
        unlocked
            .save_revision(
                NaiveDate::from_ymd_opt(2026, 3, 16).expect("date"),
                "synced entry",
            )
            .expect("save");

        let mut app =
            App::with_initial_date(Some(NaiveDate::from_ymd_opt(2026, 3, 16).expect("date")));
        app.vault = Some(unlocked);
        app.vault_path = local_root.clone();
        app.config.sync_target_path = Some(remote_root.clone());
        app.overlay = Some(Overlay::SyncStatus(SyncStatusOverlay::pending(
            "FOLDER".to_string(),
            remote_root.display().to_string(),
            false,
        )));
        app.pending_sync_request = Some(SyncRequest::Folder {
            remote_root: remote_root.clone(),
            target_label: remote_root.display().to_string(),
        });

        app.run_pending_sync();

        match app.overlay.as_ref().expect("sync overlay") {
            Overlay::SyncStatus(SyncStatusOverlay {
                phase:
                    SyncPhase::Complete {
                        pulled,
                        pushed,
                        integrity_ok,
                        ..
                    },
                ..
            }) => {
                assert_eq!(*pulled, 0);
                assert_eq!(*pushed, 1);
                assert!(*integrity_ok);
            }
            other => panic!("unexpected overlay after sync: {other:?}"),
        }
        assert_eq!(app.integrity_status_label(), "VERIFY OK");
        let synced_files = std::fs::read_dir(remote_root.join("entries/2026/2026-03-16"))
            .expect("synced dir")
            .filter_map(Result::ok)
            .count();
        assert_eq!(synced_files, 1);
    }

    #[test]
    fn help_overlay_renders_close_hint() {
        let mut app = App::with_initial_date(None);
        app.overlay = Some(Overlay::Help);

        let rendered = render_app(&app, 100, 30);

        assert!(rendered.contains("BlueScreen Journal"));
        assert!(rendered.contains(env!("CARGO_PKG_VERSION")));
        assert!(rendered.contains("Awassee LLC and Sean Heiney"));
        assert!(rendered.contains("sean@sean.net"));
        assert!(rendered.contains("Enter/Esc/F1 closes."));
        assert!(rendered.contains("Ctrl+O/E/W/Y/T/U/L menus."));
    }

    #[test]
    fn small_terminal_warning_reports_current_size() {
        let app = App::with_initial_date(None);

        let rendered = render_app(&app, 50, 16);

        assert!(rendered.contains("TERMINAL TOO SMALL"));
        assert!(rendered.contains("CURRENT 50x16"));
        assert!(rendered.contains("Need at least"));
    }

    #[test]
    fn compact_terminal_still_renders_editor_and_menu_bar() {
        let app = App::with_initial_date(None);

        let rendered = render_app(&app, 60, 20);

        assert!(!rendered.contains("TERMINAL TOO SMALL"));
        assert!(rendered.contains("VERSION v"));
        assert!(rendered.contains("PERSONAL JOURNAL [COMPACT]"));
        assert!(rendered.contains("FILE"));
    }

    #[test]
    fn standard_header_shows_version_label() {
        let app = App::with_initial_date(None);

        let rendered = render_app(&app, 100, 30);

        assert!(rendered.contains("VERSION v"));
    }

    #[test]
    fn setting_prompt_renders_full_footer_line() {
        let mut app = App::with_initial_date(None);
        app.overlay = Some(Overlay::SettingPrompt(SettingPrompt::new(
            SettingField::VaultPath,
            &app.config,
        )));

        let rendered = render_app(&app, 100, 30);

        assert!(rendered.contains("Enter save  Esc cancel"));
    }

    #[test]
    fn conflict_overlay_renders_all_actions_and_footer() {
        let mut app = App::with_initial_date(None);
        let date = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");
        let conflict = ConflictState {
            date,
            heads: vec![
                ConflictHead {
                    revision_hash: "a".repeat(64),
                    device_id: "mac-a".to_string(),
                    seq: 1,
                    saved_at: Utc::now(),
                    body: "Body A".to_string(),
                    closing_thought: None,
                    entry_metadata: EntryMetadata::default(),
                    preview: "Preview A".to_string(),
                },
                ConflictHead {
                    revision_hash: "b".repeat(64),
                    device_id: "mac-b".to_string(),
                    seq: 2,
                    saved_at: Utc::now(),
                    body: "Body B".to_string(),
                    closing_thought: None,
                    entry_metadata: EntryMetadata::default(),
                    preview: "Preview B".to_string(),
                },
            ],
        };
        app.overlay = Some(Overlay::ConflictChoice(ConflictOverlay::new(conflict)));

        let rendered = render_app(&app, 100, 30);

        assert!(rendered.contains("CONFLICT DETECTED FOR 2026-03-16"));
        assert!(rendered.contains("Left/Right or 1-5 choose  Esc close"));
    }

    #[test]
    fn search_overlay_renders_bottom_navigation_hints() {
        let mut app = App::with_initial_date(None);
        let date = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");
        let mut overlay = SearchOverlay::new(Some("quiet".to_string()));
        overlay.results = vec![SearchResult {
            date,
            entry_number: "0000001".to_string(),
            snippet: Snippet {
                text: "quiet morning notes".to_string(),
                highlight_start: 0,
                highlight_end: 5,
            },
            row: 0,
            start_col: 0,
            end_col: 5,
            matched_text: "quiet".to_string(),
        }];
        overlay.active_field = SearchField::Results;
        app.overlay = Some(Overlay::Search(overlay));

        let rendered = render_app(&app, 100, 30);

        assert!(rendered.contains("Tab fields  Enter search/open"));
        assert!(rendered.contains("T today  W week"));
        assert!(rendered.contains("Ctrl+L clear all"));
        assert!(rendered.contains("Ctrl+R recall query"));
        assert!(rendered.contains("Selected: 1 / 1"));
    }

    #[test]
    fn index_overlay_renders_filter_and_open_hints() {
        let mut app = App::with_initial_date(None);
        let date = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");
        app.overlay = Some(Overlay::Index(IndexState::new(
            vec![
                IndexEntry {
                    date,
                    entry_number: "0000001".to_string(),
                    preview: "Quiet morning".to_string(),
                    has_conflict: false,
                    metadata: EntryMetadata::default(),
                },
                IndexEntry {
                    date: date + Duration::days(1),
                    entry_number: "0000002".to_string(),
                    preview: "Conflict review".to_string(),
                    has_conflict: true,
                    metadata: EntryMetadata {
                        tags: vec!["work".to_string()],
                        people: vec!["Alex".to_string()],
                        project: Some("Launch".to_string()),
                        mood: Some(7),
                    },
                },
            ],
            date,
            Default::default(),
        )));

        let rendered = render_app(&app, 100, 30);

        assert!(rendered.contains("Shift+F favorites  Shift+C conflicts"));
        assert!(rendered.contains("Enter open  Esc close"));
    }

    #[test]
    fn restore_overlay_renders_safety_guidance() {
        let mut app = App::with_initial_date(None);
        let backup = BackupEntry {
            path: PathBuf::from("/tmp/backup-20260316T010203Z.bsjbak.enc"),
            created_at: Utc::now(),
            size_bytes: 4096,
        };
        app.overlay = Some(Overlay::RestorePrompt(RestorePrompt::new(
            vec![backup],
            NaiveDate::from_ymd_opt(2026, 3, 16).expect("date"),
        )));

        let rendered = render_app(&app, 100, 30);

        assert!(rendered.contains("Use a new or empty folder."));
        assert!(rendered.contains("Enter next/restore  Esc cancel"));
    }

    #[test]
    fn sync_overlay_renders_close_hint() {
        let mut app = App::with_initial_date(None);
        app.overlay = Some(Overlay::SyncStatus(SyncStatusOverlay {
            backend_label: "FOLDER".to_string(),
            target_label: "/tmp/bsj-sync".to_string(),
            phase: SyncPhase::Complete {
                pulled: 1,
                pushed: 2,
                conflicts: vec![NaiveDate::from_ymd_opt(2026, 3, 16).expect("date")],
                integrity_ok: true,
                integrity_issue_count: 0,
            },
            draft_notice: true,
        }));

        let rendered = render_app(&app, 100, 30);

        assert!(rendered.contains("SYNC COMPLETE"));
        assert!(rendered.contains("Enter/Esc close"));
    }

    #[test]
    fn menu_action_roundtrip_returns_to_clean_editor_frame() {
        let mut app = App::with_initial_date(None);
        app.overlay = None;

        let viewport_height = 20;
        let viewport_width = 80;
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("terminal");
        let ghost_marker = "GHOST_MENU_TOKEN_42";

        type_editor_text(
            &mut app,
            &format!("Entry seed text {ghost_marker}"),
            viewport_height,
            viewport_width,
        );
        let _ = render_into_terminal(&mut terminal, &app);

        app.handle_event_with_viewport(
            Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty())),
            viewport_height,
            viewport_width,
        );
        app.handle_event_with_viewport(
            Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::empty())),
            viewport_height,
            viewport_width,
        );
        let menu_frame = render_into_terminal(&mut terminal, &app);
        assert!(menu_frame.contains("Typewriter Mode"));

        app.handle_event_with_viewport(
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty())),
            viewport_height,
            viewport_width,
        );
        let _ = render_into_terminal(&mut terminal, &app);
        app.handle_event_with_viewport(
            Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty())),
            viewport_height,
            viewport_width,
        );
        assert!(app.menu().is_none());
        assert!(app.overlay().is_none());

        for _ in 0..ghost_marker.chars().count() {
            send_editor_key(
                &mut app,
                KeyCode::Backspace,
                KeyModifiers::empty(),
                viewport_height,
                viewport_width,
            );
        }
        type_editor_text(&mut app, "clean frame", viewport_height, viewport_width);

        let final_frame = render_into_terminal(&mut terminal, &app);
        assert!(!app.buffer.to_text().contains(ghost_marker));
        assert!(!final_frame.contains(ghost_marker));
        assert!(!final_frame.contains("Typewriter Mode"));
    }

    #[test]
    fn shortened_line_repaint_does_not_leave_tail_ghost_text() {
        let mut app = App::with_initial_date(None);
        app.overlay = None;

        let viewport_height = 20;
        let viewport_width = 64;
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("terminal");
        let ghost_marker = "WRAP_GHOST_TRAIL_9999";

        type_editor_text(
            &mut app,
            &format!("Typing through wrap checks {ghost_marker}"),
            viewport_height,
            viewport_width,
        );
        let first_frame = render_into_terminal(&mut terminal, &app);
        assert!(first_frame.contains(ghost_marker));

        for _ in 0..ghost_marker.chars().count() {
            send_editor_key(
                &mut app,
                KeyCode::Backspace,
                KeyModifiers::empty(),
                viewport_height,
                viewport_width,
            );
        }
        type_editor_text(&mut app, "done", viewport_height, viewport_width);

        let final_frame = render_into_terminal(&mut terminal, &app);
        assert!(!app.buffer.to_text().contains(ghost_marker));
        assert!(!final_frame.contains(ghost_marker));
    }
}
