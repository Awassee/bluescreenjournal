mod ai;
mod config;
mod doctor;
mod help;
mod logging;
mod platform;
mod search;
mod secure_fs;
mod spellcheck;
mod sync;
mod sysop;
mod tui;
mod vault;

use chrono::{Datelike, Local, NaiveDate, Weekday};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use secrecy::SecretString;
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashSet},
    env,
    hash::{Hash, Hasher},
    io,
    path::{Path, PathBuf},
};

#[derive(Debug, Parser)]
#[command(
    name = "bsj",
    version = env!("CARGO_PKG_VERSION"),
    about = "BlueScreen Journal",
    long_about = "BlueScreen Journal is an encrypted, local-first macOS terminal journal with a nostalgic blue-screen full-screen editor, a menu-driven TUI, append-only revisions, encrypted drafts, encrypted backups, and encrypted sync targets.",
    after_help = "Examples:\n  bsj\n  bsj open 2026-03-16\n  bsj search \"quiet morning\" --from 2026-03-01 --to 2026-03-31\n  bsj search \"focus\" --whole-word --case-sensitive --limit 20\n  bsj search \"mood:7\" --json --context 40\n  bsj search \"ship\" --match-mode any --sort relevance --hits-per-entry 5\n  bsj search \"focus\" --range last7 --summary\n  bsj search --preset \"Weekly Review\"\n  bsj search --list-presets\n  bsj search \"mood:7\" --save-preset \"Mood Seven\"\n  bsj spellcheck --date 2026-03-16\n  bsj spellcheck --range last7 --count-only\n  bsj timeline --query ship --tag work --person Riley --project Phoenix --metadata\n  bsj timeline --range last30 --group-by week\n  bsj timeline --save-preset \"Recent Work\" --query ship --tag work\n  bsj timeline --list-presets\n  bsj review --range last30 --goal 750\n  bsj ai summary --date 2026-03-16\n  bsj ai summary --range last7 --remote\n  bsj ai coach --date 2026-03-16 --questions 5\n  bsj export 2026-03-16 --format markdown --output ~/Desktop/entry.md\n  bsj sync --backend folder --remote ~/Documents/BlueScreenJournal-Sync\n  bsj backup\n  bsj backup list\n  bsj backup prune --apply\n  bsj settings init\n  bsj settings get vault_path\n  bsj settings set sync_target_path ~/Documents/BlueScreenJournal-Sync\n  bsj doctor --unlock\n  bsj sysop dashboard\n  bsj sysop runbook\n  bsj sysop sync-preview --backend folder --remote ~/Documents/BlueScreenJournal-Sync\n  bsj completions zsh\n\nGuides:\n  bsj guide docs\n  bsj guide quickstart\n  bsj guide troubleshooting\n  bsj guide sync\n  bsj guide backup\n  bsj guide macros\n  bsj guide terminal\n  bsj guide privacy\n  bsj guide product\n  bsj guide datasheet\n  bsj guide faq\n  bsj guide support\n  bsj guide setup\n  bsj guide settings\n  bsj guide distribution\n\nPackaging:\n  ./install.sh --prebuilt\n  ./scripts/package-release.sh\n\nDebug logging:\n  Use --debug to enable verbose file logging at ~/Library/Logs/bsj/bsj.log"
)]
struct Cli {
    #[arg(
        long,
        global = true,
        help = "Enable verbose debug logging to ~/Library/Logs/bsj/bsj.log"
    )]
    debug: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Open a specific journal date in the TUI
    Open { date: String },
    /// Export a date as plain text or markdown
    Export {
        date: String,
        #[arg(long, value_enum, default_value_t = ExportFormatArg::Text)]
        format: ExportFormatArg,
        #[arg(long, help = "Write the export to a file instead of stdout")]
        output: Option<String>,
    },
    /// Search encrypted entries without writing a plaintext index
    Search {
        query: Option<String>,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
        #[arg(
            long,
            value_enum,
            help = "Quick date range preset (overrides preset range)"
        )]
        range: Option<DateRangeArg>,
        #[arg(long, help = "Print matches in JSON format")]
        json: bool,
        #[arg(
            long,
            default_value_t = 0,
            help = "Maximum number of matches to print (0 = all)"
        )]
        limit: usize,
        #[arg(long, help = "Print only the number of matches")]
        count_only: bool,
        #[arg(long, help = "Use case-sensitive matching")]
        case_sensitive: bool,
        #[arg(long, help = "Match whole words only")]
        whole_word: bool,
        #[arg(
            long,
            default_value_t = 24,
            help = "Snippet context characters around each match"
        )]
        context: usize,
        #[arg(
            long,
            value_enum,
            default_value_t = SearchMatchArg::All,
            help = "Token matching mode"
        )]
        match_mode: SearchMatchArg,
        #[arg(
            long,
            value_enum,
            default_value_t = SearchSortArg::Newest,
            help = "Result ordering"
        )]
        sort: SearchSortArg,
        #[arg(long, default_value_t = 1, help = "Maximum matches to emit per entry")]
        hits_per_entry: usize,
        #[arg(
            long,
            help = "Print aggregate search stats instead of individual matches"
        )]
        summary: bool,
        #[arg(long, help = "Run a saved preset by name")]
        preset: Option<String>,
        #[arg(long, help = "List saved search presets and exit")]
        list_presets: bool,
        #[arg(long, help = "Save this query/range as a preset name before searching")]
        save_preset: Option<String>,
        #[arg(long, help = "Delete a saved preset by name and exit")]
        delete_preset: Option<String>,
    },
    /// Spellcheck saved entries without writing plaintext indexes
    Spellcheck {
        #[arg(long, help = "Spellcheck one specific date (YYYY-MM-DD)")]
        date: Option<String>,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
        #[arg(long, value_enum, help = "Quick date range preset")]
        range: Option<DateRangeArg>,
        #[arg(long, help = "Print results as JSON")]
        json: bool,
        #[arg(
            long,
            default_value_t = 0,
            help = "Maximum misspellings to print (0 = all)"
        )]
        limit: usize,
        #[arg(long, help = "Print only the misspelling count")]
        count_only: bool,
    },
    /// Show writing analytics (streak, recency, and top metadata)
    Review {
        #[arg(long, default_value_t = 5, help = "Top N tags/people/projects to show")]
        top: usize,
        #[arg(
            long,
            default_value_t = 5,
            help = "Maximum number of On This Day entries to display"
        )]
        on_this_day: usize,
        #[arg(long, help = "Limit review metrics to entries on or after this date")]
        from: Option<String>,
        #[arg(long, help = "Limit review metrics to entries on or before this date")]
        to: Option<String>,
        #[arg(long, value_enum, help = "Quick date range preset")]
        range: Option<DateRangeArg>,
        #[arg(long, help = "Print the review report in JSON")]
        json: bool,
        #[arg(
            long,
            default_value_t = 1,
            help = "Minimum frequency to include in top tags/people/projects"
        )]
        min_count: usize,
        #[arg(long, help = "Daily word goal override for this review run")]
        goal: Option<usize>,
    },
    /// Print a timeline of saved entries with filters
    Timeline {
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
        #[arg(long, value_enum, help = "Quick date range preset")]
        range: Option<DateRangeArg>,
        #[arg(long, help = "Case-insensitive text filter over preview and metadata")]
        query: Option<String>,
        #[arg(long, help = "Filter to entries tagged with this value")]
        tag: Option<String>,
        #[arg(long, help = "Filter to entries that mention this person")]
        person: Option<String>,
        #[arg(long, help = "Filter to entries in this project")]
        project: Option<String>,
        #[arg(
            long,
            value_parser = clap::value_parser!(u8).range(0..=9),
            help = "Filter to entries with this mood value (0-9)"
        )]
        mood: Option<u8>,
        #[arg(long, help = "Only include entries with at least one tag")]
        has_tags: bool,
        #[arg(long, help = "Only include entries with at least one person")]
        has_people: bool,
        #[arg(long, help = "Only include entries with a project set")]
        has_project: bool,
        #[arg(long, value_enum, value_delimiter = ',', help = "Filter by weekday")]
        weekday: Vec<WeekdayArg>,
        #[arg(long, default_value_t = 30)]
        limit: usize,
        #[arg(long, help = "Show oldest entries first")]
        asc: bool,
        #[arg(long, help = "Only include favorite dates from settings")]
        favorites: bool,
        #[arg(long, help = "Only include dates currently in conflict")]
        conflicts: bool,
        #[arg(long, help = "Include metadata stamp (tags/people/project/mood)")]
        metadata: bool,
        #[arg(
            long,
            value_enum,
            help = "Aggregate timeline rows by calendar group instead of individual entries"
        )]
        group_by: Option<TimelineGroupByArg>,
        #[arg(
            long,
            value_enum,
            default_value_t = TimelineFormatArg::Text,
            help = "Output format"
        )]
        format: TimelineFormatArg,
        #[arg(long, help = "Print aggregate timeline summary instead of rows")]
        summary: bool,
        #[arg(long, help = "Run a saved timeline preset by name")]
        preset: Option<String>,
        #[arg(long, help = "List saved timeline presets and exit")]
        list_presets: bool,
        #[arg(long, help = "Save this timeline filter as a preset")]
        save_preset: Option<String>,
        #[arg(long, help = "Delete a saved timeline preset by name and exit")]
        delete_preset: Option<String>,
    },
    /// Launch directly on the next date without a saved revision
    Next {
        #[arg(long, help = "Start scanning from this date (YYYY-MM-DD)")]
        from: Option<String>,
    },
    /// Prompt library for journaling warm starts
    Prompts {
        #[command(subcommand)]
        command: PromptCommand,
    },
    /// Optional AI coach prompts and summaries (off by default)
    Ai {
        #[command(subcommand)]
        command: AiCommand,
    },
    /// Sync encrypted revisions with a folder, S3, or WebDAV target
    Sync {
        #[arg(long, value_enum)]
        backend: Option<SyncBackendArg>,
        #[arg(long)]
        remote: Option<String>,
    },
    /// Create, list, or prune encrypted backups under <vault>/backups
    Backup {
        #[command(subcommand)]
        command: Option<BackupCommand>,
    },
    /// Restore an encrypted backup snapshot into a target directory
    Restore {
        backup: String,
        #[arg(long)]
        into: String,
    },
    /// Verify revision hashchains
    Verify,
    /// Show effective settings or manage config values
    Settings {
        #[arg(long, help = "Print the current settings report in JSON")]
        json: bool,
        #[command(subcommand)]
        command: Option<SettingsCommand>,
    },
    /// Print the docs hub, quickstart, troubleshooting, sync, backup, macros, terminal, privacy, product, datasheet, FAQ, support, settings, or distribution guide
    Guide {
        #[arg(value_enum, default_value_t = GuideTopicArg::Setup)]
        topic: GuideTopicArg,
    },
    /// Diagnose install, config, sync, and vault health
    Doctor {
        #[arg(long, help = "Unlock the vault and run encrypted integrity checks")]
        unlock: bool,
        #[arg(long, help = "Print the doctor report in JSON")]
        json: bool,
    },
    /// Operator-grade SYSOP diagnostics, audits, and previews
    Sysop {
        #[command(subcommand)]
        command: SysopCommand,
    },
    /// Generate shell completion scripts to stdout
    Completions {
        #[arg(value_enum)]
        shell: CompletionShellArg,
    },
}

#[derive(Debug, Subcommand)]
enum BackupCommand {
    /// List encrypted backups under <vault>/backups
    List,
    /// Show or apply retention-based backup pruning
    Prune {
        #[arg(long, help = "Delete the backups instead of showing a dry run")]
        apply: bool,
    },
}

#[derive(Debug, Subcommand)]
enum SettingsCommand {
    /// Create a default config file if one does not exist
    Init {
        #[arg(long, help = "Overwrite an existing or broken config file")]
        force: bool,
    },
    /// Read one setting value
    Get { key: String },
    /// Update one editable setting value
    Set { key: String, value: String },
}

#[derive(Debug, Subcommand)]
enum PromptCommand {
    /// List built-in prompt templates
    List {
        #[arg(long, value_enum)]
        category: Option<PromptCategoryArg>,
        #[arg(long, help = "Print prompts as JSON")]
        json: bool,
    },
    /// Pick one deterministic prompt for a date
    Pick {
        #[arg(long, value_enum)]
        category: Option<PromptCategoryArg>,
        #[arg(long, help = "Use a specific date (YYYY-MM-DD) instead of today")]
        date: Option<String>,
        #[arg(long, help = "Print prompt output as JSON")]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum AiCommand {
    /// Summarize one date or a date range using optional AI assistance
    Summary {
        #[arg(long, help = "Summarize one specific date (YYYY-MM-DD)")]
        date: Option<String>,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
        #[arg(long, value_enum, help = "Quick date range preset")]
        range: Option<DateRangeArg>,
        #[arg(
            long,
            default_value_t = 5,
            help = "Maximum summary lines for text output"
        )]
        max_points: usize,
        #[arg(long, help = "Print summary output as JSON")]
        json: bool,
        #[arg(
            long,
            help = "Use remote AI if BSJ_AI_ENABLE_REMOTE=1 and API key are configured"
        )]
        remote: bool,
    },
    /// Generate reflective AI-style coaching questions for a date
    Coach {
        #[arg(
            long,
            help = "Generate prompts for one date (YYYY-MM-DD, default=today)"
        )]
        date: Option<String>,
        #[arg(
            long,
            default_value_t = 5,
            help = "Number of coaching questions to generate"
        )]
        questions: usize,
        #[arg(long, help = "Print prompts as JSON")]
        json: bool,
        #[arg(
            long,
            help = "Use remote AI if BSJ_AI_ENABLE_REMOTE=1 and API key are configured"
        )]
        remote: bool,
    },
}

