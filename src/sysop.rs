use chrono::{DateTime, NaiveDate, TimeDelta, Utc};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevisionStat {
    pub date: NaiveDate,
    pub revisions: usize,
    pub drafts: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaleDraft {
    pub date: NaiveDate,
    pub path: PathBuf,
    pub age_days: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct PermissionIssue {
    pub path: PathBuf,
    pub issue: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct VaultLayoutReport {
    pub vault_json_present: bool,
    pub entries_dir_present: bool,
    pub devices_dir_present: bool,
    pub backups_dir_present: bool,
    pub cache_file_present: bool,
    pub revision_files: usize,
    pub draft_files: usize,
    pub invalid_files: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivityPoint {
    pub date: NaiveDate,
    pub revisions: usize,
}

pub fn collect_revision_stats(root: &Path) -> Result<Vec<RevisionStat>, String> {
    let entries_root = root.join("entries");
    if !entries_root.exists() {
        return Ok(Vec::new());
    }

    let mut stats = Vec::new();
    for year_entry in fs::read_dir(&entries_root)
        .map_err(|error| format!("failed to read {}: {error}", entries_root.display()))?
    {
        let year_entry =
            year_entry.map_err(|error| format!("failed to read year entry: {error}"))?;
        if !year_entry
            .file_type()
            .map_err(|error| format!("failed to inspect file type: {error}"))?
            .is_dir()
        {
            continue;
        }
        for date_entry in fs::read_dir(year_entry.path())
            .map_err(|error| format!("failed to read date directory: {error}"))?
        {
            let date_entry =
                date_entry.map_err(|error| format!("failed to inspect date directory: {error}"))?;
            if !date_entry
                .file_type()
                .map_err(|error| format!("failed to inspect file type: {error}"))?
                .is_dir()
            {
                continue;
            }
            let date_name = date_entry.file_name().to_string_lossy().to_string();
            let Ok(date) = NaiveDate::parse_from_str(&date_name, "%Y-%m-%d") else {
                continue;
            };

            let mut revisions = 0usize;
            let mut drafts = 0usize;
            for file in fs::read_dir(date_entry.path()).map_err(|error| {
                format!("failed to read {}: {error}", date_entry.path().display())
            })? {
                let file = file.map_err(|error| format!("failed to inspect file: {error}"))?;
                if !file
                    .file_type()
                    .map_err(|error| format!("failed to inspect file type: {error}"))?
                    .is_file()
                {
                    continue;
                }
                let name = file.file_name().to_string_lossy().to_string();
                if is_revision_file_name(&name) {
                    revisions += 1;
                } else if is_draft_file_name(&name) {
                    drafts += 1;
                }
            }
            if revisions > 0 || drafts > 0 {
                stats.push(RevisionStat {
                    date,
                    revisions,
                    drafts,
                });
            }
        }
    }

    stats.sort_unstable_by(|left, right| {
        right
            .revisions
            .cmp(&left.revisions)
            .then_with(|| right.date.cmp(&left.date))
    });
    Ok(stats)
}

pub fn collect_stale_drafts(
    root: &Path,
    older_than_days: i64,
    now: DateTime<Utc>,
) -> Result<Vec<StaleDraft>, String> {
    let entries_root = root.join("entries");
    if !entries_root.exists() {
        return Ok(Vec::new());
    }

    let threshold = older_than_days.max(0);
    let mut stale = Vec::new();
    for year in fs::read_dir(&entries_root)
        .map_err(|error| format!("failed to read {}: {error}", entries_root.display()))?
    {
        let year = year.map_err(|error| format!("failed to inspect year: {error}"))?;
        if !year
            .file_type()
            .map_err(|error| format!("failed to inspect type: {error}"))?
            .is_dir()
        {
            continue;
        }
        for date_dir in fs::read_dir(year.path())
            .map_err(|error| format!("failed to read {}: {error}", year.path().display()))?
        {
            let date_dir =
                date_dir.map_err(|error| format!("failed to inspect date dir: {error}"))?;
            if !date_dir
                .file_type()
                .map_err(|error| format!("failed to inspect type: {error}"))?
                .is_dir()
            {
                continue;
            }
            let date_name = date_dir.file_name().to_string_lossy().to_string();
            let Ok(date) = NaiveDate::parse_from_str(&date_name, "%Y-%m-%d") else {
                continue;
            };
            for file in fs::read_dir(date_dir.path())
                .map_err(|error| format!("failed to read {}: {error}", date_dir.path().display()))?
            {
                let file =
                    file.map_err(|error| format!("failed to inspect draft file: {error}"))?;
                if !file
                    .file_type()
                    .map_err(|error| format!("failed to inspect file type: {error}"))?
                    .is_file()
                {
                    continue;
                }
                let name = file.file_name().to_string_lossy().to_string();
                if !is_draft_file_name(&name) {
                    continue;
                }
                let modified = file
                    .metadata()
                    .and_then(|metadata| metadata.modified())
                    .map(DateTime::<Utc>::from)
                    .map_err(|error| format!("failed to inspect modified time: {error}"))?;
                let age_days = now.signed_duration_since(modified).num_days();
                if age_days >= threshold {
                    stale.push(StaleDraft {
                        date,
                        path: file.path(),
                        age_days,
                    });
                }
            }
        }
    }

    stale.sort_unstable_by(|left, right| {
        right
            .age_days
            .cmp(&left.age_days)
            .then_with(|| right.date.cmp(&left.date))
    });
    Ok(stale)
}

pub fn collect_permission_issues(root: &Path) -> Result<Vec<PermissionIssue>, String> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut issues = Vec::new();
    visit_paths(root, &mut |path, metadata| {
        if metadata.file_type().is_symlink() {
            issues.push(PermissionIssue {
                path: path.to_path_buf(),
                issue: "symlink path detected".to_string(),
            });
            return;
        }
        #[cfg(unix)]
        {
            let mode = metadata.permissions().mode() & 0o777;
            if mode & 0o077 != 0 {
                issues.push(PermissionIssue {
                    path: path.to_path_buf(),
                    issue: format!("permissions too open ({mode:o})"),
                });
            }
        }
    })?;
    Ok(issues)
}

pub fn analyze_vault_layout(root: &Path) -> Result<VaultLayoutReport, String> {
    let mut report = VaultLayoutReport {
        vault_json_present: root.join("vault.json").is_file(),
        entries_dir_present: root.join("entries").is_dir(),
        devices_dir_present: root.join("devices").is_dir(),
        backups_dir_present: root.join("backups").is_dir(),
        cache_file_present: root.join(".cache").join("search-index.bsj.enc").is_file(),
        revision_files: 0,
        draft_files: 0,
        invalid_files: Vec::new(),
    };

    let entries_root = root.join("entries");
    if entries_root.exists() {
        visit_paths(&entries_root, &mut |path, metadata| {
            if !metadata.is_file() {
                return;
            }
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                report.invalid_files.push(path.to_path_buf());
                return;
            };
            if is_revision_file_name(name) {
                report.revision_files += 1;
            } else if is_draft_file_name(name) {
                report.draft_files += 1;
            } else {
                report.invalid_files.push(path.to_path_buf());
            }
        })?;
    }

    Ok(report)
}

pub fn collect_orphan_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut orphans = Vec::new();
    visit_paths(root, &mut |path, metadata| {
        if !metadata.is_file() {
            return;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            return;
        };
        if !is_expected_vault_file(relative) {
            orphans.push(path.to_path_buf());
        }
    })?;
    orphans.sort_unstable();
    Ok(orphans)
}

pub fn build_activity_series(
    stats: &[RevisionStat],
    today: NaiveDate,
    days: usize,
) -> Vec<ActivityPoint> {
    if days == 0 {
        return Vec::new();
    }
    let mut by_day = BTreeMap::<NaiveDate, usize>::new();
    for stat in stats {
        by_day.insert(stat.date, stat.revisions);
    }

    let clamped_days = days.min(366);
    let mut series = Vec::with_capacity(clamped_days);
    for offset in (0..clamped_days).rev() {
        let date = today - TimeDelta::days(offset as i64);
        let revisions = by_day.get(&date).copied().unwrap_or(0);
        series.push(ActivityPoint { date, revisions });
    }
    series
}

pub fn is_revision_file_name(name: &str) -> bool {
    name.starts_with("rev-") && name.ends_with(".bsj.enc")
}

pub fn is_draft_file_name(name: &str) -> bool {
    name.starts_with("draft-") && name.ends_with(".bsj.enc")
}

fn is_expected_vault_file(relative: &Path) -> bool {
    let value = relative.to_string_lossy().replace('\\', "/");
    if value == "vault.json" {
        return true;
    }
    if value == ".cache/search-index.bsj.enc" {
        return true;
    }
    if value.starts_with("devices/") && value.ends_with(".json") {
        return true;
    }
    if value.starts_with("backups/") && value.ends_with(".bsjbak.enc") {
        return true;
    }
    if let Some(name) = value.rsplit('/').next()
        && value.starts_with("entries/")
    {
        return is_revision_file_name(name) || is_draft_file_name(name);
    }
    false
}

fn visit_paths<F>(root: &Path, visit: &mut F) -> Result<(), String>
where
    F: FnMut(&Path, &fs::Metadata),
{
    let metadata = fs::symlink_metadata(root)
        .map_err(|error| format!("failed to inspect {}: {error}", root.display()))?;
    visit(root, &metadata);
    if !metadata.is_dir() {
        return Ok(());
    }

    for entry in
        fs::read_dir(root).map_err(|error| format!("failed to read {}: {error}", root.display()))?
    {
        let entry = entry.map_err(|error| format!("failed to inspect entry: {error}"))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
        visit(&path, &metadata);
        if metadata.is_dir() {
            visit_paths(&path, visit)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        analyze_vault_layout, build_activity_series, collect_orphan_files, collect_revision_stats,
        collect_stale_drafts, is_draft_file_name, is_revision_file_name,
    };
    use chrono::{NaiveDate, Utc};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn revision_and_draft_file_name_matchers_work() {
        assert!(is_revision_file_name("rev-device-000001.bsj.enc"));
        assert!(is_draft_file_name("draft-device.bsj.enc"));
        assert!(!is_revision_file_name("rev-device-000001.txt"));
        assert!(!is_draft_file_name("draft.txt"));
    }

    #[test]
    fn collect_revision_stats_counts_revisions_and_drafts() {
        let dir = tempdir().expect("tempdir");
        let date_dir = dir.path().join("entries/2026/2026-03-19");
        fs::create_dir_all(&date_dir).expect("mkdir");
        fs::write(date_dir.join("rev-dev-000001.bsj.enc"), b"x").expect("rev");
        fs::write(date_dir.join("rev-dev-000002.bsj.enc"), b"x").expect("rev");
        fs::write(date_dir.join("draft-dev.bsj.enc"), b"x").expect("draft");

        let stats = collect_revision_stats(dir.path()).expect("stats");
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].revisions, 2);
        assert_eq!(stats[0].drafts, 1);
    }

    #[test]
    fn analyze_layout_marks_invalid_entry_files() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        fs::create_dir_all(root.join("entries/2026/2026-03-19")).expect("mkdir");
        fs::write(
            root.join("entries/2026/2026-03-19/not-valid-name.bin"),
            b"x",
        )
        .expect("write");
        fs::write(root.join("vault.json"), b"{}").expect("vault");

        let report = analyze_vault_layout(root).expect("layout");
        assert!(report.vault_json_present);
        assert_eq!(report.invalid_files.len(), 1);
    }

    #[test]
    fn orphan_scan_detects_unknown_files() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        fs::create_dir_all(root.join("entries/2026/2026-03-19")).expect("mkdir");
        fs::write(
            root.join("entries/2026/2026-03-19/rev-dev-000001.bsj.enc"),
            b"x",
        )
        .expect("rev");
        fs::write(root.join("README.txt"), b"oops").expect("oops");

        let orphans = collect_orphan_files(root).expect("orphans");
        assert_eq!(orphans.len(), 1);
        assert!(orphans[0].ends_with("README.txt"));
    }

    #[test]
    fn stale_draft_scan_respects_threshold_days() {
        let dir = tempdir().expect("tempdir");
        let date_dir = dir.path().join("entries/2026/2026-03-19");
        fs::create_dir_all(&date_dir).expect("mkdir");
        fs::write(date_dir.join("draft-dev.bsj.enc"), b"x").expect("draft");

        let stale = collect_stale_drafts(dir.path(), 0, Utc::now()).expect("stale");
        assert_eq!(stale.len(), 1);
    }

    #[test]
    fn activity_series_covers_requested_range() {
        let stats = vec![super::RevisionStat {
            date: NaiveDate::from_ymd_opt(2026, 3, 18).expect("date"),
            revisions: 3,
            drafts: 0,
        }];
        let series = build_activity_series(
            &stats,
            NaiveDate::from_ymd_opt(2026, 3, 19).expect("today"),
            2,
        );
        assert_eq!(series.len(), 2);
        assert_eq!(series[0].revisions, 3);
        assert_eq!(series[1].revisions, 0);
    }
}
