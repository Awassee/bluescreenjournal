use crate::{
    config::{
        self, AppConfig, LastSyncInfo, MacroActionConfig, MacroCommandConfig, default_vault_path,
    },
    doctor,
    help::{self, EnvironmentSettings},
    logging, platform,
    search::{SearchIndex, SearchQuery, SearchResult},
    secure_fs, sync,
    tui::{
        buffer::{MatchPos, TextBuffer},
        calendar,
    },
    vault::{self, EntryMetadata, IndexEntry, UnlockedVault},
};
use chrono::{DateTime, Duration as ChronoDuration, Local, NaiveDate};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use secrecy::SecretString;
use std::{
    collections::BTreeSet,
    env,
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
    InsertText(String),
    ShowInfo { title: String, text: String },
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
}

impl PickerOverlay {
    fn new(
        title: impl Into<String>,
        items: Vec<PickerItem>,
        empty_message: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            items,
            selected: 0,
            filter_input: String::new(),
            empty_message: empty_message.into(),
        }
    }

    fn filtered_indices(&self) -> Vec<usize> {
        let trimmed = self.filter_input.trim().to_ascii_lowercase();
        if trimmed.is_empty() {
            return (0..self.items.len()).collect();
        }

        let tokens = trimmed.split_whitespace().collect::<Vec<_>>();
        self.items
            .iter()
            .enumerate()
            .filter_map(|(idx, item)| {
                let haystack = format!(
                    "{} {} {}",
                    item.title.to_ascii_lowercase(),
                    item.detail.to_ascii_lowercase(),
                    item.keywords.to_ascii_lowercase()
                );
                tokens
                    .iter()
                    .all(|token| haystack.contains(token))
                    .then_some(idx)
            })
            .collect()
    }

    fn clamp_selection(&mut self) {
        let filtered_len = self.filtered_indices().len();
        self.selected = self.selected.min(filtered_len.saturating_sub(1));
        if filtered_len == 0 {
            self.selected = 0;
        }
    }

    pub fn window(&self, max_rows: usize) -> (Vec<usize>, usize, usize) {
        let filtered = self.filtered_indices();
        if filtered.is_empty() || max_rows == 0 {
            return (filtered, 0, 0);
        }
        let max_rows = max_rows.max(1);
        let mut start = self.selected.saturating_sub(max_rows / 2);
        let max_start = filtered.len().saturating_sub(max_rows);
        if start > max_start {
            start = max_start;
        }
        let end = (start + max_rows).min(filtered.len());
        (filtered, start, end)
    }

    fn selected_item(&self) -> Option<&PickerItem> {
        let filtered = self.filtered_indices();
        filtered
            .get(self.selected)
            .and_then(|index| self.items.get(*index))
    }

    fn move_selection(&mut self, delta: isize) {
        let filtered_len = self.filtered_indices().len();
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
    }

    fn pop_filter_char(&mut self) {
        self.filter_input.pop();
        self.clamp_selection();
    }

    fn wipe(&mut self) {
        self.title.zeroize();
        self.filter_input.zeroize();
        self.empty_message.zeroize();
        for item in &mut self.items {
            item.title.zeroize();
            item.detail.zeroize();
            item.keywords.zeroize();
            match &mut item.action {
                PickerAction::Menu(_) | PickerAction::OpenDate(_) => {}
                PickerAction::OpenSearch(query) | PickerAction::InsertText(query) => {
                    query.zeroize();
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
}

impl IndexState {
    fn new(items: Vec<IndexEntry>, selected_date: NaiveDate) -> Self {
        let mut state = Self {
            all_items: items.clone(),
            items,
            selected: 0,
            filter_input: String::new(),
            sort_oldest_first: false,
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
            .filter(|entry| index_matches_filter(entry, &needle))
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
        }
        self.all_items.clear();
        for item in &mut self.items {
            item.entry_number.zeroize();
            item.preview.zeroize();
        }
        self.items.clear();
        self.selected = 0;
        self.filter_input.zeroize();
        self.sort_oldest_first = false;
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
    BackupHistory,
    BackupCleanupPreview,
    BackupPruneNow,
    Backup,
    RestoreBackup,
    Dashboard,
    SyncCenter,
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
    GlobalSearch,
    SearchHistory,
    FindNext,
    FindPrevious,
    PreviousParagraph,
    NextParagraph,
    RebuildSearchIndex,
    Dates,
    RecentEntries,
    FavoriteEntries,
    PreviousEntry,
    NextEntry,
    Today,
    Index,
    Sync,
    Verify,
    SettingsSummary,
    ReviewPrompts,
    SyncHistory,
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
    last_autosave_check: Instant,
    session_started_at: Instant,
    recent_dates: Vec<NaiveDate>,
    search_history: Vec<String>,
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

        let mut app = Self {
            config,
            buffer: TextBuffer::new(),
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
            last_autosave_check: Instant::now(),
            session_started_at: Instant::now(),
            recent_dates: Vec::new(),
            search_history: Vec::new(),
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

    pub fn footer_dirty_label(&self) -> &'static str {
        if self.dirty { "MOD" } else { "VIEW" }
    }

    pub fn cursor_status_label(&self) -> String {
        let (row, col) = self.buffer.cursor();
        format!("LN {:>3} COL {:>3}", row + 1, col + 1)
    }

    pub fn footer_context_label(&self) -> String {
        if let Some(menu) = &self.menu {
            let items = self.menu_items(menu.selected_menu);
            if items.is_empty() {
                return menu.selected_menu.title().to_string();
            }
            return format!(
                "{} {}/{}",
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
                format!("RESULT {}/{}", search.selected + 1, search.results.len())
            }
            Some(Overlay::Index(index)) if !index.items.is_empty() => {
                format!("ENTRY {}/{}", index.selected + 1, index.items.len())
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
        let lines = self.buffer.line_count();
        let text = self.buffer.to_text();
        let words = self.document_word_count();
        let chars = text.chars().count();
        if let Some(goal) = self.config.daily_word_goal {
            format!("L{lines} W{words}/{goal} C{chars}")
        } else {
            format!("L{lines} W{words} C{chars}")
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

    pub fn empty_state_lines(&self) -> [&'static str; 6] {
        [
            "START TYPING TO WRITE TODAY'S ENTRY",
            "Esc opens menus for FILE / EDIT / SEARCH / GO / TOOLS / SETUP / HELP",
            "F2 saves an encrypted revision. Autosave writes an encrypted draft.",
            "F3 calendar  F5 vault search  F7 index  F8 sync/sync center",
            "Use favorites, recents, prompts, and command palette from menus",
            "F1 shows help. Ctrl+K opens the command palette.",
        ]
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
            ],
            MenuId::Go => vec![
                MenuItem {
                    label: "Open Calendar".to_string(),
                    detail: "F3".to_string(),
                    action: MenuAction::Dates,
                    enabled: self.vault.is_some(),
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
                    label: "Verify Integrity".to_string(),
                    detail: self.integrity_status_label(),
                    action: MenuAction::Verify,
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
                self.setting_menu_item(SettingField::BackupDaily),
                self.setting_menu_item(SettingField::BackupWeekly),
                self.setting_menu_item(SettingField::BackupMonthly),
            ],
            MenuId::Help => vec![
                MenuItem {
                    label: "Key and Menu Guide".to_string(),
                    detail: "F1".to_string(),
                    action: MenuAction::Help,
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

    pub fn handle_event(&mut self, event: Event, viewport_height: usize) {
        self.last_viewport_height = viewport_height.max(1);
        match event {
            Event::Key(key) => self.handle_key(key, viewport_height),
            Event::Resize(width, height) => {
                log::debug!("terminal resized to {}x{}", width, height);
                self.ensure_cursor_visible(viewport_height);
            }
            _ => {}
        }
    }

    fn handle_key(&mut self, key: KeyEvent, viewport_height: usize) {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
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
            KeyCode::Char(ch) if Self::is_text_input_key(&key) => {
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
                KeyCode::Tab => {
                    search.cycle_field();
                    search.error = None;
                }
                KeyCode::Backspace => {
                    if let Some(input) = search.active_input_mut() {
                        input.pop();
                        search.clear_results();
                        search.error = None;
                        self.maybe_live_run_global_search(search);
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
                        self.maybe_live_run_global_search(search);
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
                    self.dirty = replaced > 0;
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
                KeyCode::Char('s') | KeyCode::Char('S') => {
                    let selected_date = index.selected_date().unwrap_or(self.selected_date);
                    index.toggle_sort(selected_date);
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
            return;
        };

        match vault.load_date_state(self.selected_date) {
            Ok(state) => {
                self.buffer = TextBuffer::from_text(state.revision_text.as_deref().unwrap_or(""));
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
        self.overlay = Some(Overlay::Search(SearchOverlay::new(query)));
    }

    fn open_export_prompt(&mut self) {
        self.menu = None;
        self.overlay = Some(Overlay::ExportPrompt(ExportPrompt::new(self.selected_date)));
    }

    fn open_index_overlay(&mut self) {
        self.menu = None;
        let items = self.load_index_entries();
        self.overlay = Some(Overlay::Index(IndexState::new(items, self.selected_date)));
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
        self.buffer.to_text().split_whitespace().count()
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

    fn open_first_run_guide_overlay(&mut self) {
        let lines = vec![
            "WELCOME TO BLUESCREEN JOURNAL".to_string(),
            String::new(),
            "First 2 minutes:".to_string(),
            "1. Type immediately into today's entry.".to_string(),
            "2. Press F2 for your first encrypted saved revision.".to_string(),
            "3. Use EDIT -> Entry Metadata for tags, people, project, and mood.".to_string(),
            "4. Use GO -> Open Calendar or Index Timeline to move through time.".to_string(),
            "5. Use TOOLS -> Sync Center before trusting a new sync target.".to_string(),
            "6. Use FILE -> Backup Snapshot before major travel or changes.".to_string(),
            String::new(),
            "The app autosaves encrypted drafts, but manual Save creates history.".to_string(),
            "F1 opens the cheatsheet. Esc opens the full DOS-style menu bar.".to_string(),
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

        let mut lines = vec!["Sync Center".to_string(), String::new()];
        match self.resolve_sync_request() {
            Ok(request) => {
                lines.push(format!("Backend      : {}", request.backend_label()));
                lines.push(format!("Target       : {}", request.target_label()));
                match self.sync_preview_report(&request) {
                    Ok(preview) => {
                        lines.push(format!("Local revs   : {}", preview.local_revisions));
                        lines.push(format!("Remote revs  : {}", preview.remote_revisions));
                        lines.push(format!("Upload queue : {}", preview.local_only_revisions));
                        lines.push(format!("Download q   : {}", preview.remote_only_revisions));
                        lines.push(format!("Shared revs  : {}", preview.shared_revisions));
                    }
                    Err(error) => lines.push(format!("Preview      : {error}")),
                }
            }
            Err(error) => lines.push(format!("Backend      : {error}")),
        }
        let conflicts = vault
            .list_conflicted_dates()
            .map(|dates| dates.len())
            .unwrap_or(0);
        lines.push(format!("Conflicts    : {conflicts}"));
        lines.push(format!(
            "Dirty draft  : {}",
            if self.dirty { "YES" } else { "NO" }
        ));
        lines.push(format!(
            "Last sync    : {}",
            self.config
                .last_sync
                .as_ref()
                .map(|sync| format!(
                    "{} {} +{} / -{}",
                    sync.timestamp, sync.backend, sync.pushed, sync.pulled
                ))
                .unwrap_or_else(|| "never".to_string())
        ));
        lines.push(String::new());
        lines.push("Use F8 or TOOLS -> Sync Vault to run the encrypted sync pipeline.".to_string());
        self.open_info_overlay("Sync Center", lines.join("\n"));
    }

    fn open_review_overlay(&mut self) {
        let Some(vault) = &self.vault else {
            self.flash_status("LOCKED.");
            return;
        };
        match vault.review_summary(Local::now().date_naive()) {
            Ok(review) => {
                let mut lines = vec![
                    format!("Total entries : {}", review.total_entries),
                    format!("Streak        : {} day(s)", review.streak_days),
                    format!("This week     : {}", review.entries_this_week),
                    format!("This month    : {}", review.entries_this_month),
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
                self.open_info_overlay("Review Mode", lines.join("\n"));
            }
            Err(error) => self.open_info_overlay("Review Mode", format!("Review failed: {error}")),
        }
    }

    fn open_update_check_overlay(&mut self) {
        match platform::check_for_updates(env!("CARGO_PKG_VERSION")) {
            Ok(info) => {
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
                    "Install".to_string(),
                    "curl -fsSL https://raw.githubusercontent.com/Awassee/bluescreenjournal/main/install.sh | bash".to_string(),
                ];
                if !info.asset_names.is_empty() {
                    lines.push(String::new());
                    lines.push("Assets".to_string());
                    for asset in info.asset_names.iter().take(6) {
                        lines.push(format!("  {asset}"));
                    }
                }
                self.open_info_overlay("Updates", lines.join("\n"));
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

        let output = match vault.list_backups() {
            Ok(backups) if backups.is_empty() => "No encrypted backups yet.".to_string(),
            Ok(backups) => {
                let mut lines = vec![
                    "Encrypted backups".to_string(),
                    String::new(),
                    "DATE/TIME           SIZE       FILE".to_string(),
                    "-----------------------------------------------".to_string(),
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
            Err(error) => format!("Failed to load backups: {error}"),
        };

        self.open_info_overlay("Backup History", output);
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
        let integrity = self.integrity_status_label();
        let dirty = if self.dirty { "modified" } else { "clean" };
        let save_state = self.save_status_label();

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
            format!("Sync runs   : {}", self.config.sync_history.len()),
            format!("Logs        : {}", logging::log_file_path().display()),
            String::new(),
            "Use FILE for export/backups, GO for dates/index, TOOLS for sync/verify/doctor."
                .to_string(),
        ]
        .join("\n");

        self.open_info_overlay("Dashboard", output);
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
                self.flash_status("DATE OPENED.");
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

        self.open_date(dates[next_idx as usize]);
        self.flash_status("DATE OPENED.");
    }

    fn perform_menu_action(&mut self, action: MenuAction, viewport_height: usize) {
        match action {
            MenuAction::CommandPalette => self.open_command_palette(),
            MenuAction::Save => self.save_current_date(),
            MenuAction::Export => self.open_export_prompt(),
            MenuAction::BackupHistory => self.open_backup_history_overlay(),
            MenuAction::BackupCleanupPreview => self.open_backup_cleanup_preview_overlay(),
            MenuAction::BackupPruneNow => self.prune_backups_now(),
            MenuAction::Backup => self.create_backup_now(),
            MenuAction::RestoreBackup => self.open_restore_prompt(),
            MenuAction::Dashboard => self.open_dashboard_overlay(),
            MenuAction::SyncCenter => self.open_sync_center_overlay(),
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
            MenuAction::GlobalSearch => self.open_search_overlay(),
            MenuAction::SearchHistory => self.open_search_history_overlay(),
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
            MenuAction::Dates => self.open_date_picker(),
            MenuAction::RecentEntries => self.open_recent_entries_overlay(),
            MenuAction::FavoriteEntries => self.open_favorite_entries_overlay(),
            MenuAction::PreviousEntry => self.open_adjacent_saved_entry(-1),
            MenuAction::NextEntry => self.open_adjacent_saved_entry(1),
            MenuAction::Today => {
                self.open_date(Local::now().date_naive());
                self.flash_status("JUMPED TO TODAY.");
            }
            MenuAction::Index => self.open_index_overlay(),
            MenuAction::Sync => self.begin_sync(),
            MenuAction::Verify => self.verify_integrity_now(),
            MenuAction::SettingsSummary => self.open_settings_summary_overlay(),
            MenuAction::ReviewPrompts => self.open_review_prompts_overlay(),
            MenuAction::SyncHistory => self.open_sync_history_overlay(),
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
        self.vault = None;
        self.integrity_status = None;
        self.overlay = Some(Overlay::UnlockPrompt {
            input: String::new(),
            error: Some("Vault locked. Enter passphrase.".to_string()),
        });
        self.flash_status("LOCKED.");
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
                    self.run_global_search(&mut overlay);
                }
                self.menu = None;
                self.overlay = Some(Overlay::Search(overlay));
            }
            PickerAction::InsertText(text) => {
                self.buffer.insert_text(&text);
                self.dirty = true;
                self.ensure_cursor_visible(viewport_height);
                self.flash_status("TEXT INSERTED.");
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
                let file_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("export");
                self.flash_status(&format!("EXPORTED {file_name}."));
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
                self.last_save_kind = Some(SaveKind::Saved);
                self.last_save_time = Some(Local::now());
                self.load_selected_date();
                self.last_save_kind = Some(SaveKind::Saved);
                self.last_save_time = Some(Local::now());
                self.flash_status("SAVED.");
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

    fn maybe_live_run_global_search(&mut self, search: &mut SearchOverlay) {
        if search.query_input.trim().is_empty() {
            search.clear_results();
            search.error = None;
            search.active_field = SearchField::Query;
            return;
        }
        self.run_global_search(search);
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
                self.dirty = true;
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
                    self.dirty = true;
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

fn render_rank_lines(items: &[(String, usize)]) -> Vec<String> {
    if items.is_empty() {
        return vec!["  No data yet.".to_string()];
    }
    items
        .iter()
        .map(|(label, count)| format!("  {:<18} {}", label, count))
        .collect()
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
    date.to_ascii_lowercase().contains(needle)
        || entry.entry_number.to_ascii_lowercase().contains(needle)
        || preview.contains(needle)
        || (entry.has_conflict && "conflict".contains(needle))
}

#[cfg(test)]
mod tests {
    use super::{
        App, DatePicker, ExportPrompt, IndexState, MenuAction, MenuId, Overlay, PickerAction,
        PickerItem, PickerOverlay, SearchField, SearchJump, SearchOverlay, SyncPhase, SyncRequest,
        SyncStatusOverlay, default_export_path, format_reveal_codes, macro_key_matches,
        parse_optional_overlay_date, resolve_recovery_text,
    };
    use crate::{
        search::{SearchDocument, SearchIndex, SearchResult, Snippet},
        tui::buffer::{MatchPos, TextBuffer},
        vault::{self, EntryMetadata, IndexEntry},
    };
    use chrono::{Duration, NaiveDate};
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    use secrecy::SecretString;
    use tempfile::tempdir;

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
            },
            IndexEntry {
                date: selected + Duration::days(1),
                entry_number: "0000002".to_string(),
                preview: "Conflict review".to_string(),
                has_conflict: true,
            },
        ];
        let mut index = IndexState::new(items, selected);
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
            },
            IndexEntry {
                date: selected,
                entry_number: "0000001".to_string(),
                preview: "Earlier".to_string(),
                has_conflict: false,
            },
        ];
        let mut index = IndexState::new(items, selected + Duration::days(1));
        index.toggle_sort(selected + Duration::days(1));

        assert_eq!(index.items.first().map(|entry| entry.date), Some(selected));
        assert!(index.sort_oldest_first);
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
        assert_eq!(overlay.active_field, SearchField::Results);
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
    fn word_goal_status_label_reflects_progress() {
        let mut app = App::with_initial_date(None);
        app.config.daily_word_goal = Some(5);
        app.buffer = TextBuffer::from_text("one two three");

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
}