#[derive(Debug, Subcommand)]
enum SysopCommand {
    /// Unified operator dashboard summary
    Dashboard {
        #[arg(long, help = "Print dashboard as JSON")]
        json: bool,
    },
    /// Prioritized runbook generated from current vault state
    Runbook,
    /// Inspect environment variable readiness
    Env {
        #[arg(long, help = "Print environment report as JSON")]
        json: bool,
    },
    /// Show resolved config/log/vault paths and availability
    Paths {
        #[arg(long, help = "Print path report as JSON")]
        json: bool,
    },
    /// Audit file permissions and symlink risk under vault root
    Permissions {
        #[arg(long, help = "Print permission audit as JSON")]
        json: bool,
        #[arg(
            long,
            default_value_t = 20,
            help = "Maximum issues to print in text mode"
        )]
        limit: usize,
    },
    /// Validate vault layout and encrypted file naming hygiene
    VaultLayout {
        #[arg(long, help = "Print layout report as JSON")]
        json: bool,
    },
    /// Detect files in vault root that are outside supported structure
    Orphans {
        #[arg(long, help = "Print orphan file report as JSON")]
        json: bool,
        #[arg(
            long,
            default_value_t = 50,
            help = "Maximum files to print in text mode"
        )]
        limit: usize,
    },
    /// Show dates with highest revision volume
    Revisions {
        #[arg(long, default_value_t = 10, help = "Top N dates to print")]
        top: usize,
        #[arg(long, help = "Print revision stats as JSON")]
        json: bool,
    },
    /// List encrypted draft files older than a threshold
    Drafts {
        #[arg(long, default_value_t = 7, help = "Minimum draft age in days")]
        older_than_days: i64,
        #[arg(long, help = "Print stale draft report as JSON")]
        json: bool,
        #[arg(
            long,
            default_value_t = 50,
            help = "Maximum drafts to print in text mode"
        )]
        limit: usize,
    },
    /// List conflicted entry dates
    Conflicts {
        #[arg(long, help = "Print conflict report as JSON")]
        json: bool,
    },
    /// Inspect encrypted search cache status
    Cache {
        #[arg(long, help = "Print cache report as JSON")]
        json: bool,
    },
    /// Show backup inventory and retention preview
    Backups {
        #[arg(long, help = "Print backup report as JSON")]
        json: bool,
    },
    /// Run hashchain integrity verification with issue details
    Integrity {
        #[arg(long, help = "Print integrity report as JSON")]
        json: bool,
    },
    /// Render recent activity series based on revision counts
    Activity {
        #[arg(long, default_value_t = 30, help = "Number of days in activity series")]
        days: usize,
        #[arg(long, help = "Print activity series as JSON")]
        json: bool,
    },
    /// Non-destructive local/remote sync delta preview
    SyncPreview {
        #[arg(long, value_enum)]
        backend: Option<SyncBackendArg>,
        #[arg(long)]
        remote: Option<String>,
        #[arg(long, help = "Print sync preview as JSON")]
        json: bool,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum SyncBackendArg {
    Folder,
    S3,
    Webdav,
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq, Hash)]
enum PromptCategoryArg {
    Reflection,
    Gratitude,
    Growth,
    Focus,
    Relationships,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum GuideTopicArg {
    Docs,
    Quickstart,
    Troubleshooting,
    Sync,
    Backup,
    Macros,
    Terminal,
    Privacy,
    Setup,
    Product,
    Datasheet,
    Faq,
    Support,
    Settings,
    Distribution,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ExportFormatArg {
    Text,
    Markdown,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum DateRangeArg {
    Today,
    Yesterday,
    Last7,
    Last30,
    ThisMonth,
    LastMonth,
    Ytd,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum SearchMatchArg {
    All,
    Any,
    Phrase,
}

impl SearchMatchArg {
    fn to_search_mode(self) -> search::SearchMatchMode {
        match self {
            Self::All => search::SearchMatchMode::All,
            Self::Any => search::SearchMatchMode::Any,
            Self::Phrase => search::SearchMatchMode::Phrase,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
enum SearchSortArg {
    Newest,
    Oldest,
    Relevance,
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
enum TimelineFormatArg {
    Text,
    Json,
    Csv,
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
enum TimelineGroupByArg {
    Day,
    Week,
    Month,
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq, Hash)]
enum WeekdayArg {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

impl WeekdayArg {
    fn to_chrono(self) -> Weekday {
        match self {
            Self::Mon => Weekday::Mon,
            Self::Tue => Weekday::Tue,
            Self::Wed => Weekday::Wed,
            Self::Thu => Weekday::Thu,
            Self::Fri => Weekday::Fri,
            Self::Sat => Weekday::Sat,
            Self::Sun => Weekday::Sun,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CompletionShellArg {
    Bash,
    Fish,
    Zsh,
}

struct LoadedConfigState {
    path: PathBuf,
    config: config::AppConfig,
    exists: bool,
    error: Option<String>,
}

fn main() {
    let cli = Cli::parse();
    if let Err(error) = logging::init(cli.debug) {
        eprintln!("failed to initialize logging: {error}");
        std::process::exit(1);
    }
    log::info!("bsj starting");

    match cli.command {
        Some(Command::Open { date }) => {
            let initial_date = match parse_date_arg("date", &date) {
                Ok(date) => Some(date),
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(2);
                }
            };

            log::info!("launching TUI for requested date");
            if let Err(error) = tui::run(initial_date) {
                log::error!("failed to launch TUI: {error}");
                eprintln!("failed to launch TUI: {error}");
                std::process::exit(1);
            }
        }
        Some(Command::Export {
            date,
            format,
            output,
        }) => {
            let date = match parse_date_arg("date", &date) {
                Ok(date) => date,
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(2);
                }
            };

            if let Err(error) = run_cli_export(date, format, output.as_deref()) {
                log::error!("export failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some(Command::Search {
            query,
            from,
            to,
            range,
            json,
            limit,
            count_only,
            case_sensitive,
            whole_word,
            context,
            match_mode,
            sort,
            hits_per_entry,
            summary,
            preset,
            list_presets,
            save_preset,
            delete_preset,
        }) => {
            let from = match parse_optional_date_arg("from", from.as_deref()) {
                Ok(date) => date,
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(2);
                }
            };
            let to = match parse_optional_date_arg("to", to.as_deref()) {
                Ok(date) => date,
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(2);
                }
            };

            if let Err(error) = run_cli_search(SearchCliArgs {
                query,
                from,
                to,
                range,
                json_output: json,
                limit,
                count_only,
                case_sensitive,
                whole_word,
                context_chars: context,
                match_mode,
                sort,
                hits_per_entry,
                summary,
                preset_name: preset,
                list_presets,
                save_preset,
                delete_preset,
            }) {
                log::error!("search failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some(Command::Spellcheck {
            date,
            from,
            to,
            range,
            json,
            limit,
            count_only,
        }) => {
            let date = match parse_optional_date_arg("date", date.as_deref()) {
                Ok(date) => date,
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(2);
                }
            };
            let from = match parse_optional_date_arg("from", from.as_deref()) {
                Ok(date) => date,
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(2);
                }
            };
            let to = match parse_optional_date_arg("to", to.as_deref()) {
                Ok(date) => date,
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(2);
                }
            };

            if let Err(error) = run_cli_spellcheck(SpellcheckCliArgs {
                date,
                from,
                to,
                range,
                json_output: json,
                limit,
                count_only,
            }) {
                log::error!("spellcheck failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some(Command::Review {
            top,
            on_this_day,
            from,
            to,
            range,
            json,
            min_count,
            goal,
        }) => {
            let from = match parse_optional_date_arg("from", from.as_deref()) {
                Ok(date) => date,
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(2);
                }
            };
            let to = match parse_optional_date_arg("to", to.as_deref()) {
                Ok(date) => date,
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(2);
                }
            };

            if let Err(error) = run_cli_review(ReviewCliArgs {
                top,
                on_this_day_limit: on_this_day,
                from,
                to,
                range,
                json_output: json,
                min_count,
                goal_override: goal,
            }) {
                log::error!("review failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some(Command::Timeline {
            from,
            to,
            range,
            query,
            tag,
            person,
            project,
            mood,
            has_tags,
            has_people,
            has_project,
            weekday,
            limit,
            asc,
            favorites,
            conflicts,
            metadata,
            group_by,
            format,
            summary,
            preset,
            list_presets,
            save_preset,
            delete_preset,
        }) => {
            let from = match parse_optional_date_arg("from", from.as_deref()) {
                Ok(date) => date,
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(2);
                }
            };
            let to = match parse_optional_date_arg("to", to.as_deref()) {
                Ok(date) => date,
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(2);
                }
            };

            if let Err(error) = run_cli_timeline(TimelineFilters {
                from,
                to,
                range,
                query,
                tag,
                person,
                project,
                mood,
                has_tags_only: has_tags,
                has_people_only: has_people,
                has_project_only: has_project,
                weekdays: weekday.into_iter().map(WeekdayArg::to_chrono).collect(),
                limit,
                asc,
                favorites_only: favorites,
                conflicts_only: conflicts,
                show_metadata: metadata,
                group_by,
                format,
                summary_only: summary,
                preset_name: preset,
                list_presets,
                save_preset,
                delete_preset,
            }) {
                log::error!("timeline failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some(Command::Next { from }) => {
            let from = match parse_optional_date_arg("from", from.as_deref()) {
                Ok(date) => date,
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(2);
                }
            };

            if let Err(error) = run_cli_next(from) {
                log::error!("next failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some(Command::Prompts { command }) => {
            if let Err(error) = run_cli_prompts(command) {
                log::error!("prompts failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some(Command::Ai { command }) => {
            if let Err(error) = run_cli_ai(command) {
                log::error!("ai command failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some(Command::Sync { backend, remote }) => {
            if let Err(error) = run_cli_sync(backend, remote.as_deref()) {
                log::error!("sync failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some(Command::Backup { command }) => {
            if let Err(error) = run_cli_backup(command) {
                log::error!("backup failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some(Command::Restore { backup, into }) => {
            if let Err(error) = run_cli_restore(&backup, &into) {
                log::error!("restore failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some(Command::Verify) => {
            if let Err(error) = run_cli_verify() {
                log::error!("verify failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some(Command::Settings { json, command }) => {
            if let Err(error) = run_cli_settings(json, command) {
                log::error!("settings failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some(Command::Guide { topic }) => {
            if let Err(error) = run_cli_guide(topic) {
                log::error!("guide failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some(Command::Doctor { unlock, json }) => match run_cli_doctor(unlock, json) {
            Ok(true) => {}
            Ok(false) => std::process::exit(1),
            Err(error) => {
                log::error!("doctor failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        },
        Some(Command::Sysop { command }) => {
            if let Err(error) = run_cli_sysop(command) {
                log::error!("sysop failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some(Command::Completions { shell }) => {
            if let Err(error) = run_cli_completions(shell) {
                log::error!("completion generation failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        None => {
            log::info!("launching TUI");
            if let Err(error) = tui::run(None) {
                log::error!("failed to launch TUI: {error}");
                eprintln!("failed to launch TUI: {error}");
                std::process::exit(1);
            }
        }
    }
}

fn parse_optional_date_arg(label: &str, value: Option<&str>) -> Result<Option<NaiveDate>, String> {
    value.map(|value| parse_date_arg(label, value)).transpose()
}

fn parse_date_arg(label: &str, value: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| format!("invalid {label} '{value}'; expected YYYY-MM-DD"))
}

fn resolve_range_bounds(range: DateRangeArg, today: NaiveDate) -> (NaiveDate, NaiveDate) {
    match range {
        DateRangeArg::Today => (today, today),
        DateRangeArg::Yesterday => {
            let date = today - chrono::Duration::days(1);
            (date, date)
        }
        DateRangeArg::Last7 => (today - chrono::Duration::days(6), today),
        DateRangeArg::Last30 => (today - chrono::Duration::days(29), today),
        DateRangeArg::ThisMonth => {
            let from = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).expect("date");
            (from, today)
        }
        DateRangeArg::LastMonth => {
            let first_of_month =
                NaiveDate::from_ymd_opt(today.year(), today.month(), 1).expect("date");
            let last_of_previous = first_of_month - chrono::Duration::days(1);
            let from =
                NaiveDate::from_ymd_opt(last_of_previous.year(), last_of_previous.month(), 1)
                    .expect("date");
            (from, last_of_previous)
        }
        DateRangeArg::Ytd => {
            let from = NaiveDate::from_ymd_opt(today.year(), 1, 1).expect("date");
            (from, today)
        }
    }
}

#[derive(Debug)]
struct SearchCliArgs {
    query: Option<String>,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    range: Option<DateRangeArg>,
    json_output: bool,
    limit: usize,
    count_only: bool,
    case_sensitive: bool,
    whole_word: bool,
    context_chars: usize,
    match_mode: SearchMatchArg,
    sort: SearchSortArg,
    hits_per_entry: usize,
    summary: bool,
    preset_name: Option<String>,
    list_presets: bool,
    save_preset: Option<String>,
    delete_preset: Option<String>,
}

#[derive(Debug)]
struct SpellcheckCliArgs {
    date: Option<NaiveDate>,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    range: Option<DateRangeArg>,
    json_output: bool,
    limit: usize,
    count_only: bool,
}

#[derive(Clone, Debug, Serialize)]
struct SpellcheckCliHit {
    date: String,
    entry_number: String,
    row: usize,
    col: usize,
    word: String,
    suggestions: Vec<String>,
}

fn run_cli_spellcheck(args: SpellcheckCliArgs) -> Result<(), String> {
    log::info!("running CLI spellcheck");
    if args.date.is_some() && (args.from.is_some() || args.to.is_some() || args.range.is_some()) {
        return Err("--date cannot be combined with --from/--to/--range".to_string());
    }

    let (from, to) = if let Some(date) = args.date {
        (Some(date), Some(date))
    } else {
        let (range_from, range_to) = args
            .range
            .map(|range| resolve_range_bounds(range, Local::now().date_naive()))
            .map_or((None, None), |(from, to)| (Some(from), Some(to)));
        (args.from.or(range_from), args.to.or(range_to))
    };

    if let (Some(from), Some(to)) = (from, to)
        && from > to
    {
        return Err("--from cannot be after --to".to_string());
    }

    let config = config::AppConfig::load_or_default();
    let vault = unlock_cli_vault(&config)?;
    let documents = vault
        .load_search_documents()
        .map_err(|error| format!("failed to read entries: {error}"))?;
    let checker = spellcheck::SpellChecker::load();
    let session_words = HashSet::new();

    let mut hits = Vec::<SpellcheckCliHit>::new();
    for document in documents {
        if !search::matches_date_filter(document.date, from, to) {
            continue;
        }
        for hit in checker.check_text(&document.body, &session_words) {
            hits.push(SpellcheckCliHit {
                date: document.date.format("%Y-%m-%d").to_string(),
                entry_number: document.entry_number.clone(),
                row: hit.row + 1,
                col: hit.start_col + 1,
                word: hit.word,
                suggestions: hit.suggestions,
            });
        }
    }

    hits.sort_by(|left, right| {
        left.date
            .cmp(&right.date)
            .then_with(|| left.row.cmp(&right.row))
            .then_with(|| left.col.cmp(&right.col))
    });

    if args.limit > 0 && hits.len() > args.limit {
        hits.truncate(args.limit);
    }

    if args.count_only {
        if args.json_output {
            println!("{}", serde_json::json!({ "count": hits.len() }));
        } else {
            println!("{}", hits.len());
        }
        return Ok(());
    }

    if hits.is_empty() {
        if args.json_output {
            println!("[]");
        } else {
            println!("No spelling issues found.");
        }
        return Ok(());
    }

    if args.json_output {
        let payload = serde_json::to_string_pretty(&hits)
            .map_err(|error| format!("failed to serialize spellcheck JSON: {error}"))?;
        println!("{payload}");
    } else {
        for hit in hits {
            let suggestion = hit
                .suggestions
                .first()
                .cloned()
                .unwrap_or_else(|| "(no suggestion)".to_string());
            println!(
                "{}:{}:{}  {}  -> {}",
                hit.date, hit.row, hit.col, hit.word, suggestion
            );
        }
    }

    Ok(())
}

fn run_cli_search(args: SearchCliArgs) -> Result<(), String> {
    let SearchCliArgs {
        query,
        from,
        to,
        range,
        json_output,
        limit,
        count_only,
        case_sensitive,
        whole_word,
        context_chars,
        match_mode,
        sort,
        hits_per_entry,
        summary,
        preset_name,
        list_presets,
        save_preset,
        delete_preset,
    } = args;

    log::info!("running CLI search");
    if hits_per_entry == 0 {
        return Err("--hits-per-entry must be at least 1".to_string());
    }
    let mut config = config::AppConfig::load_or_default();

    if list_presets {
        let lines = render_search_preset_lines(&config);
        if lines.is_empty() {
            println!("No saved search presets.");
        } else {
            for line in lines {
                println!("{line}");
            }
        }
        return Ok(());
    }

    if let Some(name) = delete_preset.as_deref() {
        if config.remove_search_preset(name) {
            config
                .save()
                .map_err(|error| format!("failed to save config: {error}"))?;
            println!("Deleted preset: {}", name.trim());
            return Ok(());
        }
        return Err(format!("search preset not found: {name}"));
    }

    let preset = if let Some(preset_name) = preset_name.as_deref() {
        Some(
            config
                .search_preset(preset_name)
                .cloned()
                .ok_or_else(|| format!("search preset not found: {preset_name}"))?,
        )
    } else {
        None
    };

    let effective_query = query
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| preset.as_ref().map(|preset| preset.query.clone()))
        .ok_or_else(|| "missing search query; provide QUERY or --preset NAME".to_string())?;

    let preset_from = parse_optional_date_arg(
        "preset from",
        preset.as_ref().and_then(|preset| preset.from.as_deref()),
    )?;
    let preset_to = parse_optional_date_arg(
        "preset to",
        preset.as_ref().and_then(|preset| preset.to.as_deref()),
    )?;
    let (range_from, range_to) = range
        .map(|range| resolve_range_bounds(range, Local::now().date_naive()))
        .map_or((None, None), |(from, to)| (Some(from), Some(to)));
    let from = from.or(range_from).or(preset_from);
    let to = to.or(range_to).or(preset_to);

    if let (Some(from), Some(to)) = (from, to) {
        if from > to {
            return Err("--from cannot be after --to".to_string());
        }
    }

    if let Some(save_name) = save_preset.as_deref() {
        config.upsert_search_preset(config::SearchPresetConfig {
            name: save_name.trim().to_string(),
            query: effective_query.clone(),
            from: from.map(|date| date.format("%Y-%m-%d").to_string()),
            to: to.map(|date| date.format("%Y-%m-%d").to_string()),
        })?;
        config
            .save()
            .map_err(|error| format!("failed to save config: {error}"))?;
        println!("Saved preset: {}", save_name.trim());
    }

    let vault = unlock_cli_vault(&config)?;
    let documents = vault
        .load_search_documents()
        .map_err(|error| format!("failed to read entries: {error}"))?;
    let index = search::SearchIndex::build(documents);
    let mut results = index.search_with_options(
        &search::SearchQuery {
            text: effective_query.clone(),
            from,
            to,
        },
        &search::SearchOptions {
            case_sensitive,
            whole_word,
            context_chars,
            match_mode: match_mode.to_search_mode(),
            max_hits_per_document: hits_per_entry,
        },
    );

    sort_search_results(&mut results, sort, &effective_query, case_sensitive);

    if limit > 0 && results.len() > limit {
        results.truncate(limit);
    }

    if summary {
        let summary = build_search_summary(&results);
        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&summary)
                    .map_err(|error| format!("json failed: {error}"))?
            );
        } else {
            println!("Search Summary");
            println!("Matches      : {}", summary.total_matches);
            println!("Dates matched: {}", summary.matched_dates);
            println!(
                "Date range   : {} .. {}",
                summary.first_date.as_deref().unwrap_or("-"),
                summary.last_date.as_deref().unwrap_or("-")
            );
            println!("Top dates");
            if summary.top_dates.is_empty() {
                println!("  (none)");
            } else {
                for (date, count) in &summary.top_dates {
                    println!("  {date}: {count}");
                }
            }
        }
        return Ok(());
    }

    if count_only {
        if json_output {
            println!("{}", serde_json::json!({ "count": results.len() }));
        } else {
            println!("{}", results.len());
        }
        return Ok(());
    }

    if results.is_empty() {
        println!("No matches.");
        return Ok(());
    }

    if json_output {
        #[derive(Serialize)]
        struct CliSearchResult {
            date: String,
            entry_number: String,
            row: usize,
            start_col: usize,
            end_col: usize,
            snippet: String,
            highlight_start: usize,
            highlight_end: usize,
            matched_text: String,
        }

        let payload = results
            .into_iter()
            .map(|result| CliSearchResult {
                date: result.date.format("%Y-%m-%d").to_string(),
                entry_number: result.entry_number,
                row: result.row + 1,
                start_col: result.start_col + 1,
                end_col: result.end_col + 1,
                snippet: result.snippet.text,
                highlight_start: result.snippet.highlight_start + 1,
                highlight_end: result.snippet.highlight_end + 1,
                matched_text: result.matched_text,
            })
            .collect::<Vec<_>>();

        let json = serde_json::to_string_pretty(&payload)
            .map_err(|error| format!("json failed: {error}"))?;
        println!("{json}");
        return Ok(());
    }

    for result in results {
        println!(
            "{}:{}:{}  {}",
            result.date.format("%Y-%m-%d"),
            result.row + 1,
            result.start_col + 1,
            search::format_cli_snippet(&result.snippet)
        );
    }

    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct SearchSummaryCli {
    total_matches: usize,
    matched_dates: usize,
    first_date: Option<String>,
    last_date: Option<String>,
    top_dates: Vec<(String, usize)>,
}

fn sort_search_results(
    results: &mut [search::SearchResult],
    sort: SearchSortArg,
    query_text: &str,
    case_sensitive: bool,
) {
    match sort {
        SearchSortArg::Newest => {
            results.sort_by(|left, right| {
                right
                    .date
                    .cmp(&left.date)
                    .then_with(|| left.row.cmp(&right.row))
                    .then_with(|| left.start_col.cmp(&right.start_col))
            });
        }
        SearchSortArg::Oldest => {
            results.sort_by(|left, right| {
                left.date
                    .cmp(&right.date)
                    .then_with(|| left.row.cmp(&right.row))
                    .then_with(|| left.start_col.cmp(&right.start_col))
            });
        }
        SearchSortArg::Relevance => {
            let query_cmp = if case_sensitive {
                query_text.to_string()
            } else {
                query_text.to_lowercase()
            };
            results.sort_by(|left, right| {
                let left_score = search_relevance_score(left, &query_cmp, case_sensitive);
                let right_score = search_relevance_score(right, &query_cmp, case_sensitive);
                right_score
                    .cmp(&left_score)
                    .then_with(|| right.date.cmp(&left.date))
                    .then_with(|| left.row.cmp(&right.row))
                    .then_with(|| left.start_col.cmp(&right.start_col))
            });
        }
    }
}

fn search_relevance_score(
    result: &search::SearchResult,
    query_cmp: &str,
    case_sensitive: bool,
) -> usize {
    let text = if case_sensitive {
        result.matched_text.clone()
    } else {
        result.matched_text.to_lowercase()
    };
    let exact_bonus = usize::from(text == query_cmp) * 1000;
    exact_bonus + text.chars().count()
}

fn build_search_summary(results: &[search::SearchResult]) -> SearchSummaryCli {
    let mut counts = BTreeMap::<String, usize>::new();
    for result in results {
        let key = result.date.format("%Y-%m-%d").to_string();
        *counts.entry(key).or_insert(0) += 1;
    }

    let mut top_dates = counts
        .iter()
        .map(|(date, count)| (date.clone(), *count))
        .collect::<Vec<_>>();
    top_dates.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| right.0.cmp(&left.0)));
    top_dates.truncate(10);

    SearchSummaryCli {
        total_matches: results.len(),
        matched_dates: counts.len(),
        first_date: results
            .iter()
            .map(|result| result.date)
            .min()
            .map(|date| date.format("%Y-%m-%d").to_string()),
        last_date: results
            .iter()
            .map(|result| result.date)
            .max()
            .map(|date| date.format("%Y-%m-%d").to_string()),
        top_dates,
    }
}

#[derive(Clone, Debug)]
struct TimelineFilters {
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    range: Option<DateRangeArg>,
    query: Option<String>,
    tag: Option<String>,
    person: Option<String>,
    project: Option<String>,
    mood: Option<u8>,
    has_tags_only: bool,
    has_people_only: bool,
    has_project_only: bool,
    weekdays: HashSet<Weekday>,
    limit: usize,
    asc: bool,
    favorites_only: bool,
    conflicts_only: bool,
    show_metadata: bool,
    group_by: Option<TimelineGroupByArg>,
    format: TimelineFormatArg,
    summary_only: bool,
    preset_name: Option<String>,
    list_presets: bool,
    save_preset: Option<String>,
    delete_preset: Option<String>,
}

#[derive(Clone, Debug)]
struct ReviewCliArgs {
    top: usize,
    on_this_day_limit: usize,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    range: Option<DateRangeArg>,
    json_output: bool,
    min_count: usize,
    goal_override: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct ReviewCliSummary {
    review_date: String,
    from: Option<String>,
    to: Option<String>,
    total_entries: usize,
    streak_days: usize,
    entries_this_week: usize,
    entries_this_month: usize,
    top_tags: Vec<(String, usize)>,
    top_people: Vec<(String, usize)>,
    top_projects: Vec<(String, usize)>,
    on_this_day: Vec<ReviewHitCli>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct ReviewHitCli {
    date: String,
    entry_number: String,
    preview: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct ReviewWordStatsCli {
    total_words: usize,
    entries_counted: usize,
    avg_words_per_entry: f64,
    active_days: usize,
    goal: Option<usize>,
    days_meeting_goal: usize,
    goal_hit_rate: f64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct TimelineRowCli {
    date: String,
    entry_number: String,
    favorite: bool,
    conflict: bool,
    preview: String,
    metadata: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct TimelineSummaryCli {
    total: usize,
    conflicts: usize,
    favorites: usize,
    first_date: Option<String>,
    last_date: Option<String>,
    moods: Vec<(u8, usize)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct TimelineGroupRowCli {
    group: String,
    entries: usize,
    conflicts: usize,
    favorites: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PromptTemplate {
    category: PromptCategoryArg,
    title: &'static str,
    body: &'static str,
}

const PROMPT_TEMPLATES: &[PromptTemplate] = &[
    PromptTemplate {
        category: PromptCategoryArg::Reflection,
        title: "Signal vs. noise",
        body: "What felt truly important today, and what only felt urgent?",
    },
    PromptTemplate {
        category: PromptCategoryArg::Reflection,
        title: "Future me note",
        body: "Write a note to next month you: what should stay the same?",
    },
    PromptTemplate {
        category: PromptCategoryArg::Gratitude,
        title: "Small wins",
        body: "Name three small things that made today better than expected.",
    },
    PromptTemplate {
        category: PromptCategoryArg::Gratitude,
        title: "Unexpected support",
        body: "Who helped you this week, directly or indirectly, and how?",
    },
    PromptTemplate {
        category: PromptCategoryArg::Growth,
        title: "Stretch point",
        body: "Where did you feel resistance today, and what did it teach you?",
    },
    PromptTemplate {
        category: PromptCategoryArg::Growth,
        title: "Repeat or redesign",
        body: "What process should you repeat tomorrow, and what should you redesign?",
    },
    PromptTemplate {
        category: PromptCategoryArg::Focus,
        title: "One needle mover",
        body: "If tomorrow only had one meaningful hour, where should it go?",
    },
    PromptTemplate {
        category: PromptCategoryArg::Focus,
        title: "Constraint clarity",
        body: "What is the real bottleneck right now, and what removes it?",
    },
    PromptTemplate {
        category: PromptCategoryArg::Relationships,
        title: "Repair and reinforce",
        body: "Is there a relationship to repair, or one worth reinforcing this week?",
    },
    PromptTemplate {
        category: PromptCategoryArg::Relationships,
        title: "Conversation to have",
        body: "What conversation are you avoiding that would improve things quickly?",
    },
];

fn run_cli_review(args: ReviewCliArgs) -> Result<(), String> {
    log::info!("running CLI review");
    if args.goal_override == Some(0) {
        return Err("--goal must be at least 1".to_string());
    }
    let (range_from, range_to) = args
        .range
        .map(|range| resolve_range_bounds(range, Local::now().date_naive()))
        .map_or((None, None), |(from, to)| (Some(from), Some(to)));
    let from = args.from.or(range_from);
    let to = args.to.or(range_to);

    if let (Some(from), Some(to)) = (from, to)
        && from > to
    {
        return Err("--from cannot be after --to".to_string());
    }

    let config = config::AppConfig::load_or_default();
    let vault = unlock_cli_vault(&config)?;
    let today = Local::now().date_naive();
    let entries = vault
        .list_index_entries(120)
        .map_err(|error| format!("review failed: {error}"))?;
    let mut review = summarize_review_entries(entries, today, from, to, args.min_count);
    review.on_this_day.truncate(args.on_this_day_limit.max(1));
    let word_stats = compute_review_word_stats(
        &vault
            .load_search_documents()
            .map_err(|error| format!("review failed: {error}"))?,
        from,
        to,
        args.goal_override.or(config.daily_word_goal),
    );

    if args.json_output {
        #[derive(Serialize)]
        struct ReviewOutput<'a> {
            summary: &'a ReviewCliSummary,
            words: ReviewWordStatsCli,
        }
        let payload = ReviewOutput {
            summary: &review,
            words: word_stats.clone(),
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&payload)
                .map_err(|error| format!("failed to serialize review JSON: {error}"))?
        );
        return Ok(());
    }

    println!("Review Date: {}", review.review_date);
    if let Some(from) = &review.from {
        println!("From        : {from}");
    }
    if let Some(to) = &review.to {
        println!("To          : {to}");
    }
    println!("Total entries : {}", review.total_entries);
    println!("Current streak: {} day(s)", review.streak_days);
    println!("This week     : {}", review.entries_this_week);
    println!("This month    : {}", review.entries_this_month);
    println!("Words total   : {}", word_stats.total_words);
    println!("Words/entry   : {:.1}", word_stats.avg_words_per_entry);
    if let Some(goal) = word_stats.goal {
        println!(
            "Goal ({goal}/day): {}/{} day(s) ({:.0}%)",
            word_stats.days_meeting_goal,
            word_stats.active_days,
            word_stats.goal_hit_rate * 100.0
        );
    }
    println!();

    println!("Top tags");
    print_rank_lines(&review.top_tags, args.top);
    println!("Top people");
    print_rank_lines(&review.top_people, args.top);
    println!("Top projects");
    print_rank_lines(&review.top_projects, args.top);
    println!("On this day");
    if review.on_this_day.is_empty() {
        println!("  (none yet)");
    } else {
        for hit in &review.on_this_day {
            println!("  {}  {}  {}", hit.date, hit.entry_number, hit.preview);
        }
    }

    Ok(())
}

fn run_cli_timeline(mut filters: TimelineFilters) -> Result<(), String> {
    log::info!("running CLI timeline");
    let mut config = config::AppConfig::load_or_default();

    if filters.list_presets {
        let lines = render_timeline_preset_lines(&config);
        if lines.is_empty() {
            println!("No saved timeline presets.");
        } else {
            for line in lines {
                println!("{line}");
            }
        }
        return Ok(());
    }

    if let Some(name) = filters.delete_preset.as_deref() {
        if config.remove_timeline_preset(name) {
            config
                .save()
                .map_err(|error| format!("failed to save config: {error}"))?;
            println!("Deleted timeline preset: {}", name.trim());
            return Ok(());
        }
        return Err(format!("timeline preset not found: {name}"));
    }

    let preset = if let Some(name) = filters.preset_name.as_deref() {
        Some(
            config
                .timeline_preset(name)
                .cloned()
                .ok_or_else(|| format!("timeline preset not found: {name}"))?,
        )
    } else {
        None
    };

    let preset_from = parse_optional_date_arg(
        "preset from",
        preset.as_ref().and_then(|p| p.from.as_deref()),
    )?;
    let preset_to =
        parse_optional_date_arg("preset to", preset.as_ref().and_then(|p| p.to.as_deref()))?;
    let (range_from, range_to) = filters
        .range
        .map(|range| resolve_range_bounds(range, Local::now().date_naive()))
        .map_or((None, None), |(from, to)| (Some(from), Some(to)));

    filters.from = filters.from.or(range_from).or(preset_from);
    filters.to = filters.to.or(range_to).or(preset_to);
    if filters.query.is_none() {
        filters.query = preset.as_ref().and_then(|p| p.query.clone());
    }
    if filters.tag.is_none() {
        filters.tag = preset.as_ref().and_then(|p| p.tag.clone());
    }
    if filters.person.is_none() {
        filters.person = preset.as_ref().and_then(|p| p.person.clone());
    }
    if filters.project.is_none() {
        filters.project = preset.as_ref().and_then(|p| p.project.clone());
    }
    if filters.mood.is_none() {
        filters.mood = preset.as_ref().and_then(|p| p.mood);
    }

    if let Some(name) = filters.save_preset.as_deref() {
        config.upsert_timeline_preset(config::TimelinePresetConfig {
            name: name.trim().to_string(),
            from: filters.from.map(|date| date.format("%Y-%m-%d").to_string()),
            to: filters.to.map(|date| date.format("%Y-%m-%d").to_string()),
            query: filters
                .query
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            tag: filters
                .tag
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            person: filters
                .person
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            project: filters
                .project
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            mood: filters.mood,
        })?;
        config
            .save()
            .map_err(|error| format!("failed to save config: {error}"))?;
        println!("Saved timeline preset: {}", name.trim());
    }

    if let (Some(from), Some(to)) = (filters.from, filters.to)
        && from > to
    {
        return Err("--from cannot be after --to".to_string());
    }

    let vault = unlock_cli_vault(&config)?;
    let entries = vault
        .list_index_entries(120)
        .map_err(|error| format!("timeline failed: {error}"))?;
    let favorite_dates = parse_favorite_dates(&config.favorite_dates);
    let show_metadata = filters.show_metadata;
    let format = filters.format;
    let summary_only = filters.summary_only;
    let group_by = filters.group_by;
    let entries = filter_timeline_entries(entries, filters, &favorite_dates);

    if entries.is_empty() {
        match format {
            TimelineFormatArg::Text => {
                println!("No entries found for the selected timeline filters.");
            }
            TimelineFormatArg::Json => println!("[]"),
            TimelineFormatArg::Csv => {
                if group_by.is_some() {
                    println!("group,entries,conflicts,favorites");
                } else {
                    println!("date,entry_number,favorite,conflict,preview,metadata");
                }
            }
        }
        return Ok(());
    }

    if let Some(group_by) = group_by {
        let rows = group_timeline_entries(&entries, &favorite_dates, group_by);
        match format {
            TimelineFormatArg::Text => {
                for row in &rows {
                    println!(
                        "{}  entries={} conflicts={} favorites={}",
                        row.group, row.entries, row.conflicts, row.favorites
                    );
                }
            }
            TimelineFormatArg::Json => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&rows)
                        .map_err(|error| format!("failed to serialize timeline JSON: {error}"))?
                );
            }
            TimelineFormatArg::Csv => {
                println!("group,entries,conflicts,favorites");
                for row in &rows {
                    println!(
                        "{},{},{},{}",
                        escape_csv_cell(&row.group),
                        row.entries,
                        row.conflicts,
                        row.favorites
                    );
                }
            }
        }
        return Ok(());
    }

    if summary_only {
        let summary = build_timeline_summary(&entries, &favorite_dates);
        match format {
            TimelineFormatArg::Text => {
                println!("Timeline Summary");
                println!("Total      : {}", summary.total);
                println!("Conflicts  : {}", summary.conflicts);
                println!("Favorites  : {}", summary.favorites);
                println!(
                    "Date range : {} .. {}",
                    summary.first_date.as_deref().unwrap_or("-"),
                    summary.last_date.as_deref().unwrap_or("-")
                );
                println!("Moods");
                if summary.moods.is_empty() {
                    println!("  (none yet)");
                } else {
                    for (mood, count) in &summary.moods {
                        println!("  {mood}: {count}");
                    }
                }
            }
            TimelineFormatArg::Json => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&summary)
                        .map_err(|error| format!("failed to serialize timeline JSON: {error}"))?
                );
            }
            TimelineFormatArg::Csv => {
                println!("metric,value");
                println!("total,{}", summary.total);
                println!("conflicts,{}", summary.conflicts);
                println!("favorites,{}", summary.favorites);
                println!(
                    "first_date,{}",
                    summary.first_date.as_deref().unwrap_or_default()
                );
                println!(
                    "last_date,{}",
                    summary.last_date.as_deref().unwrap_or_default()
                );
                for (mood, count) in &summary.moods {
                    println!("mood_{mood},{count}");
                }
            }
        }
        return Ok(());
    }

    match format {
        TimelineFormatArg::Text => {
            for entry in &entries {
                let favorite_marker = if favorite_dates.contains(&entry.date) {
                    '*'
                } else {
                    ' '
                };
                let conflict_marker = if entry.has_conflict { " CONFLICT" } else { "" };
                println!(
                    "{favorite_marker} {}  {}{}  {}",
                    entry.date.format("%Y-%m-%d"),
                    entry.entry_number,
                    conflict_marker,
                    entry.preview
                );
                if show_metadata && let Some(stamp) = format_metadata_stamp(&entry.metadata) {
                    println!("    [{stamp}]");
                }
            }
        }
        TimelineFormatArg::Json => {
            let rows = entries
                .iter()
                .map(|entry| timeline_row_cli(entry, &favorite_dates, show_metadata))
                .collect::<Vec<_>>();
            println!(
                "{}",
                serde_json::to_string_pretty(&rows)
                    .map_err(|error| format!("failed to serialize timeline JSON: {error}"))?
            );
        }
        TimelineFormatArg::Csv => {
            println!("date,entry_number,favorite,conflict,preview,metadata");
            for row in entries
                .iter()
                .map(|entry| timeline_row_cli(entry, &favorite_dates, show_metadata))
            {
                println!(
                    "{},{},{},{},{},{}",
                    row.date,
                    escape_csv_cell(&row.entry_number),
                    row.favorite,
                    row.conflict,
                    escape_csv_cell(&row.preview),
                    escape_csv_cell(row.metadata.as_deref().unwrap_or_default())
                );
            }
        }
    }

    Ok(())
}

fn run_cli_next(start_from: Option<NaiveDate>) -> Result<(), String> {
    log::info!("running CLI next");
    let start = start_from.unwrap_or_else(|| Local::now().date_naive());
    let config = config::AppConfig::load_or_default();
    let target = if vault::vault_exists(&config.vault_path) {
        let vault = unlock_cli_vault(&config)?;
        let existing = vault
            .list_entry_dates()
            .map_err(|error| format!("next date discovery failed: {error}"))?;
        let existing_set = existing.into_iter().collect::<HashSet<_>>();
        next_blank_date(start, &existing_set)
    } else {
        start
    };
    tui::run(Some(target)).map_err(|error| format!("failed to launch TUI: {error}"))
}

fn run_cli_prompts(command: PromptCommand) -> Result<(), String> {
    log::info!("running CLI prompts");
    match command {
        PromptCommand::List { category, json } => {
            let prompts = prompts_for_category(category);
            if json {
                #[derive(Serialize)]
                struct PromptRow {
                    category: String,
                    title: String,
                    body: String,
                }
                let rows = prompts
                    .into_iter()
                    .map(|prompt| PromptRow {
                        category: prompt_category_label(prompt.category).to_string(),
                        title: prompt.title.to_string(),
                        body: prompt.body.to_string(),
                    })
                    .collect::<Vec<_>>();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&rows)
                        .map_err(|error| format!("failed to serialize prompts JSON: {error}"))?
                );
                return Ok(());
            }

            if prompts.is_empty() {
                println!("No prompts in this category.");
                return Ok(());
            }
            for prompt in prompts {
                println!(
                    "{}\t{}\t{}",
                    prompt_category_label(prompt.category),
                    prompt.title,
                    prompt.body
                );
            }
        }
        PromptCommand::Pick {
            category,
            date,
            json,
        } => {
            let date = parse_optional_date_arg("date", date.as_deref())?
                .unwrap_or_else(|| Local::now().date_naive());
            let Some(prompt) = deterministic_prompt_for_date(date, category) else {
                return Err("no prompts available for this category".to_string());
            };
            if json {
                #[derive(Serialize)]
                struct PromptPick {
                    date: String,
                    category: String,
                    title: String,
                    body: String,
                }
                let payload = PromptPick {
                    date: date.format("%Y-%m-%d").to_string(),
                    category: prompt_category_label(prompt.category).to_string(),
                    title: prompt.title.to_string(),
                    body: prompt.body.to_string(),
                };
                println!(
                    "{}",
                    serde_json::to_string_pretty(&payload)
                        .map_err(|error| format!("failed to serialize prompt JSON: {error}"))?
                );
                return Ok(());
            }
            println!("Date     : {}", date.format("%Y-%m-%d"));
            println!("Category : {}", prompt_category_label(prompt.category));
            println!("Title    : {}", prompt.title);
            println!();
            println!("{}", prompt.body);
        }
    }
    Ok(())
}

fn run_cli_ai(command: AiCommand) -> Result<(), String> {
    log::info!("running CLI ai command");
    let config = config::AppConfig::load_or_default();
    let vault = unlock_cli_vault(&config)?;

    match command {
        AiCommand::Summary {
            date,
            from,
            to,
            range,
            max_points,
            json,
            remote,
        } => {
            let date = parse_optional_date_arg("date", date.as_deref())?;
            let mut from = parse_optional_date_arg("from", from.as_deref())?;
            let mut to = parse_optional_date_arg("to", to.as_deref())?;
            if let Some(range) = range {
                let (range_from, range_to) = resolve_range_bounds(range, Local::now().date_naive());
                from = from.or(Some(range_from));
                to = to.or(Some(range_to));
            }
            if let Some(date) = date {
                from = Some(date);
                to = Some(date);
            }
            if let (Some(from), Some(to)) = (from, to)
                && from > to
            {
                return Err("--from cannot be after --to".to_string());
            }

            let documents = vault
                .load_search_documents()
                .map_err(|error| format!("failed to load entries: {error}"))?;
            let mut selected = documents
                .into_iter()
                .filter(|doc| search::matches_date_filter(doc.date, from, to))
                .collect::<Vec<_>>();
            if selected.is_empty() {
                return Err("no entries found for requested AI summary scope".to_string());
            }
            selected.sort_by(|left, right| left.date.cmp(&right.date));
            let context = selected
                .iter()
                .map(|doc| {
                    format!(
                        "DATE {}\nENTRY {}\n{}\n",
                        doc.date.format("%Y-%m-%d"),
                        doc.entry_number,
                        doc.body
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");

            let mode = if remote {
                ai::AiRequestMode::RemoteIfConfigured
            } else {
                ai::AiRequestMode::LocalOnly
            };
            let summary = ai::summarize_text(&context, max_points.max(1), mode);
            if json {
                #[derive(Serialize)]
                struct Payload {
                    provider: String,
                    from: Option<String>,
                    to: Option<String>,
                    entry_count: usize,
                    summary: String,
                }
                let payload = Payload {
                    provider: summary.provider,
                    from: from.map(|value| value.format("%Y-%m-%d").to_string()),
                    to: to.map(|value| value.format("%Y-%m-%d").to_string()),
                    entry_count: selected.len(),
                    summary: summary.text,
                };
                println!(
                    "{}",
                    serde_json::to_string_pretty(&payload)
                        .map_err(|error| format!("failed to serialize AI summary JSON: {error}"))?
                );
                return Ok(());
            }

            println!(
                "AI Summary [{}] {}..{} ({} entries)\n",
                summary.provider,
                from.map(|value| value.format("%Y-%m-%d").to_string())
                    .unwrap_or_else(|| "start".to_string()),
                to.map(|value| value.format("%Y-%m-%d").to_string())
                    .unwrap_or_else(|| "now".to_string()),
                selected.len(),
            );
            println!("{}", summary.text);
        }
        AiCommand::Coach {
            date,
            questions,
            json,
            remote,
        } => {
            let date = parse_optional_date_arg("date", date.as_deref())?
                .unwrap_or_else(|| Local::now().date_naive());
            let context = vault
                .export_entry(date)
                .map_err(|error| format!("failed to read entry: {error}"))?
                .map(|entry| entry.body)
                .unwrap_or_default();
            let mode = if remote {
                ai::AiRequestMode::RemoteIfConfigured
            } else {
                ai::AiRequestMode::LocalOnly
            };
            let coach = ai::coach_questions(&context, questions.max(1), mode);
            if json {
                #[derive(Serialize)]
                struct Payload {
                    provider: String,
                    date: String,
                    questions: Vec<String>,
                }
                println!(
                    "{}",
                    serde_json::to_string_pretty(&Payload {
                        provider: coach.provider,
                        date: date.format("%Y-%m-%d").to_string(),
                        questions: coach.questions,
                    })
                    .map_err(|error| format!("failed to serialize AI coach JSON: {error}"))?
                );
                return Ok(());
            }

            println!(
                "AI Coach [{}] for {}\n",
                coach.provider,
                date.format("%Y-%m-%d")
            );
            for (idx, question) in coach.questions.iter().enumerate() {
                println!("Q{}: {}", idx + 1, question);
            }
        }
    }

    Ok(())
}

fn filter_timeline_entries(
    mut entries: Vec<vault::IndexEntry>,
    filters: TimelineFilters,
    favorite_dates: &HashSet<NaiveDate>,
) -> Vec<vault::IndexEntry> {
    let query_filter = normalize_filter(filters.query.as_deref());
    let tag_filter = normalize_filter(filters.tag.as_deref());
    let person_filter = normalize_filter(filters.person.as_deref());
    let project_filter = normalize_filter(filters.project.as_deref());

    entries.retain(|entry| {
        if let Some(from) = filters.from
            && entry.date < from
        {
            return false;
        }
        if let Some(to) = filters.to
            && entry.date > to
        {
            return false;
        }
        if filters.favorites_only && !favorite_dates.contains(&entry.date) {
            return false;
        }
        if filters.conflicts_only && !entry.has_conflict {
            return false;
        }
        if let Some(query) = query_filter.as_deref()
            && !timeline_entry_contains_query(entry, query)
        {
            return false;
        }
        if let Some(tag) = tag_filter.as_deref()
            && !entry
                .metadata
                .tags
                .iter()
                .any(|value| value.eq_ignore_ascii_case(tag))
        {
            return false;
        }
        if let Some(person) = person_filter.as_deref()
            && !entry
                .metadata
                .people
                .iter()
                .any(|value| value.eq_ignore_ascii_case(person))
        {
            return false;
        }
        if let Some(project) = project_filter.as_deref()
            && !entry
                .metadata
                .project
                .as_deref()
                .is_some_and(|value| value.eq_ignore_ascii_case(project))
        {
            return false;
        }
        if let Some(mood) = filters.mood
            && entry.metadata.mood != Some(mood)
        {
            return false;
        }
        if filters.has_tags_only && entry.metadata.tags.is_empty() {
            return false;
        }
        if filters.has_people_only && entry.metadata.people.is_empty() {
            return false;
        }
        if filters.has_project_only && entry.metadata.project.is_none() {
            return false;
        }
        if !filters.weekdays.is_empty() && !filters.weekdays.contains(&entry.date.weekday()) {
            return false;
        }
        true
    });

    entries.sort_by(|left, right| {
        if filters.asc {
            left.date.cmp(&right.date)
        } else {
            right.date.cmp(&left.date)
        }
    });

    if filters.limit > 0 && entries.len() > filters.limit {
        entries.truncate(filters.limit);
    }

    entries
}

fn summarize_review_entries(
    entries: Vec<vault::IndexEntry>,
    today: NaiveDate,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    min_count: usize,
) -> ReviewCliSummary {
    let filtered = entries
        .into_iter()
        .filter(|entry| {
            if let Some(from) = from
                && entry.date < from
            {
                return false;
            }
            if let Some(to) = to
                && entry.date > to
            {
                return false;
            }
            true
        })
        .collect::<Vec<_>>();

    let entry_dates = filtered
        .iter()
        .map(|entry| entry.date)
        .collect::<HashSet<_>>();
    let mut streak_days = 0usize;
    let mut cursor = today;
    while entry_dates.contains(&cursor) {
        streak_days += 1;
        cursor -= chrono::Duration::days(1);
    }

    let entries_this_week = filtered
        .iter()
        .filter(|entry| (today - entry.date).num_days().abs() < 7 && entry.date <= today)
        .count();
    let entries_this_month = filtered
        .iter()
        .filter(|entry| entry.date.year() == today.year() && entry.date.month() == today.month())
        .count();

    let mut on_this_day = filtered
        .iter()
        .filter(|entry| {
            entry.date.month() == today.month()
                && entry.date.day() == today.day()
                && entry.date.year() != today.year()
        })
        .map(|entry| ReviewHitCli {
            date: entry.date.format("%Y-%m-%d").to_string(),
            entry_number: entry.entry_number.clone(),
            preview: entry.preview.clone(),
        })
        .collect::<Vec<_>>();
    on_this_day.sort_by(|left, right| right.date.cmp(&left.date));

    ReviewCliSummary {
        review_date: today.format("%Y-%m-%d").to_string(),
        from: from.map(|value| value.format("%Y-%m-%d").to_string()),
        to: to.map(|value| value.format("%Y-%m-%d").to_string()),
        total_entries: filtered.len(),
        streak_days,
        entries_this_week,
        entries_this_month,
        top_tags: top_counts_from_strings(
            filtered
                .iter()
                .flat_map(|entry| entry.metadata.tags.iter().cloned()),
            min_count,
        ),
        top_people: top_counts_from_strings(
            filtered
                .iter()
                .flat_map(|entry| entry.metadata.people.iter().cloned()),
            min_count,
        ),
        top_projects: top_counts_from_strings(
            filtered
                .iter()
                .filter_map(|entry| entry.metadata.project.clone()),
            min_count,
        ),
        on_this_day,
    }
}

fn compute_review_word_stats(
    documents: &[search::SearchDocument],
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    goal: Option<usize>,
) -> ReviewWordStatsCli {
    let mut total_words = 0usize;
    let mut entries_counted = 0usize;
    let mut words_by_day = BTreeMap::<NaiveDate, usize>::new();

    for document in documents {
        if !search::matches_date_filter(document.date, from, to) {
            continue;
        }
        let words = search::tokenize(&document.body).len();
        total_words += words;
        entries_counted += 1;
        *words_by_day.entry(document.date).or_insert(0) += words;
    }

    let active_days = words_by_day.len();
    let days_meeting_goal = goal
        .map(|goal| {
            words_by_day
                .values()
                .filter(|total| **total >= goal)
                .count()
        })
        .unwrap_or(0);
    let goal_hit_rate = if goal.is_some() && active_days > 0 {
        days_meeting_goal as f64 / active_days as f64
    } else {
        0.0
    };

    ReviewWordStatsCli {
        total_words,
        entries_counted,
        avg_words_per_entry: if entries_counted > 0 {
            total_words as f64 / entries_counted as f64
        } else {
            0.0
        },
        active_days,
        goal,
        days_meeting_goal,
        goal_hit_rate,
    }
}

fn top_counts_from_strings<I>(values: I, min_count: usize) -> Vec<(String, usize)>
where
    I: IntoIterator<Item = String>,
{
    let mut counts = BTreeMap::<String, usize>::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        *counts.entry(trimmed.to_string()).or_default() += 1;
    }
    let mut rows = counts.into_iter().collect::<Vec<_>>();
    let threshold = min_count.max(1);
    rows.retain(|(_, count)| *count >= threshold);
    rows.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    rows
}

fn timeline_row_cli(
    entry: &vault::IndexEntry,
    favorite_dates: &HashSet<NaiveDate>,
    show_metadata: bool,
) -> TimelineRowCli {
    TimelineRowCli {
        date: entry.date.format("%Y-%m-%d").to_string(),
        entry_number: entry.entry_number.clone(),
        favorite: favorite_dates.contains(&entry.date),
        conflict: entry.has_conflict,
        preview: entry.preview.clone(),
        metadata: if show_metadata {
            format_metadata_stamp(&entry.metadata)
        } else {
            None
        },
    }
}

fn build_timeline_summary(
    entries: &[vault::IndexEntry],
    favorite_dates: &HashSet<NaiveDate>,
) -> TimelineSummaryCli {
    let conflicts = entries.iter().filter(|entry| entry.has_conflict).count();
    let favorites = entries
        .iter()
        .filter(|entry| favorite_dates.contains(&entry.date))
        .count();
    let first_date = entries
        .iter()
        .map(|entry| entry.date)
        .min()
        .map(|date| date.format("%Y-%m-%d").to_string());
    let last_date = entries
        .iter()
        .map(|entry| entry.date)
        .max()
        .map(|date| date.format("%Y-%m-%d").to_string());

    let mut mood_counts = BTreeMap::<u8, usize>::new();
    for mood in entries.iter().filter_map(|entry| entry.metadata.mood) {
        *mood_counts.entry(mood).or_default() += 1;
    }

    TimelineSummaryCli {
        total: entries.len(),
        conflicts,
        favorites,
        first_date,
        last_date,
        moods: mood_counts.into_iter().collect(),
    }
}

fn group_timeline_entries(
    entries: &[vault::IndexEntry],
    favorite_dates: &HashSet<NaiveDate>,
    group_by: TimelineGroupByArg,
) -> Vec<TimelineGroupRowCli> {
    let mut grouped = BTreeMap::<String, TimelineGroupRowCli>::new();

    for entry in entries {
        let label = match group_by {
            TimelineGroupByArg::Day => entry.date.format("%Y-%m-%d").to_string(),
            TimelineGroupByArg::Week => {
                let iso = entry.date.iso_week();
                format!("{}-W{:02}", iso.year(), iso.week())
            }
            TimelineGroupByArg::Month => entry.date.format("%Y-%m").to_string(),
        };
        let row = grouped.entry(label.clone()).or_insert(TimelineGroupRowCli {
            group: label,
            entries: 0,
            conflicts: 0,
            favorites: 0,
        });
        row.entries += 1;
        if entry.has_conflict {
            row.conflicts += 1;
        }
        if favorite_dates.contains(&entry.date) {
            row.favorites += 1;
        }
    }

    let mut rows = grouped.into_values().collect::<Vec<_>>();
    rows.sort_by(|left, right| right.group.cmp(&left.group));
    rows
}

fn escape_csv_cell(value: &str) -> String {
    let needs_quotes = value.contains(',') || value.contains('"') || value.contains('\n');
    if !needs_quotes {
        return value.to_string();
    }
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn normalize_filter(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
}

fn timeline_entry_contains_query(entry: &vault::IndexEntry, query: &str) -> bool {
    if entry.preview.to_ascii_lowercase().contains(query) {
        return true;
    }

    if entry
        .metadata
        .tags
        .iter()
        .any(|value| value.to_ascii_lowercase().contains(query))
    {
        return true;
    }

    if entry
        .metadata
        .people
        .iter()
        .any(|value| value.to_ascii_lowercase().contains(query))
    {
        return true;
    }

    if entry
        .metadata
        .project
        .as_deref()
        .is_some_and(|value| value.to_ascii_lowercase().contains(query))
    {
        return true;
    }

    false
}

fn parse_favorite_dates(values: &[String]) -> HashSet<NaiveDate> {
    values
        .iter()
        .filter_map(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
        .collect()
}

fn next_blank_date(start: NaiveDate, existing_dates: &HashSet<NaiveDate>) -> NaiveDate {
    let mut cursor = start;
    while existing_dates.contains(&cursor) {
        cursor += chrono::Duration::days(1);
    }
    cursor
}

fn format_metadata_stamp(metadata: &vault::EntryMetadata) -> Option<String> {
    let mut parts = Vec::new();
    if !metadata.tags.is_empty() {
        parts.push(format!("tags={}", metadata.tags.join(",")));
    }
    if !metadata.people.is_empty() {
        parts.push(format!("people={}", metadata.people.join(",")));
    }
    if let Some(project) = metadata.project.as_deref() {
        parts.push(format!("project={project}"));
    }
    if let Some(mood) = metadata.mood {
        parts.push(format!("mood={mood}"));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" | "))
    }
}

fn print_rank_lines(counts: &[(String, usize)], top: usize) {
    let lines = rank_lines(counts, top);
    if lines.is_empty() {
        println!("  (none yet)");
    } else {
        for line in lines {
            println!("  {line}");
        }
    }
}

fn rank_lines(counts: &[(String, usize)], top: usize) -> Vec<String> {
    counts
        .iter()
        .take(top.max(1))
        .map(|(value, count)| format!("{value}: {count}"))
        .collect()
}

fn prompts_for_category(category: Option<PromptCategoryArg>) -> Vec<PromptTemplate> {
    PROMPT_TEMPLATES
        .iter()
        .copied()
        .filter(|prompt| match category {
            Some(category) => prompt.category == category,
            None => true,
        })
        .collect()
}

fn deterministic_prompt_for_date(
    date: NaiveDate,
    category: Option<PromptCategoryArg>,
) -> Option<PromptTemplate> {
    let prompts = prompts_for_category(category);
    if prompts.is_empty() {
        return None;
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    date.hash(&mut hasher);
    category.hash(&mut hasher);
    let seed = hasher.finish() as usize;
    let index = seed % prompts.len();
    prompts.get(index).copied()
}

fn prompt_category_label(category: PromptCategoryArg) -> &'static str {
    match category {
        PromptCategoryArg::Reflection => "reflection",
        PromptCategoryArg::Gratitude => "gratitude",
        PromptCategoryArg::Growth => "growth",
        PromptCategoryArg::Focus => "focus",
        PromptCategoryArg::Relationships => "relationships",
    }
}

fn run_cli_export(
    date: NaiveDate,
    format: ExportFormatArg,
    output: Option<&str>,
) -> Result<(), String> {
    log::info!("running CLI export");
    let config = config::AppConfig::load_or_default();
    let vault = unlock_cli_vault(&config)?;
    let Some(entry) = vault
        .export_entry(date)
        .map_err(|error| format!("export failed: {error}"))?
    else {
        return Err(format!("no saved entry for {}", date.format("%Y-%m-%d")));
    };

    let rendered = match format {
        ExportFormatArg::Text => {
            vault::format_export_text(&entry.body, entry.closing_thought.as_deref())
        }
        ExportFormatArg::Markdown => render_markdown_export(&entry),
    };

    if let Some(output) = output {
        let path = config::expand_path_like(output);
        write_plaintext_output(&path, &rendered)?;
        println!("Wrote: {}", path.display());
    } else {
        println!("{rendered}");
    }

    Ok(())
}

fn run_cli_sync(
    backend_arg: Option<SyncBackendArg>,
    remote_arg: Option<&str>,
) -> Result<(), String> {
    log::info!("running CLI sync");
    let mut config = config::AppConfig::load_or_default();
    let vault = unlock_cli_vault(&config)?;
    let backend_kind = resolve_sync_backend_kind(backend_arg, remote_arg)?;

    let report = match backend_kind {
        SyncBackendArg::Folder => {
            let remote_root = resolve_folder_sync_target_path(&mut config, remote_arg)?;
            vault
                .sync_folder(&remote_root)
                .map_err(|error| format!("sync failed: {error}"))?
        }
        SyncBackendArg::S3 => {
            let mut backend = sync::S3Backend::from_remote(remote_arg)?;
            vault
                .sync_with_backend(&mut backend)
                .map_err(|error| format!("sync failed: {error}"))?
        }
        SyncBackendArg::Webdav => {
            let mut backend = sync::WebDavBackend::from_remote(remote_arg)?;
            vault
                .sync_with_backend(&mut backend)
                .map_err(|error| format!("sync failed: {error}"))?
        }
    };

    println!("Pulled: {}", report.pulled);
    println!("Pushed: {}", report.pushed);
    if report.conflicts.is_empty() {
        println!("Conflicts: none");
    } else {
        println!("Conflicts:");
        for date in report.conflicts {
            println!("  {}", date.format("%Y-%m-%d"));
        }
    }

    Ok(())
}

fn run_cli_verify() -> Result<(), String> {
    log::info!("running CLI verify");
    let config = config::AppConfig::load_or_default();
    let vault = unlock_cli_vault(&config)?;
    let report = vault
        .verify_integrity()
        .map_err(|error| format!("verify failed: {error}"))?;

    if report.ok {
        println!("OK");
    } else {
        println!("BROKEN");
        for issue in report.issues {
            match issue.date {
                Some(date) => println!("{}  {}", date.format("%Y-%m-%d"), issue.message),
                None => println!("{}", issue.message),
            }
        }
    }
    Ok(())
}

fn run_cli_backup(command: Option<BackupCommand>) -> Result<(), String> {
    log::info!("running CLI backup");
    let config = config::AppConfig::load_or_default();
    let vault = unlock_cli_vault(&config)?;

    match command {
        None => {
            let summary = vault
                .create_backup(&config.backup_retention)
                .map_err(|error| format!("backup failed: {error}"))?;
            println!("Backup: {}", summary.path.display());
            println!("Pruned: {}", summary.pruned);
        }
        Some(BackupCommand::List) => {
            let backups = vault
                .list_backups()
                .map_err(|error| format!("backup list failed: {error}"))?;
            if backups.is_empty() {
                println!("No backups.");
            } else {
                for backup in backups {
                    println!(
                        "{}  {:>10} bytes  {}",
                        backup.created_at.format("%Y-%m-%d %H:%M:%SZ"),
                        backup.size_bytes,
                        backup.path.display()
                    );
                }
            }
        }
        Some(BackupCommand::Prune { apply }) => {
            let pruned = if apply {
                vault
                    .prune_backups_now(&config.backup_retention)
                    .map_err(|error| format!("backup prune failed: {error}"))?
            } else {
                vault
                    .preview_backup_prune(&config.backup_retention)
                    .map_err(|error| format!("backup prune preview failed: {error}"))?
            };
            if pruned.is_empty() {
                println!("No backups to prune.");
            } else {
                println!(
                    "{} {} backup(s):",
                    if apply { "Pruned" } else { "Would prune" },
                    pruned.len()
                );
                for backup in pruned {
                    println!(
                        "  {}  {}",
                        backup.created_at.format("%Y-%m-%d %H:%M:%SZ"),
                        backup.path.display()
                    );
                }
                if !apply {
                    println!("Dry run only. Re-run with --apply to delete these backups.");
                }
            }
        }
    }

    Ok(())
}

fn run_cli_restore(backup: &str, into: &str) -> Result<(), String> {
    log::info!("running CLI restore");
    let config = config::AppConfig::load_or_default();
    let vault = unlock_cli_vault(&config)?;
    let backup_path = config::expand_path_like(backup);
    let restore_root = config::expand_path_like(into);
    vault
        .restore_backup_into(&backup_path, &restore_root)
        .map_err(|error| format!("restore failed: {error}"))?;
    println!("Restored: {}", restore_root.display());
    Ok(())
}

fn run_cli_settings(json_output: bool, command: Option<SettingsCommand>) -> Result<(), String> {
    log::info!("running CLI settings");
    let state = load_config_state();

    match command {
        None => render_settings_report(&state, json_output),
        Some(SettingsCommand::Init { force }) => {
            if json_output {
                return Err("--json cannot be combined with settings init".to_string());
            }
            run_cli_settings_init(state, force)
        }
        Some(SettingsCommand::Get { key }) => {
            if json_output {
                return Err("--json cannot be combined with settings get".to_string());
            }
            if let Some(error) = state.error.as_deref() {
                return Err(format!(
                    "config is invalid: {error}. Fix it or run `bsj settings init --force`."
                ));
            }
            println!("{}", config::get_setting_value(&state.config, &key)?);
            Ok(())
        }
        Some(SettingsCommand::Set { key, value }) => {
            if json_output {
                return Err("--json cannot be combined with settings set".to_string());
            }
            if state.exists && state.error.is_some() {
                return Err(
                    "config is invalid. Fix it or run `bsj settings init --force` before using settings set."
                        .to_string(),
                );
            }
            let mut config = state.config.clone();
            let updated = config::set_setting_value(&mut config, &key, &value)?;
            config
                .save()
                .map_err(|error| format!("failed to save config: {error}"))?;
            println!("{key}={updated}");
            Ok(())
        }
    }
}

fn render_settings_report(state: &LoadedConfigState, json_output: bool) -> Result<(), String> {
    let log_path = logging::log_file_path();
    let env = help::EnvironmentSettings::capture();
    let (vault_metadata, vault_metadata_error) = load_vault_metadata_state(&state.config);

    if let Some(error) = state.error.as_deref()
        && json_output
    {
        let document = serde_json::json!({
            "config_error": error,
            "paths": {
                "config_file": state.path.display().to_string(),
                "config_file_exists": state.exists,
                "log_file": log_path.display().to_string(),
            }
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&document)
                .map_err(|error| format!("failed to serialize settings JSON: {error}"))?
        );
        return Ok(());
    }

    if let Some(error) = state.error.as_deref() {
        return Err(format!(
            "config is invalid: {error}. Fix it or run `bsj settings init --force`."
        ));
    }

    if json_output {
        let document = help::render_settings_json(
            &state.path,
            state.exists,
            &state.config,
            &log_path,
            &env,
            vault_metadata.as_ref(),
        );
        println!(
            "{}",
            serde_json::to_string_pretty(&document)
                .map_err(|error| format!("failed to serialize settings JSON: {error}"))?
        );
    } else {
        let mut report = help::render_settings_report(
            &state.path,
            state.exists,
            &state.config,
            &log_path,
            &env,
            vault_metadata.as_ref(),
        );
        if let Some(error) = vault_metadata_error {
            report.push_str("\nWarnings\n");
            report.push_str(&format!("  vault_metadata_error   {error}\n"));
        }
        println!("{report}");
    }
    Ok(())
}

fn run_cli_settings_init(state: LoadedConfigState, force: bool) -> Result<(), String> {
    if state.exists && state.error.is_none() && !force {
        return Err(format!(
            "config already exists at {}. Re-run with --force to overwrite.",
            state.path.display()
        ));
    }
    if state.exists && state.error.is_some() && !force {
        return Err(format!(
            "config at {} is invalid. Re-run with --force to replace it.",
            state.path.display()
        ));
    }

    let config = if force && state.error.is_some() {
        config::AppConfig::load_or_default()
    } else {
        state.config
    };
    config
        .save()
        .map_err(|error| format!("failed to write config: {error}"))?;
    println!("Initialized: {}", state.path.display());
    Ok(())
}

fn run_cli_guide(topic: GuideTopicArg) -> Result<(), String> {
    log::info!("printing guide");
    let config_path = config::config_file_path()
        .unwrap_or_else(|_| fallback_config_path("bsj").join("config.json"));
    let default_vault_path = config::default_vault_path();
    let log_path = logging::log_file_path();

    let output = match topic {
        GuideTopicArg::Docs => help::render_docs_hub(),
        GuideTopicArg::Quickstart => help::render_quickstart_guide(),
        GuideTopicArg::Troubleshooting => help::render_troubleshooting_guide(),
        GuideTopicArg::Sync => help::render_sync_guide(),
        GuideTopicArg::Backup => help::render_backup_restore_guide(),
        GuideTopicArg::Macros => help::render_macro_guide(),
        GuideTopicArg::Terminal => help::render_terminal_guide(),
        GuideTopicArg::Privacy => help::render_privacy_guide(),
        GuideTopicArg::Setup => {
            help::render_setup_guide(&config_path, &default_vault_path, &log_path)
        }
        GuideTopicArg::Product => help::render_product_guide(),
        GuideTopicArg::Datasheet => help::render_datasheet(),
        GuideTopicArg::Faq => help::render_faq(),
        GuideTopicArg::Support => help::render_support(),
        GuideTopicArg::Settings => {
            help::render_settings_guide(&config_path, &default_vault_path, &log_path)
        }
        GuideTopicArg::Distribution => help::render_distribution_guide(),
    };

    println!("{output}");
    Ok(())
}

fn run_cli_doctor(unlock: bool, json_output: bool) -> Result<bool, String> {
    log::info!("running CLI doctor");
    let state = load_config_state();
    let log_path = logging::log_file_path();
    let env = help::EnvironmentSettings::capture();
    let (vault_metadata, vault_metadata_error) = load_vault_metadata_state(&state.config);
    let vault_exists = vault::vault_exists(&state.config.vault_path);

    let mut integrity_report = None;
    let mut unlock_error = None;
    let mut entry_count = None;
    let mut backup_count = None;
    let mut conflict_count = None;

    if unlock && vault_exists && state.error.is_none() && vault_metadata_error.is_none() {
        match unlock_cli_vault(&state.config) {
            Ok(vault) => {
                match vault.verify_integrity() {
                    Ok(report) => integrity_report = Some(report),
                    Err(error) => unlock_error = Some(format!("integrity check failed: {error}")),
                }

                if unlock_error.is_none() {
                    entry_count = Some(
                        vault
                            .list_entry_dates()
                            .map_err(|error| format!("failed to count entries: {error}"))?
                            .len(),
                    );
                    backup_count = Some(
                        vault
                            .list_backups()
                            .map_err(|error| format!("failed to count backups: {error}"))?
                            .len(),
                    );
                    conflict_count = Some(
                        vault
                            .list_conflicted_dates()
                            .map_err(|error| format!("failed to count conflicts: {error}"))?
                            .len(),
                    );
                }
            }
            Err(error) => unlock_error = Some(error),
        }
    }

    let report = doctor::build_report(doctor::DoctorInputs {
        config_path: &state.path,
        config_exists: state.exists,
        config_error: state.error.as_deref(),
        config: &state.config,
        log_path: &log_path,
        env: &env,
        vault_exists,
        vault_metadata: vault_metadata.as_ref(),
        vault_metadata_error: vault_metadata_error.as_deref(),
        integrity_report: integrity_report.as_ref(),
        unlock_error: unlock_error.as_deref(),
        entry_count,
        backup_count,
        conflict_count,
    });

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&report)
                .map_err(|error| format!("failed to serialize doctor JSON: {error}"))?
        );
    } else {
        println!("{}", doctor::render_text(&report));
    }

    Ok(report.ok)
}

fn run_cli_sysop(command: SysopCommand) -> Result<(), String> {
    log::info!("running CLI sysop");
    let state = load_config_state();
    let env = help::EnvironmentSettings::capture();
    let log_path = logging::log_file_path();
    let vault_exists = vault::vault_exists(&state.config.vault_path);

    match command {
        SysopCommand::Dashboard { json } => {
            let layout = if vault_exists {
                Some(sysop::analyze_vault_layout(&state.config.vault_path)?)
            } else {
                None
            };
            let permissions = if vault_exists {
                Some(sysop::collect_permission_issues(&state.config.vault_path)?.len())
            } else {
                None
            };
            let orphans = if vault_exists {
                Some(sysop::collect_orphan_files(&state.config.vault_path)?.len())
            } else {
                None
            };
            let revisions = if vault_exists {
                Some(sysop::collect_revision_stats(&state.config.vault_path)?)
            } else {
                None
            };
            let stale_drafts = if vault_exists {
                Some(sysop::collect_stale_drafts(
                    &state.config.vault_path,
                    7,
                    chrono::Utc::now(),
                )?)
            } else {
                None
            };

            let mut integrity_ok = None;
            let mut integrity_issues = None;
            let mut conflict_count = None;
            let mut cache_valid = None;
            let mut backup_count = None;
            let mut unlock_error = None;
            if vault_exists && state.error.is_none() {
                match unlock_cli_vault(&state.config) {
                    Ok(vault) => {
                        match vault.verify_integrity() {
                            Ok(report) => {
                                integrity_ok = Some(report.ok);
                                integrity_issues = Some(report.issues.len());
                            }
                            Err(error) => unlock_error = Some(error.to_string()),
                        }
                        if unlock_error.is_none() {
                            conflict_count = Some(
                                vault
                                    .list_conflicted_dates()
                                    .map_err(|error| format!("conflict scan failed: {error}"))?
                                    .len(),
                            );
                            backup_count = Some(
                                vault
                                    .list_backups()
                                    .map_err(|error| format!("backup scan failed: {error}"))?
                                    .len(),
                            );
                            cache_valid = Some(vault.search_cache_status().valid);
                        }
                    }
                    Err(error) => unlock_error = Some(error),
                }
            }

            let top_revision = revisions
                .as_ref()
                .and_then(|stats| stats.first())
                .map(|stat| {
                    serde_json::json!({
                        "date": stat.date.format("%Y-%m-%d").to_string(),
                        "revisions": stat.revisions,
                        "drafts": stat.drafts,
                    })
                });

            let dashboard = serde_json::json!({
                "config_ok": state.error.is_none(),
                "vault_exists": vault_exists,
                "config_path": state.path.display().to_string(),
                "vault_path": state.config.vault_path.display().to_string(),
                "log_path": log_path.display().to_string(),
                "sync_target_path": state.config.sync_target_path.as_ref().map(|path| path.display().to_string()),
                "layout": layout,
                "permission_issue_count": permissions,
                "orphan_file_count": orphans,
                "revision_date_count": revisions.as_ref().map(|stats| stats.len()),
                "stale_draft_count": stale_drafts.as_ref().map(|drafts| drafts.len()),
                "top_revision_day": top_revision,
                "integrity_ok": integrity_ok,
                "integrity_issue_count": integrity_issues,
                "conflict_count": conflict_count,
                "backup_count": backup_count,
                "search_cache_valid": cache_valid,
                "unlock_error": unlock_error,
                "last_sync": state.config.last_sync,
            });

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&dashboard)
                        .map_err(|error| format!("failed to serialize dashboard JSON: {error}"))?
                );
            } else {
                println!("BlueScreen Journal SYSOP Dashboard");
                println!();
                println!(
                    "Config: {}",
                    if state.error.is_none() { "OK" } else { "ERROR" }
                );
                println!(
                    "Vault: {}",
                    if vault_exists { "PRESENT" } else { "MISSING" }
                );
                println!("Path: {}", state.config.vault_path.display());
                if let Some(layout) = &layout {
                    println!(
                        "Layout: revisions={} drafts={} invalid_files={}",
                        layout.revision_files,
                        layout.draft_files,
                        layout.invalid_files.len()
                    );
                }
                if let Some(count) = permissions {
                    println!("Permissions: {count} issue(s)");
                }
                if let Some(count) = orphans {
                    println!("Orphans: {count}");
                }
                if let Some(ok) = integrity_ok {
                    println!("Integrity: {}", if ok { "OK" } else { "BROKEN" });
                }
                if let Some(count) = integrity_issues {
                    println!("Integrity issues: {count}");
                }
                if let Some(count) = conflict_count {
                    println!("Conflicts: {count}");
                }
                if let Some(count) = backup_count {
                    println!("Backups: {count}");
                }
                if let Some(valid) = cache_valid {
                    println!("Search cache: {}", if valid { "VALID" } else { "INVALID" });
                }
                if let Some(error) = unlock_error {
                    println!("Unlock error: {error}");
                }
                if let Some(last_sync) = &state.config.last_sync {
                    println!(
                        "Last sync: {} {} pulled={} pushed={} conflicts={}",
                        last_sync.timestamp,
                        last_sync.backend,
                        last_sync.pulled,
                        last_sync.pushed,
                        last_sync.conflicts
                    );
                }
            }
        }
        SysopCommand::Runbook => {
            let mut actions = Vec::new();
            if state.error.is_some() {
                actions.push("Fix config first: `bsj settings init --force`".to_string());
            }
            if !vault_exists {
                actions.push("Initialize vault from the TUI setup wizard (`bsj`)".to_string());
            }
            if state.config.sync_target_path.is_none() {
                actions.push(
                    "Set a default sync target: `bsj settings set sync_target_path <PATH>`"
                        .to_string(),
                );
            }

            if vault_exists && state.error.is_none() {
                match unlock_cli_vault(&state.config) {
                    Ok(vault) => {
                        let integrity = vault
                            .verify_integrity()
                            .map_err(|error| format!("integrity check failed: {error}"))?;
                        if !integrity.ok {
                            actions.push(format!(
                                "Resolve integrity issues (`bsj verify`) count={}",
                                integrity.issues.len()
                            ));
                        }

                        let conflicts = vault
                            .list_conflicted_dates()
                            .map_err(|error| format!("conflict scan failed: {error}"))?;
                        if !conflicts.is_empty() {
                            actions.push(format!(
                                "Resolve {} conflicted date(s) from INDEX/Merge view",
                                conflicts.len()
                            ));
                        }

                        let backups = vault
                            .list_backups()
                            .map_err(|error| format!("backup scan failed: {error}"))?;
                        if backups.is_empty() {
                            actions.push(
                                "Create first encrypted backup now: `bsj backup`".to_string(),
                            );
                        }

                        let stale = sysop::collect_stale_drafts(
                            &state.config.vault_path,
                            7,
                            chrono::Utc::now(),
                        )?;
                        if !stale.is_empty() {
                            actions.push(format!(
                                "Review {} stale draft(s): `bsj sysop drafts --older-than-days 7`",
                                stale.len()
                            ));
                        }

                        let orphans = sysop::collect_orphan_files(&state.config.vault_path)?;
                        if !orphans.is_empty() {
                            actions.push(format!(
                                "Audit {} orphan file(s): `bsj sysop orphans`",
                                orphans.len()
                            ));
                        }
                    }
                    Err(error) => {
                        actions.push(format!(
                            "Unlock vault failed ({error}). Verify passphrase and rerun SYSOP checks."
                        ));
                    }
                }
            }

            if actions.is_empty() {
                println!("SYSOP Runbook: all green. No urgent operator actions detected.");
            } else {
                println!("SYSOP Runbook");
                println!();
                for (index, action) in actions.iter().enumerate() {
                    println!("{}. {}", index + 1, action);
                }
            }
        }
        SysopCommand::Env { json } => {
            let document = serde_json::json!({
                "BSJ_PASSPHRASE": env.passphrase_set,
                "BSJ_SYNC_BACKEND": env.sync_backend,
                "BSJ_S3_BUCKET": env.s3_bucket_set,
                "BSJ_S3_PREFIX": env.s3_prefix_set,
                "AWS_REGION": env.aws_region_set,
                "BSJ_WEBDAV_URL": env.webdav_url_set,
                "BSJ_WEBDAV_USERNAME": env.webdav_username_set,
                "BSJ_WEBDAV_PASSWORD": env.webdav_password_set,
            });
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&document)
                        .map_err(|error| format!("failed to serialize env JSON: {error}"))?
                );
            } else {
                println!("SYSOP Environment");
                println!("BSJ_PASSPHRASE      {}", on_off(env.passphrase_set));
                println!(
                    "BSJ_SYNC_BACKEND    {}",
                    env.sync_backend.unwrap_or_else(|| "unset".to_string())
                );
                println!("BSJ_S3_BUCKET       {}", on_off(env.s3_bucket_set));
                println!("BSJ_S3_PREFIX       {}", on_off(env.s3_prefix_set));
                println!("AWS_REGION          {}", on_off(env.aws_region_set));
                println!("BSJ_WEBDAV_URL      {}", on_off(env.webdav_url_set));
                println!("BSJ_WEBDAV_USERNAME {}", on_off(env.webdav_username_set));
                println!("BSJ_WEBDAV_PASSWORD {}", on_off(env.webdav_password_set));
            }
        }
        SysopCommand::Paths { json } => {
            let document = serde_json::json!({
                "config_path": state.path.display().to_string(),
                "config_exists": state.exists,
                "vault_path": state.config.vault_path.display().to_string(),
                "vault_exists": vault_exists,
                "log_path": log_path.display().to_string(),
                "log_dir_exists": log_path.parent().is_some_and(Path::exists),
                "sync_target_path": state.config.sync_target_path.as_ref().map(|path| path.display().to_string()),
                "sync_target_exists": state.config.sync_target_path.as_ref().is_some_and(|path| path.exists()),
            });
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&document)
                        .map_err(|error| format!("failed to serialize paths JSON: {error}"))?
                );
            } else {
                println!("SYSOP Paths");
                println!(
                    "Config: {} ({})",
                    state.path.display(),
                    on_off(state.exists)
                );
                println!(
                    "Vault : {} ({})",
                    state.config.vault_path.display(),
                    on_off(vault_exists)
                );
                println!("Log   : {}", log_path.display());
                if let Some(sync) = &state.config.sync_target_path {
                    println!("Sync  : {} ({})", sync.display(), on_off(sync.exists()));
                } else {
                    println!("Sync  : unset");
                }
            }
        }
        SysopCommand::Permissions { json, limit } => {
            let issues = if vault_exists {
                sysop::collect_permission_issues(&state.config.vault_path)?
            } else {
                Vec::new()
            };
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&issues).map_err(|error| format!(
                        "failed to serialize permissions JSON: {error}"
                    ))?
                );
            } else if !vault_exists {
                println!("Vault not found at {}", state.config.vault_path.display());
            } else if issues.is_empty() {
                println!("Permissions audit: OK");
            } else {
                println!("Permissions audit: {} issue(s)", issues.len());
                for issue in issues.iter().take(limit.max(1)) {
                    println!("  {}  {}", issue.path.display(), issue.issue);
                }
                if issues.len() > limit.max(1) {
                    println!("  ... {} more", issues.len() - limit.max(1));
                }
            }
        }
        SysopCommand::VaultLayout { json } => {
            let report = if vault_exists {
                Some(sysop::analyze_vault_layout(&state.config.vault_path)?)
            } else {
                None
            };
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .map_err(|error| format!("failed to serialize layout JSON: {error}"))?
                );
            } else if let Some(report) = report {
                println!("SYSOP Vault Layout");
                println!("vault.json  {}", on_off(report.vault_json_present));
                println!("entries/    {}", on_off(report.entries_dir_present));
                println!("devices/    {}", on_off(report.devices_dir_present));
                println!("backups/    {}", on_off(report.backups_dir_present));
                println!(".cache      {}", on_off(report.cache_file_present));
                println!("revisions   {}", report.revision_files);
                println!("drafts      {}", report.draft_files);
                println!("invalid     {}", report.invalid_files.len());
            } else {
                println!("Vault not found at {}", state.config.vault_path.display());
            }
        }
        SysopCommand::Orphans { json, limit } => {
            let orphans = if vault_exists {
                sysop::collect_orphan_files(&state.config.vault_path)?
            } else {
                Vec::new()
            };
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&orphans)
                        .map_err(|error| format!("failed to serialize orphan JSON: {error}"))?
                );
            } else if !vault_exists {
                println!("Vault not found at {}", state.config.vault_path.display());
            } else if orphans.is_empty() {
                println!("Orphan scan: clean");
            } else {
                println!("Orphan files: {}", orphans.len());
                for path in orphans.iter().take(limit.max(1)) {
                    println!("  {}", path.display());
                }
                if orphans.len() > limit.max(1) {
                    println!("  ... {} more", orphans.len() - limit.max(1));
                }
            }
        }
        SysopCommand::Revisions { top, json } => {
            let stats = if vault_exists {
                sysop::collect_revision_stats(&state.config.vault_path)?
            } else {
                Vec::new()
            };
            let output = if top == 0 {
                stats.clone()
            } else {
                stats.into_iter().take(top).collect::<Vec<_>>()
            };
            if json {
                let document = output
                    .iter()
                    .map(|stat| {
                        serde_json::json!({
                            "date": stat.date.format("%Y-%m-%d").to_string(),
                            "revisions": stat.revisions,
                            "drafts": stat.drafts,
                        })
                    })
                    .collect::<Vec<_>>();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&document)
                        .map_err(|error| format!("failed to serialize revision JSON: {error}"))?
                );
            } else if output.is_empty() {
                println!("No revisions found.");
            } else {
                println!("SYSOP Revisions");
                for stat in output {
                    println!(
                        "{}  revisions={} drafts={}",
                        stat.date.format("%Y-%m-%d"),
                        stat.revisions,
                        stat.drafts
                    );
                }
            }
        }
        SysopCommand::Drafts {
            older_than_days,
            json,
            limit,
        } => {
            let drafts = if vault_exists {
                sysop::collect_stale_drafts(
                    &state.config.vault_path,
                    older_than_days,
                    chrono::Utc::now(),
                )?
            } else {
                Vec::new()
            };
            if json {
                let document = drafts
                    .iter()
                    .map(|draft| {
                        serde_json::json!({
                            "date": draft.date.format("%Y-%m-%d").to_string(),
                            "path": draft.path.display().to_string(),
                            "age_days": draft.age_days,
                        })
                    })
                    .collect::<Vec<_>>();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&document)
                        .map_err(|error| format!("failed to serialize drafts JSON: {error}"))?
                );
            } else if drafts.is_empty() {
                println!("No stale drafts found.");
            } else {
                println!("Stale drafts: {}", drafts.len());
                for draft in drafts.iter().take(limit.max(1)) {
                    println!(
                        "{}  age={}d  {}",
                        draft.date.format("%Y-%m-%d"),
                        draft.age_days,
                        draft.path.display()
                    );
                }
                if drafts.len() > limit.max(1) {
                    println!("  ... {} more", drafts.len() - limit.max(1));
                }
            }
        }
        SysopCommand::Conflicts { json } => {
            if !vault_exists {
                return Err(format!(
                    "vault not found at {}",
                    state.config.vault_path.display()
                ));
            }
            let vault = unlock_cli_vault(&state.config)?;
            let conflicts = vault
                .list_conflicted_dates()
                .map_err(|error| format!("conflict scan failed: {error}"))?;
            if json {
                let dates = conflicts
                    .iter()
                    .map(|date| date.format("%Y-%m-%d").to_string())
                    .collect::<Vec<_>>();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&dates)
                        .map_err(|error| format!("failed to serialize conflicts JSON: {error}"))?
                );
            } else if conflicts.is_empty() {
                println!("No conflicts.");
            } else {
                println!("Conflicted dates:");
                for date in conflicts {
                    println!("  {}", date.format("%Y-%m-%d"));
                }
            }
        }
        SysopCommand::Cache { json } => {
            if !vault_exists {
                return Err(format!(
                    "vault not found at {}",
                    state.config.vault_path.display()
                ));
            }
            let vault = unlock_cli_vault(&state.config)?;
            let status = vault.search_cache_status();
            if json {
                let document = serde_json::json!({
                    "path": status.path.display().to_string(),
                    "exists": status.exists,
                    "size_bytes": status.size_bytes,
                    "modified_at": status.modified_at.map(|value| value.to_rfc3339()),
                    "entry_count": status.entry_count,
                    "valid": status.valid,
                    "issue": status.issue,
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&document)
                        .map_err(|error| format!("failed to serialize cache JSON: {error}"))?
                );
            } else {
                println!("SYSOP Cache");
                println!("Path   : {}", status.path.display());
                println!("Exists : {}", on_off(status.exists));
                println!("Valid  : {}", on_off(status.valid));
                println!("Size   : {} bytes", status.size_bytes);
                if let Some(entry_count) = status.entry_count {
                    println!("Entries: {entry_count}");
                }
                if let Some(issue) = status.issue {
                    println!("Issue  : {issue}");
                }
            }
        }
        SysopCommand::Backups { json } => {
            if !vault_exists {
                return Err(format!(
                    "vault not found at {}",
                    state.config.vault_path.display()
                ));
            }
            let vault = unlock_cli_vault(&state.config)?;
            let backups = vault
                .list_backups()
                .map_err(|error| format!("backup list failed: {error}"))?;
            let prune_preview = vault
                .preview_backup_prune(&state.config.backup_retention)
                .map_err(|error| format!("backup prune preview failed: {error}"))?;
            let total_bytes = backups.iter().map(|entry| entry.size_bytes).sum::<u64>();
            let document = serde_json::json!({
                "count": backups.len(),
                "total_bytes": total_bytes,
                "newest": backups.first().map(|entry| entry.path.display().to_string()),
                "oldest": backups.last().map(|entry| entry.path.display().to_string()),
                "prune_preview_count": prune_preview.len(),
                "retention": state.config.backup_retention,
            });
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&document)
                        .map_err(|error| format!("failed to serialize backup JSON: {error}"))?
                );
            } else {
                println!("SYSOP Backups");
                println!("Count : {}", backups.len());
                println!("Size  : {} bytes", total_bytes);
                println!(
                    "Prune : {} candidate(s) with current retention",
                    prune_preview.len()
                );
                println!(
                    "Policy: daily={} weekly={} monthly={}",
                    state.config.backup_retention.daily,
                    state.config.backup_retention.weekly,
                    state.config.backup_retention.monthly
                );
            }
        }
        SysopCommand::Integrity { json } => {
            if !vault_exists {
                return Err(format!(
                    "vault not found at {}",
                    state.config.vault_path.display()
                ));
            }
            let vault = unlock_cli_vault(&state.config)?;
            let report = vault
                .verify_integrity()
                .map_err(|error| format!("integrity check failed: {error}"))?;
            if json {
                let issues = report
                    .issues
                    .iter()
                    .map(|issue| {
                        serde_json::json!({
                            "date": issue.date.map(|value| value.format("%Y-%m-%d").to_string()),
                            "message": issue.message,
                        })
                    })
                    .collect::<Vec<_>>();
                let document = serde_json::json!({
                    "ok": report.ok,
                    "issue_count": issues.len(),
                    "issues": issues,
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&document)
                        .map_err(|error| format!("failed to serialize integrity JSON: {error}"))?
                );
            } else if report.ok {
                println!("Integrity: OK");
            } else {
                println!("Integrity: BROKEN");
                for issue in report.issues {
                    if let Some(date) = issue.date {
                        println!("  {}  {}", date.format("%Y-%m-%d"), issue.message);
                    } else {
                        println!("  {}", issue.message);
                    }
                }
            }
        }
        SysopCommand::Activity { days, json } => {
            let stats = if vault_exists {
                sysop::collect_revision_stats(&state.config.vault_path)?
            } else {
                Vec::new()
            };
            let today = Local::now().date_naive();
            let series = sysop::build_activity_series(&stats, today, days);
            if json {
                let document = series
                    .iter()
                    .map(|point| {
                        serde_json::json!({
                            "date": point.date.format("%Y-%m-%d").to_string(),
                            "revisions": point.revisions,
                        })
                    })
                    .collect::<Vec<_>>();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&document)
                        .map_err(|error| format!("failed to serialize activity JSON: {error}"))?
                );
            } else if series.is_empty() {
                println!("No activity points.");
            } else {
                println!("SYSOP Activity (last {} days)", series.len());
                for point in series {
                    println!("{}  {}", point.date.format("%Y-%m-%d"), point.revisions);
                }
            }
        }
        SysopCommand::SyncPreview {
            backend,
            remote,
            json,
        } => {
            if !vault_exists {
                return Err(format!(
                    "vault not found at {}",
                    state.config.vault_path.display()
                ));
            }
            let mut config = state.config;
            let vault = unlock_cli_vault(&config)?;
            let backend_kind = resolve_sync_backend_kind(backend, remote.as_deref())?;
            let preview = match backend_kind {
                SyncBackendArg::Folder => {
                    let remote_root =
                        resolve_folder_sync_target_path(&mut config, remote.as_deref())?;
                    let mut backend = sync::FolderBackend::new(remote_root);
                    sync::preview_root(vault.metadata(), &config.vault_path, &mut backend)
                        .map_err(|error| format!("sync preview failed: {error}"))?
                }
                SyncBackendArg::S3 => {
                    let mut backend = sync::S3Backend::from_remote(remote.as_deref())?;
                    sync::preview_root(vault.metadata(), &config.vault_path, &mut backend)
                        .map_err(|error| format!("sync preview failed: {error}"))?
                }
                SyncBackendArg::Webdav => {
                    let mut backend = sync::WebDavBackend::from_remote(remote.as_deref())?;
                    sync::preview_root(vault.metadata(), &config.vault_path, &mut backend)
                        .map_err(|error| format!("sync preview failed: {error}"))?
                }
            };
            if json {
                let document = serde_json::json!({
                    "local_revisions": preview.local_revisions,
                    "remote_revisions": preview.remote_revisions,
                    "local_only_revisions": preview.local_only_revisions,
                    "remote_only_revisions": preview.remote_only_revisions,
                    "shared_revisions": preview.shared_revisions,
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&document).map_err(|error| format!(
                        "failed to serialize sync preview JSON: {error}"
                    ))?
                );
            } else {
                println!("SYSOP Sync Preview");
                println!("Local revisions : {}", preview.local_revisions);
                println!("Remote revisions: {}", preview.remote_revisions);
                println!("Local only      : {}", preview.local_only_revisions);
                println!("Remote only     : {}", preview.remote_only_revisions);
                println!("Shared          : {}", preview.shared_revisions);
            }
        }
    }

    Ok(())
}

