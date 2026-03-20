# Setup Guide

## Install Paths

Preferred end-user install:

```bash
curl -fsSL https://raw.githubusercontent.com/Awassee/bluescreenjournal/main/install.sh | bash
```

Install a specific tagged release:

```bash
curl -fsSL https://raw.githubusercontent.com/Awassee/bluescreenjournal/main/install.sh | bash -s -- --version v1.3.1
```

Install from source instead:

```bash
curl -fsSL https://raw.githubusercontent.com/Awassee/bluescreenjournal/main/install.sh | bash -s -- --source
```

Troubleshooting helper actions:

```bash
curl -fsSL https://raw.githubusercontent.com/Awassee/bluescreenjournal/main/install.sh | bash -s -- --doctor
curl -fsSL https://raw.githubusercontent.com/Awassee/bluescreenjournal/main/install.sh | bash -s -- --repair-path
```

Local install from an extracted bundle or repo checkout:

```bash
./install.sh
```

The installer:

- downloads the latest public macOS release bundle automatically when needed
- installs the `bsj` binary
- installs docs and the man page
- installs Bash, Zsh, and Fish completions
- prints the exact `PATH` fix if needed
- points new users back into the in-app `HELP`, `SETUP`, and `TOOLS` menus instead of requiring CLI follow-up

The default public install path uses the prebuilt universal binary, so Rust and Cargo are not required.

## First Launch

Start the app:

```bash
bsj
bsj --version
```

If no vault exists, the TUI setup wizard asks for:

1. Vault path
2. Passphrase
3. Passphrase confirmation
4. Optional epoch date for `ENTRY NO.`

Default vault path:

```text
~/Documents/BlueScreenJournal
```

The setup wizard creates:

- `vault.json`
- `devices/<deviceId>.json`
- `entries/`

## First CLI Checks

After setup:

```bash
bsj settings
bsj doctor
bsj doctor --unlock
bsj verify
```

## Common Daily Tasks

Open today or a specific date:

```bash
bsj
bsj open 2026-03-16
```

Search without writing a plaintext on-disk index:

```bash
bsj search "quiet morning" --from 2026-03-01 --to 2026-03-31
```

Export:

```bash
bsj export 2026-03-16
bsj export 2026-03-16 --format markdown --output ~/Desktop/entry.md
```

Sync, backup, restore, verify:

```bash
bsj sync --backend folder --remote ~/Library/Mobile\ Documents/com~apple~CloudDocs/BlueScreenJournal
bsj backup
bsj backup list
bsj backup prune
bsj restore ~/Documents/BlueScreenJournal/backups/backup-20260316T120000Z.bsjbak.enc --into ~/Documents/BlueScreenJournal-Restore
bsj verify
```

Menu-first sync setup:

- `SETUP -> Cloud Provider Setup`
  - folder sync detection for Google Drive, Dropbox, OneDrive, iCloud Drive, and Box
  - direct Google Drive API mode
  - direct Dropbox API mode
- `SETUP -> Sync Backend Default`
  - blank/auto, `folder`, `s3`, `webdav`, `gdrive`, `dropbox`
- `SETUP -> Google Drive Folder ID`
  - saved non-secret default target for direct Google Drive API sync
- `SETUP -> Dropbox Root`
  - saved non-secret default target for direct Dropbox API sync

## TUI Keys

- `F1` help
- `F2` save revision
- `F3` date picker
- `F4` find
- `F5` global search
- `F6` replace
- `F7` index
- `F8` sync
- `F9` closing thought
- `F10` quit
- `F11` reveal codes
- `F12` lock
- `Ctrl+S` save fallback
- `Ctrl+F` find fallback

## Troubleshooting

- `bsj settings` shows effective config, vault path, and env status.
- `bsj doctor --unlock` verifies vault integrity and sync readiness.
- `bsj --debug` enables verbose logs without journal plaintext.
- Logs are written to `~/Library/Logs/bsj/bsj.log`.
- If the editor warns that the terminal is too small, resize to at least `80x25`.

## More Reference

- `docs/START_HERE.md`
- `docs/QUICKSTART.md`
- `bsj guide docs`
- `bsj guide quickstart`
- `bsj guide troubleshooting`
- `bsj guide sync`
- `bsj guide backup`
- `bsj guide macros`
- `bsj guide terminal`
- `bsj guide privacy`
- `bsj guide product`
- `bsj guide datasheet`
- `bsj guide faq`
- `bsj guide support`
- `bsj guide settings`
- `bsj guide distribution`
- `docs/PRODUCT_GUIDE.md`
- `docs/TROUBLESHOOTING.md`
- `docs/SYNC_GUIDE.md`
- `docs/BACKUP_RESTORE.md`
- `docs/MACRO_GUIDE.md`
- `docs/TERMINAL_GUIDE.md`
- `docs/PRIVACY.md`
- `docs/DATASHEET.md`
- `docs/FAQ.md`
- `docs/SETTINGS_GUIDE.md`
- `docs/DISTRIBUTION.md`
- `SUPPORT.md`