fn on_off(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn run_cli_completions(shell: CompletionShellArg) -> Result<(), String> {
    log::info!("generating completions");
    let mut command = Cli::command();
    let shell = match shell {
        CompletionShellArg::Bash => Shell::Bash,
        CompletionShellArg::Fish => Shell::Fish,
        CompletionShellArg::Zsh => Shell::Zsh,
    };
    let mut stdout = io::stdout();
    generate(shell, &mut command, "bsj", &mut stdout);
    Ok(())
}

fn unlock_cli_vault(config: &config::AppConfig) -> Result<vault::UnlockedVault, String> {
    if !vault::vault_exists(&config.vault_path) {
        return Err(format!(
            "vault not found at {}",
            config.vault_path.display()
        ));
    }

    let secret = read_cli_secret()?;
    vault::unlock_vault(&config.vault_path, &secret)
        .map_err(|error| format!("unlock failed: {error}"))
}

fn read_cli_secret() -> Result<SecretString, String> {
    let passphrase = match env::var("BSJ_PASSPHRASE") {
        Ok(passphrase) => passphrase,
        Err(_) => rpassword::prompt_password("Vault passphrase: ")
            .map_err(|error| format!("failed to read passphrase: {error}"))?,
    };
    Ok(SecretString::new(passphrase.into_boxed_str()))
}

fn resolve_sync_backend_kind(
    backend_arg: Option<SyncBackendArg>,
    remote_arg: Option<&str>,
) -> Result<SyncBackendArg, String> {
    if let Some(backend_arg) = backend_arg {
        return Ok(backend_arg);
    }

    if let Ok(value) = env::var("BSJ_SYNC_BACKEND") {
        return match value.to_ascii_lowercase().as_str() {
            "folder" => Ok(SyncBackendArg::Folder),
            "s3" => Ok(SyncBackendArg::S3),
            "webdav" => Ok(SyncBackendArg::Webdav),
            other => Err(format!(
                "invalid BSJ_SYNC_BACKEND '{other}'; expected folder, s3, or webdav"
            )),
        };
    }

    if let Some(remote_arg) = remote_arg {
        if sync::looks_like_s3_remote(remote_arg) {
            return Ok(SyncBackendArg::S3);
        }
        if sync::looks_like_webdav_remote(remote_arg) {
            return Ok(SyncBackendArg::Webdav);
        }
    }

    Ok(SyncBackendArg::Folder)
}

fn resolve_folder_sync_target_path(
    config: &mut config::AppConfig,
    remote_arg: Option<&str>,
) -> Result<PathBuf, String> {
    if let Some(remote_arg) = remote_arg {
        let remote_root = config::expand_path_like(remote_arg);
        config.sync_target_path = Some(remote_root.clone());
        config
            .save()
            .map_err(|error| format!("failed to save sync target: {error}"))?;
        return Ok(remote_root);
    }

    config
        .sync_target_path
        .clone()
        .ok_or_else(|| "missing sync target; use --remote PATH".to_string())
}

fn fallback_config_path(app_name: &str) -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Library")
        .join("Application Support")
        .join(app_name)
}

fn load_config_state() -> LoadedConfigState {
    let path = config::config_file_path()
        .unwrap_or_else(|_| fallback_config_path("bsj").join("config.json"));
    match config::AppConfig::load() {
        Ok(Some(config)) => LoadedConfigState {
            path,
            config,
            exists: true,
            error: None,
        },
        Ok(None) => LoadedConfigState {
            path,
            config: config::AppConfig::load_or_default(),
            exists: false,
            error: None,
        },
        Err(error) => LoadedConfigState {
            path,
            config: config::AppConfig::load_or_default(),
            exists: true,
            error: Some(error.to_string()),
        },
    }
}

fn load_vault_metadata_state(
    config: &config::AppConfig,
) -> (Option<vault::VaultMetadata>, Option<String>) {
    if !vault::vault_exists(&config.vault_path) {
        return (None, None);
    }
    match vault::load_vault_metadata(&config.vault_path) {
        Ok(metadata) => (Some(metadata), None),
        Err(error) => (None, Some(error.to_string())),
    }
}

fn render_markdown_export(entry: &vault::ExportedEntry) -> String {
    let mut out = String::new();
    out.push_str("# BlueScreen Journal Entry\n\n");
    out.push_str(&format!("Date: {}\n", entry.date.format("%Y-%m-%d")));
    out.push_str(&format!("Entry No.: {}\n\n", entry.entry_number));
    out.push_str(entry.body.trim_end());
    if let Some(closing_thought) = entry.closing_thought.as_deref() {
        if !entry.body.trim_end().is_empty() {
            out.push_str("\n\n");
        }
        out.push_str("## Closing Thought\n\n");
        out.push_str(closing_thought);
    }
    out
}

fn render_search_preset_lines(config: &config::AppConfig) -> Vec<String> {
    config
        .search_presets
        .iter()
        .map(|preset| {
            format!(
                "{}\t{}\t{}",
                preset.name,
                preset.query,
                preset_range_label(preset.from.as_deref(), preset.to.as_deref())
            )
        })
        .collect()
}

fn render_timeline_preset_lines(config: &config::AppConfig) -> Vec<String> {
    config
        .timeline_presets
        .iter()
        .map(|preset| {
            let mut filters = Vec::new();
            if let Some(query) = preset.query.as_deref() {
                filters.push(format!("query={query}"));
            }
            if let Some(tag) = preset.tag.as_deref() {
                filters.push(format!("tag={tag}"));
            }
            if let Some(person) = preset.person.as_deref() {
                filters.push(format!("person={person}"));
            }
            if let Some(project) = preset.project.as_deref() {
                filters.push(format!("project={project}"));
            }
            if let Some(mood) = preset.mood {
                filters.push(format!("mood={mood}"));
            }
            let filter_text = if filters.is_empty() {
                "-".to_string()
            } else {
                filters.join(";")
            };
            format!(
                "{}\t{}\t{}",
                preset.name,
                preset_range_label(preset.from.as_deref(), preset.to.as_deref()),
                filter_text
            )
        })
        .collect()
}

fn preset_range_label(from: Option<&str>, to: Option<&str>) -> String {
    match (from.map(str::trim), to.map(str::trim)) {
        (Some(from), Some(to)) if !from.is_empty() && !to.is_empty() => format!("{from}..{to}"),
        (Some(from), _) if !from.is_empty() => format!("FROM {from}"),
        (_, Some(to)) if !to.is_empty() => format!("TO {to}"),
        _ => "ALL TIME".to_string(),
    }
}

fn write_plaintext_output(path: &Path, text: &str) -> Result<(), String> {
    secure_fs::atomic_write_restricted(path, text.as_bytes())
        .map_err(|error| format!("failed to write {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{
        Cli, Command, DateRangeArg, PromptCategoryArg, SearchSortArg, SysopCommand,
        TimelineFilters, TimelineFormatArg, TimelineGroupByArg, WeekdayArg, build_search_summary,
        build_timeline_summary, compute_review_word_stats, deterministic_prompt_for_date,
        escape_csv_cell, filter_timeline_entries, format_metadata_stamp, group_timeline_entries,
        next_blank_date, parse_favorite_dates, preset_range_label, prompts_for_category,
        rank_lines, render_markdown_export, render_search_preset_lines,
        render_timeline_preset_lines, resolve_range_bounds, sort_search_results,
        summarize_review_entries,
    };
    use crate::config::{
        AppConfig, BackupRetentionConfig, SearchPresetConfig, TimelinePresetConfig,
    };
    use crate::search::{SearchDocument, SearchResult, Snippet};
    use crate::vault::{EntryMetadata, ExportedEntry, IndexEntry};
    use chrono::NaiveDate;
    use clap::Parser;
    use clap::error::ErrorKind;
    use std::collections::HashSet;
    use std::path::PathBuf;

    #[test]
    fn markdown_export_includes_metadata_and_closing_thought() {
        let entry = ExportedEntry {
            date: NaiveDate::from_ymd_opt(2026, 3, 17).expect("date"),
            entry_number: "0000017".to_string(),
            metadata: EntryMetadata {
                tags: vec!["work".to_string()],
                people: vec!["Alex".to_string()],
                project: Some("BlueScreen".to_string()),
                mood: Some(7),
            },
            body: "Body text".to_string(),
            closing_thought: Some("Lights out.".to_string()),
        };

        let markdown = render_markdown_export(&entry);
        assert!(markdown.contains("# BlueScreen Journal Entry"));
        assert!(markdown.contains("Entry No.: 0000017"));
        assert!(markdown.contains("## Closing Thought"));
        assert!(markdown.contains("Lights out."));
    }

    #[test]
    fn preset_range_label_formats_all_cases() {
        assert_eq!(
            preset_range_label(Some("2026-03-01"), Some("2026-03-19")),
            "2026-03-01..2026-03-19"
        );
        assert_eq!(
            preset_range_label(Some("2026-03-01"), None),
            "FROM 2026-03-01"
        );
        assert_eq!(
            preset_range_label(None, Some("2026-03-19")),
            "TO 2026-03-19"
        );
        assert_eq!(preset_range_label(None, None), "ALL TIME");
    }

    #[test]
    fn render_search_preset_lines_includes_name_query_and_range() {
        let config = AppConfig {
            vault_path: PathBuf::from("/tmp/vault"),
            sync_target_path: None,
            local_device_id: None,
            device_nickname: "This Mac".to_string(),
            typewriter_mode: false,
            clock_12h: false,
            show_seconds: false,
            show_ruler: true,
            show_footer_legend: true,
            soundtrack_source: String::new(),
            opening_line_template: String::new(),
            daily_word_goal: None,
            remember_passphrase_in_keychain: false,
            first_run_coach_completed: false,
            last_sync: None,
            sync_history: Vec::new(),
            favorite_dates: Vec::new(),
            export_history: Vec::new(),
            search_presets: vec![SearchPresetConfig {
                name: "Weekly".to_string(),
                query: "project alpha".to_string(),
                from: Some("2026-03-13".to_string()),
                to: Some("2026-03-19".to_string()),
            }],
            timeline_presets: vec![TimelinePresetConfig {
                name: "Recent Work".to_string(),
                from: Some("2026-03-01".to_string()),
                to: Some("2026-03-19".to_string()),
                query: Some("ship".to_string()),
                tag: Some("work".to_string()),
                person: None,
                project: None,
                mood: None,
            }],
            backup_retention: BackupRetentionConfig::default(),
            macros: Vec::new(),
        };

        let lines = render_search_preset_lines(&config);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("Weekly"));
        assert!(lines[0].contains("project alpha"));
        assert!(lines[0].contains("2026-03-13..2026-03-19"));
    }

    #[test]
    fn render_timeline_preset_lines_includes_filter_summary() {
        let config = AppConfig {
            vault_path: PathBuf::from("/tmp/vault"),
            sync_target_path: None,
            local_device_id: None,
            device_nickname: "This Mac".to_string(),
            typewriter_mode: false,
            clock_12h: false,
            show_seconds: false,
            show_ruler: true,
            show_footer_legend: true,
            soundtrack_source: String::new(),
            opening_line_template: String::new(),
            daily_word_goal: None,
            remember_passphrase_in_keychain: false,
            first_run_coach_completed: false,
            last_sync: None,
            sync_history: Vec::new(),
            favorite_dates: Vec::new(),
            export_history: Vec::new(),
            search_presets: Vec::new(),
            timeline_presets: vec![TimelinePresetConfig {
                name: "Weekly".to_string(),
                from: Some("2026-03-10".to_string()),
                to: Some("2026-03-19".to_string()),
                query: Some("ship".to_string()),
                tag: Some("work".to_string()),
                person: Some("riley".to_string()),
                project: None,
                mood: Some(7),
            }],
            backup_retention: BackupRetentionConfig::default(),
            macros: Vec::new(),
        };

        let lines = render_timeline_preset_lines(&config);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("Weekly"));
        assert!(lines[0].contains("2026-03-10..2026-03-19"));
        assert!(lines[0].contains("query=ship"));
        assert!(lines[0].contains("tag=work"));
        assert!(lines[0].contains("mood=7"));
    }

    #[test]
    fn cli_supports_version_flag() {
        let err = Cli::try_parse_from(["bsj", "--version"])
            .expect_err("expected --version to short-circuit with clap display output");
        assert_eq!(err.kind(), ErrorKind::DisplayVersion);
    }

    #[test]
    fn cli_parses_advanced_search_flags() {
        let cli = Cli::parse_from([
            "bsj",
            "search",
            "focus",
            "--json",
            "--limit",
            "12",
            "--count-only",
            "--case-sensitive",
            "--whole-word",
            "--context",
            "40",
        ]);

        match cli.command {
            Some(Command::Search {
                query,
                json,
                limit,
                count_only,
                case_sensitive,
                whole_word,
                context,
                ..
            }) => {
                assert_eq!(query.as_deref(), Some("focus"));
                assert!(json);
                assert_eq!(limit, 12);
                assert!(count_only);
                assert!(case_sensitive);
                assert!(whole_word);
                assert_eq!(context, 40);
            }
            _ => panic!("expected search command"),
        }
    }

    #[test]
    fn cli_parses_search_range_match_sort_and_summary_flags() {
        let cli = Cli::parse_from([
            "bsj",
            "search",
            "focus",
            "--range",
            "last7",
            "--match-mode",
            "any",
            "--sort",
            "relevance",
            "--hits-per-entry",
            "4",
            "--summary",
        ]);

        match cli.command {
            Some(Command::Search {
                range,
                match_mode,
                sort,
                hits_per_entry,
                summary,
                ..
            }) => {
                assert!(matches!(range, Some(DateRangeArg::Last7)));
                assert!(matches!(match_mode, super::SearchMatchArg::Any));
                assert!(matches!(sort, SearchSortArg::Relevance));
                assert_eq!(hits_per_entry, 4);
                assert!(summary);
            }
            _ => panic!("expected search command"),
        }
    }

    #[test]
    fn cli_parses_spellcheck_date_and_output_flags() {
        let cli = Cli::parse_from([
            "bsj",
            "spellcheck",
            "--date",
            "2026-03-18",
            "--json",
            "--limit",
            "25",
            "--count-only",
        ]);
        match cli.command {
            Some(Command::Spellcheck {
                date,
                from,
                to,
                json,
                limit,
                count_only,
                ..
            }) => {
                assert_eq!(date.as_deref(), Some("2026-03-18"));
                assert_eq!(from, None);
                assert_eq!(to, None);
                assert!(json);
                assert_eq!(limit, 25);
                assert!(count_only);
            }
            _ => panic!("expected spellcheck command"),
        }
    }

    #[test]
    fn cli_parses_spellcheck_range_flags() {
        let cli = Cli::parse_from([
            "bsj",
            "spellcheck",
            "--from",
            "2026-03-01",
            "--to",
            "2026-03-19",
            "--range",
            "last30",
        ]);
        match cli.command {
            Some(Command::Spellcheck {
                from, to, range, ..
            }) => {
                assert_eq!(from.as_deref(), Some("2026-03-01"));
                assert_eq!(to.as_deref(), Some("2026-03-19"));
                assert!(matches!(range, Some(DateRangeArg::Last30)));
            }
            _ => panic!("expected spellcheck command"),
        }
    }

    #[test]
    fn cli_parses_timeline_metadata_filters() {
        let cli = Cli::parse_from([
            "bsj",
            "timeline",
            "--query",
            "ship",
            "--tag",
            "work",
            "--person",
            "riley",
            "--project",
            "phoenix",
        ]);

        match cli.command {
            Some(Command::Timeline {
                query,
                tag,
                person,
                project,
                ..
            }) => {
                assert_eq!(query.as_deref(), Some("ship"));
                assert_eq!(tag.as_deref(), Some("work"));
                assert_eq!(person.as_deref(), Some("riley"));
                assert_eq!(project.as_deref(), Some("phoenix"));
            }
            _ => panic!("expected timeline command"),
        }
    }

    #[test]
    fn cli_parses_review_range_and_json_flags() {
        let cli = Cli::parse_from([
            "bsj",
            "review",
            "--from",
            "2026-03-01",
            "--to",
            "2026-03-15",
            "--json",
            "--min-count",
            "2",
        ]);

        match cli.command {
            Some(Command::Review {
                from,
                to,
                json,
                min_count,
                ..
            }) => {
                assert_eq!(from.as_deref(), Some("2026-03-01"));
                assert_eq!(to.as_deref(), Some("2026-03-15"));
                assert!(json);
                assert_eq!(min_count, 2);
            }
            _ => panic!("expected review command"),
        }
    }

    #[test]
    fn cli_parses_review_range_preset_and_goal() {
        let cli = Cli::parse_from(["bsj", "review", "--range", "last30", "--goal", "750"]);
        match cli.command {
            Some(Command::Review { range, goal, .. }) => {
                assert!(matches!(range, Some(DateRangeArg::Last30)));
                assert_eq!(goal, Some(750));
            }
            _ => panic!("expected review command"),
        }
    }

    #[test]
    fn cli_parses_timeline_format_and_extended_filters() {
        let cli = Cli::parse_from([
            "bsj",
            "timeline",
            "--format",
            "csv",
            "--mood",
            "7",
            "--has-tags",
            "--has-people",
            "--has-project",
            "--weekday",
            "mon,wed",
            "--summary",
        ]);

        match cli.command {
            Some(Command::Timeline {
                mood,
                has_tags,
                has_people,
                has_project,
                weekday,
                format,
                summary,
                ..
            }) => {
                assert_eq!(mood, Some(7));
                assert!(has_tags);
                assert!(has_people);
                assert!(has_project);
                assert_eq!(weekday, vec![WeekdayArg::Mon, WeekdayArg::Wed]);
                assert_eq!(format, TimelineFormatArg::Csv);
                assert!(summary);
            }
            _ => panic!("expected timeline command"),
        }
    }

    #[test]
    fn cli_parses_timeline_range_group_and_preset_flags() {
        let cli = Cli::parse_from([
            "bsj",
            "timeline",
            "--range",
            "last30",
            "--group-by",
            "week",
            "--preset",
            "Recent Work",
            "--save-preset",
            "Last Month",
        ]);
        match cli.command {
            Some(Command::Timeline {
                range,
                group_by,
                preset,
                save_preset,
                ..
            }) => {
                assert!(matches!(range, Some(DateRangeArg::Last30)));
                assert!(matches!(group_by, Some(TimelineGroupByArg::Week)));
                assert_eq!(preset.as_deref(), Some("Recent Work"));
                assert_eq!(save_preset.as_deref(), Some("Last Month"));
            }
            _ => panic!("expected timeline command"),
        }
    }

    #[test]
    fn cli_parses_prompts_json_flags() {
        let list = Cli::parse_from(["bsj", "prompts", "list", "--json"]);
        match list.command {
            Some(Command::Prompts { command }) => match command {
                super::PromptCommand::List { json, .. } => assert!(json),
                _ => panic!("expected prompts list command"),
            },
            _ => panic!("expected prompts command"),
        }

        let pick = Cli::parse_from(["bsj", "prompts", "pick", "--json"]);
        match pick.command {
            Some(Command::Prompts { command }) => match command {
                super::PromptCommand::Pick { json, .. } => assert!(json),
                _ => panic!("expected prompts pick command"),
            },
            _ => panic!("expected prompts command"),
        }
    }

    #[test]
    fn cli_parses_ai_summary_flags() {
        let cli = Cli::parse_from([
            "bsj",
            "ai",
            "summary",
            "--range",
            "last7",
            "--max-points",
            "6",
            "--json",
            "--remote",
        ]);

        match cli.command {
            Some(Command::Ai {
                command:
                    super::AiCommand::Summary {
                        range,
                        max_points,
                        json,
                        remote,
                        ..
                    },
            }) => {
                assert!(matches!(range, Some(DateRangeArg::Last7)));
                assert_eq!(max_points, 6);
                assert!(json);
                assert!(remote);
            }
            _ => panic!("expected ai summary command"),
        }
    }

    #[test]
    fn cli_parses_ai_coach_flags() {
        let cli = Cli::parse_from([
            "bsj",
            "ai",
            "coach",
            "--date",
            "2026-03-19",
            "--questions",
            "7",
            "--remote",
        ]);

        match cli.command {
            Some(Command::Ai {
                command:
                    super::AiCommand::Coach {
                        date,
                        questions,
                        remote,
                        ..
                    },
            }) => {
                assert_eq!(date.as_deref(), Some("2026-03-19"));
                assert_eq!(questions, 7);
                assert!(remote);
            }
            _ => panic!("expected ai coach command"),
        }
    }

    #[test]
    fn cli_parses_sysop_permissions_command() {
        let cli = Cli::parse_from(["bsj", "sysop", "permissions", "--json", "--limit", "5"]);
        match cli.command {
            Some(Command::Sysop {
                command: SysopCommand::Permissions { json, limit },
            }) => {
                assert!(json);
                assert_eq!(limit, 5);
            }
            _ => panic!("expected sysop permissions command"),
        }
    }

    #[test]
    fn cli_parses_sysop_sync_preview_command() {
        let cli = Cli::parse_from([
            "bsj",
            "sysop",
            "sync-preview",
            "--backend",
            "folder",
            "--remote",
            "/tmp/bsj-sync",
            "--json",
        ]);
        match cli.command {
            Some(Command::Sysop {
                command:
                    SysopCommand::SyncPreview {
                        backend,
                        remote,
                        json,
                    },
            }) => {
                assert!(matches!(backend, Some(super::SyncBackendArg::Folder)));
                assert_eq!(remote.as_deref(), Some("/tmp/bsj-sync"));
                assert!(json);
            }
            _ => panic!("expected sysop sync-preview command"),
        }
    }

    fn sample_index_entry(date: (i32, u32, u32), conflict: bool, preview: &str) -> IndexEntry {
        IndexEntry {
            date: NaiveDate::from_ymd_opt(date.0, date.1, date.2).expect("date"),
            entry_number: "000123".to_string(),
            preview: preview.to_string(),
            has_conflict: conflict,
            metadata: EntryMetadata::default(),
        }
    }

    #[test]
    fn timeline_filters_range_and_limit_descending() {
        let entries = vec![
            sample_index_entry((2026, 3, 10), false, "a"),
            sample_index_entry((2026, 3, 11), false, "b"),
            sample_index_entry((2026, 3, 12), false, "c"),
        ];
        let filtered = filter_timeline_entries(
            entries,
            TimelineFilters {
                from: Some(NaiveDate::from_ymd_opt(2026, 3, 10).expect("from")),
                to: Some(NaiveDate::from_ymd_opt(2026, 3, 11).expect("to")),
                range: None,
                query: None,
                tag: None,
                person: None,
                project: None,
                mood: None,
                has_tags_only: false,
                has_people_only: false,
                has_project_only: false,
                weekdays: HashSet::new(),
                limit: 1,
                asc: false,
                favorites_only: false,
                conflicts_only: false,
                show_metadata: false,
                group_by: None,
                format: TimelineFormatArg::Text,
                summary_only: false,
                preset_name: None,
                list_presets: false,
                save_preset: None,
                delete_preset: None,
            },
            &HashSet::new(),
        );
        assert_eq!(filtered.len(), 1);
        assert_eq!(
            filtered[0].date,
            NaiveDate::from_ymd_opt(2026, 3, 11).expect("date")
        );
    }

    #[test]
    fn timeline_filters_favorites_and_conflicts() {
        let entries = vec![
            sample_index_entry((2026, 3, 10), false, "a"),
            sample_index_entry((2026, 3, 11), true, "b"),
            sample_index_entry((2026, 3, 12), true, "c"),
        ];
        let favorites = parse_favorite_dates(&[
            "2026-03-11".to_string(),
            "invalid".to_string(),
            "2026-03-20".to_string(),
        ]);
        let filtered = filter_timeline_entries(
            entries,
            TimelineFilters {
                from: None,
                to: None,
                range: None,
                query: None,
                tag: None,
                person: None,
                project: None,
                mood: None,
                has_tags_only: false,
                has_people_only: false,
                has_project_only: false,
                weekdays: HashSet::new(),
                limit: 10,
                asc: true,
                favorites_only: true,
                conflicts_only: true,
                show_metadata: false,
                group_by: None,
                format: TimelineFormatArg::Text,
                summary_only: false,
                preset_name: None,
                list_presets: false,
                save_preset: None,
                delete_preset: None,
            },
            &favorites,
        );
        assert_eq!(filtered.len(), 1);
        assert_eq!(
            filtered[0].date,
            NaiveDate::from_ymd_opt(2026, 3, 11).expect("date")
        );
    }

    #[test]
    fn next_blank_date_skips_existing_days() {
        let start = NaiveDate::from_ymd_opt(2026, 3, 19).expect("start");
        let existing = HashSet::from([
            start,
            start.succ_opt().expect("next1"),
            start
                .succ_opt()
                .and_then(|day| day.succ_opt())
                .expect("next2"),
        ]);
        let next = next_blank_date(start, &existing);
        assert_eq!(
            next,
            NaiveDate::from_ymd_opt(2026, 3, 22).expect("expected")
        );
    }

    #[test]
    fn rank_lines_limits_and_formats() {
        let lines = rank_lines(
            &[
                ("work".to_string(), 4),
                ("health".to_string(), 2),
                ("family".to_string(), 1),
            ],
            2,
        );
        assert_eq!(lines, vec!["work: 4".to_string(), "health: 2".to_string()]);
    }

    #[test]
    fn prompts_category_filter_and_deterministic_pick() {
        let focus_prompts = prompts_for_category(Some(PromptCategoryArg::Focus));
        assert!(!focus_prompts.is_empty());
        assert!(
            focus_prompts
                .iter()
                .all(|prompt| prompt.category == PromptCategoryArg::Focus)
        );

        let date = NaiveDate::from_ymd_opt(2026, 3, 19).expect("date");
        let first =
            deterministic_prompt_for_date(date, Some(PromptCategoryArg::Focus)).expect("prompt #1");
        let second =
            deterministic_prompt_for_date(date, Some(PromptCategoryArg::Focus)).expect("prompt #2");
        assert_eq!(first, second);
    }

    #[test]
    fn metadata_stamp_includes_all_fields() {
        let metadata = EntryMetadata {
            tags: vec!["work".to_string()],
            people: vec!["Riley".to_string()],
            project: Some("BlueScreen".to_string()),
            mood: Some(8),
        };
        let stamp = format_metadata_stamp(&metadata).expect("stamp");
        assert!(stamp.contains("tags=work"));
        assert!(stamp.contains("people=Riley"));
        assert!(stamp.contains("project=BlueScreen"));
        assert!(stamp.contains("mood=8"));
    }

    #[test]
    fn timeline_query_filter_matches_preview_and_metadata() {
        let entries = vec![
            IndexEntry {
                date: NaiveDate::from_ymd_opt(2026, 3, 10).expect("date"),
                entry_number: "000123".to_string(),
                preview: "Morning ship review".to_string(),
                has_conflict: false,
                metadata: EntryMetadata::default(),
            },
            IndexEntry {
                date: NaiveDate::from_ymd_opt(2026, 3, 11).expect("date"),
                entry_number: "000124".to_string(),
                preview: "Quiet afternoon".to_string(),
                has_conflict: false,
                metadata: EntryMetadata {
                    tags: vec!["DeepWork".to_string()],
                    people: vec!["Riley".to_string()],
                    project: Some("Phoenix".to_string()),
                    mood: None,
                },
            },
        ];
        let filtered = filter_timeline_entries(
            entries,
            TimelineFilters {
                from: None,
                to: None,
                range: None,
                query: Some("phoenix".to_string()),
                tag: None,
                person: None,
                project: None,
                mood: None,
                has_tags_only: false,
                has_people_only: false,
                has_project_only: false,
                weekdays: HashSet::new(),
                limit: 10,
                asc: false,
                favorites_only: false,
                conflicts_only: false,
                show_metadata: false,
                group_by: None,
                format: TimelineFormatArg::Text,
                summary_only: false,
                preset_name: None,
                list_presets: false,
                save_preset: None,
                delete_preset: None,
            },
            &HashSet::new(),
        );
        assert_eq!(filtered.len(), 1);
        assert_eq!(
            filtered[0].date,
            NaiveDate::from_ymd_opt(2026, 3, 11).expect("date")
        );
    }

    #[test]
    fn timeline_tag_person_and_project_filters_are_case_insensitive() {
        let entries = vec![
            IndexEntry {
                date: NaiveDate::from_ymd_opt(2026, 3, 11).expect("date"),
                entry_number: "000124".to_string(),
                preview: "Quiet afternoon".to_string(),
                has_conflict: false,
                metadata: EntryMetadata {
                    tags: vec!["DeepWork".to_string()],
                    people: vec!["Riley".to_string()],
                    project: Some("Phoenix".to_string()),
                    mood: None,
                },
            },
            IndexEntry {
                date: NaiveDate::from_ymd_opt(2026, 3, 12).expect("date"),
                entry_number: "000125".to_string(),
                preview: "Another entry".to_string(),
                has_conflict: false,
                metadata: EntryMetadata {
                    tags: vec!["Admin".to_string()],
                    people: vec!["Morgan".to_string()],
                    project: Some("Blue".to_string()),
                    mood: None,
                },
            },
        ];

        let tag_filtered = filter_timeline_entries(
            entries.clone(),
            TimelineFilters {
                from: None,
                to: None,
                range: None,
                query: None,
                tag: Some("deepwork".to_string()),
                person: None,
                project: None,
                mood: None,
                has_tags_only: false,
                has_people_only: false,
                has_project_only: false,
                weekdays: HashSet::new(),
                limit: 10,
                asc: false,
                favorites_only: false,
                conflicts_only: false,
                show_metadata: false,
                group_by: None,
                format: TimelineFormatArg::Text,
                summary_only: false,
                preset_name: None,
                list_presets: false,
                save_preset: None,
                delete_preset: None,
            },
            &HashSet::new(),
        );
        assert_eq!(tag_filtered.len(), 1);

        let person_filtered = filter_timeline_entries(
            entries.clone(),
            TimelineFilters {
                from: None,
                to: None,
                range: None,
                query: None,
                tag: None,
                person: Some("riley".to_string()),
                project: None,
                mood: None,
                has_tags_only: false,
                has_people_only: false,
                has_project_only: false,
                weekdays: HashSet::new(),
                limit: 10,
                asc: false,
                favorites_only: false,
                conflicts_only: false,
                show_metadata: false,
                group_by: None,
                format: TimelineFormatArg::Text,
                summary_only: false,
                preset_name: None,
                list_presets: false,
                save_preset: None,
                delete_preset: None,
            },
            &HashSet::new(),
        );
        assert_eq!(person_filtered.len(), 1);

        let project_filtered = filter_timeline_entries(
            entries,
            TimelineFilters {
                from: None,
                to: None,
                range: None,
                query: None,
                tag: None,
                person: None,
                project: Some("PHOENIX".to_string()),
                mood: None,
                has_tags_only: false,
                has_people_only: false,
                has_project_only: false,
                weekdays: HashSet::new(),
                limit: 10,
                asc: false,
                favorites_only: false,
                conflicts_only: false,
                show_metadata: false,
                group_by: None,
                format: TimelineFormatArg::Text,
                summary_only: false,
                preset_name: None,
                list_presets: false,
                save_preset: None,
                delete_preset: None,
            },
            &HashSet::new(),
        );
        assert_eq!(project_filtered.len(), 1);
    }

    #[test]
    fn timeline_filters_mood_presence_and_weekday() {
        let entries = vec![
            IndexEntry {
                date: NaiveDate::from_ymd_opt(2026, 3, 16).expect("date"), // Monday
                entry_number: "000101".to_string(),
                preview: "monday work".to_string(),
                has_conflict: false,
                metadata: EntryMetadata {
                    tags: vec!["work".to_string()],
                    people: vec!["Riley".to_string()],
                    project: Some("Phoenix".to_string()),
                    mood: Some(7),
                },
            },
            IndexEntry {
                date: NaiveDate::from_ymd_opt(2026, 3, 17).expect("date"), // Tuesday
                entry_number: "000102".to_string(),
                preview: "tuesday".to_string(),
                has_conflict: false,
                metadata: EntryMetadata {
                    tags: Vec::new(),
                    people: vec!["Riley".to_string()],
                    project: Some("Phoenix".to_string()),
                    mood: Some(7),
                },
            },
        ];

        let filtered = filter_timeline_entries(
            entries,
            TimelineFilters {
                from: None,
                to: None,
                range: None,
                query: None,
                tag: None,
                person: None,
                project: None,
                mood: Some(7),
                has_tags_only: true,
                has_people_only: true,
                has_project_only: true,
                weekdays: HashSet::from([WeekdayArg::Mon.to_chrono()]),
                limit: 10,
                asc: false,
                favorites_only: false,
                conflicts_only: false,
                show_metadata: false,
                group_by: None,
                format: TimelineFormatArg::Text,
                summary_only: false,
                preset_name: None,
                list_presets: false,
                save_preset: None,
                delete_preset: None,
            },
            &HashSet::new(),
        );

        assert_eq!(filtered.len(), 1);
        assert_eq!(
            filtered[0].date,
            NaiveDate::from_ymd_opt(2026, 3, 16).expect("date")
        );
    }

    #[test]
    fn timeline_summary_counts_conflicts_favorites_and_moods() {
        let entries = vec![
            IndexEntry {
                date: NaiveDate::from_ymd_opt(2026, 3, 10).expect("date"),
                entry_number: "000100".to_string(),
                preview: "a".to_string(),
                has_conflict: true,
                metadata: EntryMetadata {
                    mood: Some(5),
                    ..EntryMetadata::default()
                },
            },
            IndexEntry {
                date: NaiveDate::from_ymd_opt(2026, 3, 11).expect("date"),
                entry_number: "000101".to_string(),
                preview: "b".to_string(),
                has_conflict: false,
                metadata: EntryMetadata {
                    mood: Some(5),
                    ..EntryMetadata::default()
                },
            },
        ];
        let favorites = HashSet::from([NaiveDate::from_ymd_opt(2026, 3, 11).expect("fav")]);

        let summary = build_timeline_summary(&entries, &favorites);
        assert_eq!(summary.total, 2);
        assert_eq!(summary.conflicts, 1);
        assert_eq!(summary.favorites, 1);
        assert_eq!(summary.first_date.as_deref(), Some("2026-03-10"));
        assert_eq!(summary.last_date.as_deref(), Some("2026-03-11"));
        assert_eq!(summary.moods, vec![(5, 2)]);
    }

    #[test]
    fn review_summary_applies_range_and_min_count() {
        let entries = vec![
            IndexEntry {
                date: NaiveDate::from_ymd_opt(2026, 3, 10).expect("date"),
                entry_number: "000100".to_string(),
                preview: "a".to_string(),
                has_conflict: false,
                metadata: EntryMetadata {
                    tags: vec!["work".to_string()],
                    people: vec!["Riley".to_string()],
                    project: Some("Phoenix".to_string()),
                    mood: Some(7),
                },
            },
            IndexEntry {
                date: NaiveDate::from_ymd_opt(2026, 3, 11).expect("date"),
                entry_number: "000101".to_string(),
                preview: "b".to_string(),
                has_conflict: false,
                metadata: EntryMetadata {
                    tags: vec!["work".to_string()],
                    people: vec!["Riley".to_string()],
                    project: Some("Phoenix".to_string()),
                    mood: Some(6),
                },
            },
            IndexEntry {
                date: NaiveDate::from_ymd_opt(2026, 3, 12).expect("date"),
                entry_number: "000102".to_string(),
                preview: "c".to_string(),
                has_conflict: false,
                metadata: EntryMetadata {
                    tags: vec!["family".to_string()],
                    people: vec!["Morgan".to_string()],
                    project: Some("Home".to_string()),
                    mood: Some(8),
                },
            },
        ];

        let summary = summarize_review_entries(
            entries,
            NaiveDate::from_ymd_opt(2026, 3, 19).expect("today"),
            Some(NaiveDate::from_ymd_opt(2026, 3, 10).expect("from")),
            Some(NaiveDate::from_ymd_opt(2026, 3, 11).expect("to")),
            2,
        );

        assert_eq!(summary.total_entries, 2);
        assert_eq!(summary.top_tags, vec![("work".to_string(), 2)]);
        assert_eq!(summary.top_people, vec![("Riley".to_string(), 2)]);
        assert_eq!(summary.top_projects, vec![("Phoenix".to_string(), 2)]);
    }

    #[test]
    fn resolve_range_bounds_last7_uses_inclusive_window() {
        let today = NaiveDate::from_ymd_opt(2026, 3, 19).expect("today");
        let (from, to) = resolve_range_bounds(DateRangeArg::Last7, today);
        assert_eq!(from, NaiveDate::from_ymd_opt(2026, 3, 13).expect("from"));
        assert_eq!(to, today);
    }

    #[test]
    fn sort_search_results_relevance_prefers_exact_query_match() {
        let mut results = vec![
            SearchResult {
                date: NaiveDate::from_ymd_opt(2026, 3, 19).expect("date"),
                entry_number: "1".to_string(),
                snippet: Snippet {
                    text: "abc".to_string(),
                    highlight_start: 0,
                    highlight_end: 3,
                },
                row: 0,
                start_col: 0,
                end_col: 3,
                matched_text: "foc".to_string(),
            },
            SearchResult {
                date: NaiveDate::from_ymd_opt(2026, 3, 18).expect("date"),
                entry_number: "2".to_string(),
                snippet: Snippet {
                    text: "def".to_string(),
                    highlight_start: 0,
                    highlight_end: 5,
                },
                row: 0,
                start_col: 0,
                end_col: 5,
                matched_text: "focus".to_string(),
            },
        ];
        sort_search_results(&mut results, SearchSortArg::Relevance, "focus", false);
        assert_eq!(results[0].matched_text, "focus");
    }

    #[test]
    fn build_search_summary_counts_matches_and_unique_dates() {
        let results = vec![
            SearchResult {
                date: NaiveDate::from_ymd_opt(2026, 3, 19).expect("date"),
                entry_number: "1".to_string(),
                snippet: Snippet {
                    text: "a".to_string(),
                    highlight_start: 0,
                    highlight_end: 1,
                },
                row: 0,
                start_col: 0,
                end_col: 1,
                matched_text: "a".to_string(),
            },
            SearchResult {
                date: NaiveDate::from_ymd_opt(2026, 3, 19).expect("date"),
                entry_number: "1".to_string(),
                snippet: Snippet {
                    text: "b".to_string(),
                    highlight_start: 0,
                    highlight_end: 1,
                },
                row: 1,
                start_col: 0,
                end_col: 1,
                matched_text: "b".to_string(),
            },
            SearchResult {
                date: NaiveDate::from_ymd_opt(2026, 3, 18).expect("date"),
                entry_number: "2".to_string(),
                snippet: Snippet {
                    text: "c".to_string(),
                    highlight_start: 0,
                    highlight_end: 1,
                },
                row: 0,
                start_col: 0,
                end_col: 1,
                matched_text: "c".to_string(),
            },
        ];
        let summary = build_search_summary(&results);
        assert_eq!(summary.total_matches, 3);
        assert_eq!(summary.matched_dates, 2);
        assert_eq!(summary.top_dates[0], ("2026-03-19".to_string(), 2));
    }

    #[test]
    fn group_timeline_entries_rolls_up_week_and_counts_flags() {
        let entries = vec![
            IndexEntry {
                date: NaiveDate::from_ymd_opt(2026, 3, 17).expect("date"),
                entry_number: "000100".to_string(),
                preview: "a".to_string(),
                has_conflict: false,
                metadata: EntryMetadata::default(),
            },
            IndexEntry {
                date: NaiveDate::from_ymd_opt(2026, 3, 18).expect("date"),
                entry_number: "000101".to_string(),
                preview: "b".to_string(),
                has_conflict: true,
                metadata: EntryMetadata::default(),
            },
        ];
        let favorites = HashSet::from([NaiveDate::from_ymd_opt(2026, 3, 18).expect("fav")]);
        let grouped = group_timeline_entries(&entries, &favorites, TimelineGroupByArg::Week);
        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped[0].entries, 2);
        assert_eq!(grouped[0].conflicts, 1);
        assert_eq!(grouped[0].favorites, 1);
    }

    #[test]
    fn review_word_stats_counts_words_and_goal_hit_rate() {
        let docs = vec![
            SearchDocument {
                date: NaiveDate::from_ymd_opt(2026, 3, 18).expect("date"),
                entry_number: "1".to_string(),
                body: "one two three".to_string(),
            },
            SearchDocument {
                date: NaiveDate::from_ymd_opt(2026, 3, 18).expect("date"),
                entry_number: "2".to_string(),
                body: "four five".to_string(),
            },
            SearchDocument {
                date: NaiveDate::from_ymd_opt(2026, 3, 19).expect("date"),
                entry_number: "3".to_string(),
                body: "six seven eight nine".to_string(),
            },
        ];

        let stats = compute_review_word_stats(&docs, None, None, Some(4));
        assert_eq!(stats.total_words, 9);
        assert_eq!(stats.entries_counted, 3);
        assert_eq!(stats.active_days, 2);
        assert_eq!(stats.days_meeting_goal, 2);
        assert!((stats.goal_hit_rate - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn csv_escape_quotes_when_needed() {
        assert_eq!(escape_csv_cell("plain"), "plain");
        assert_eq!(escape_csv_cell("two,parts"), "\"two,parts\"");
        assert_eq!(escape_csv_cell("say \"hello\""), "\"say \"\"hello\"\"\"");
    }
}
