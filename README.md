# bsj

BlueScreen Journal is a full-screen Rust TUI journal for macOS with a nostalgic blue-screen editor, encrypted local storage, append-only revisions, encrypted drafts, encrypted backups, and encrypted sync targets.

## Install

Prebuilt release bundle:

```bash
./install.sh --prebuilt
```

Source install from a checkout:

```bash
./install.sh --source
```

Direct Cargo fallback:

```bash
cargo install --path . --locked --force
```

Installer behavior:

- installs `bsj`
- installs docs and man page
- installs shell completions for Bash, Zsh, and Fish
- prints the exact `PATH` fix if your bin dir is not already on `PATH`

Default install locations:

- prebuilt: `~/.local/bin/bsj`
- source via installer: `~/.cargo/bin/bsj`
- docs: `<prefix>/share/doc/bsj`
- man page: `<prefix>/share/man/man1/bsj.1`

## First Run

Launch the app:

```bash
bsj
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

## Built-In Help

```bash
bsj --help
bsj guide setup
bsj guide settings
bsj guide distribution
bsj settings
bsj settings --json
bsj doctor
bsj doctor --unlock
```

## Daily Commands

```bash
bsj
bsj open 2026-03-16
bsj search "quiet morning" --from 2026-03-01 --to 2026-03-31
bsj export 2026-03-16
bsj export 2026-03-16 --format markdown --output ~/Desktop/entry.md
bsj sync --backend folder --remote ~/Library/Mobile\ Documents/com~apple~CloudDocs/BlueScreenJournal
bsj backup
bsj backup list
bsj backup prune
bsj backup prune --apply
bsj restore ~/Documents/BlueScreenJournal/backups/backup-20260316T120000Z.bsjbak.enc --into ~/Documents/BlueScreenJournal-Restore
bsj verify
```

## TUI Keys

- `F1` help
- `F2` save revision
- `F3` date picker
- `F4` incremental find
- `F5` global search
- `F6` replace with `Y/N/A/Q`
- `F7` index
- `F8` sync
- `F9` closing thought
- `F10` quit
- `F11` reveal codes
- `F12` lock
- `Ctrl+S` save fallback
- `Ctrl+F` find fallback

## Settings And Diagnostics

Config file:

```text
~/Library/Application Support/bsj/config.json
```

Useful settings commands:

```bash
bsj settings init
bsj settings get vault_path
bsj settings set sync_target_path ~/Documents/BlueScreenJournal-Sync
bsj settings set backup_retention.daily 14
```

Useful diagnostics commands:

```bash
bsj doctor
bsj doctor --unlock
bsj doctor --unlock --json
```

Editable settings:

- `vault_path`
- `sync_target_path`
- `device_nickname`
- `backup_retention.daily`
- `backup_retention.weekly`
- `backup_retention.monthly`

App-managed settings:

- `local_device_id`
- `vault.json` metadata and KDF parameters

Environment variables:

- `BSJ_PASSPHRASE`
- `BSJ_SYNC_BACKEND`
- `BSJ_S3_BUCKET`
- `BSJ_S3_PREFIX`
- `AWS_REGION`
- `BSJ_WEBDAV_URL`
- `BSJ_WEBDAV_USERNAME`
- `BSJ_WEBDAV_PASSWORD`

## Sync

`bsj sync` transfers only:

- `vault.json`
- `devices/<deviceId>.json`
- encrypted `entries/.../rev-*.bsj.enc`

It does not upload plaintext journal bodies, drafts, or a plaintext search index.

Folder sync:

```bash
bsj sync --backend folder --remote ~/Library/Mobile\ Documents/com~apple~CloudDocs/BlueScreenJournal
```

S3 sync:

```bash
export BSJ_SYNC_BACKEND=s3
export BSJ_S3_BUCKET=your-bucket
export BSJ_S3_PREFIX=bluescreenjournal
export AWS_REGION=us-east-1
bsj sync
```

WebDAV sync:

```bash
export BSJ_SYNC_BACKEND=webdav
export BSJ_WEBDAV_URL=https://dav.example.com/BlueScreenJournal/
export BSJ_WEBDAV_USERNAME=your-user
export BSJ_WEBDAV_PASSWORD=your-password
bsj sync
```

## Backups

`bsj backup` creates an encrypted snapshot under:

```text
<vault>/backups/
```

Inspect retention and prune behavior:

```bash
bsj backup list
bsj backup prune
bsj backup prune --apply
```

## Completions

Generate completions on demand:

```bash
bsj completions bash
bsj completions zsh
bsj completions fish
```

The installer also places completion files under the install prefix.

## Logging

- `--debug` enables verbose file logging
- log path: `~/Library/Logs/bsj/bsj.log`
- logs intentionally avoid journal plaintext and secrets

Examples:

```bash
bsj --debug
bsj --debug sync --backend folder --remote ~/Documents/BlueScreenJournal-Sync
```

## Distribution

Build a host-architecture release bundle:

```bash
./scripts/package-release.sh
```

Build a universal macOS release bundle:

```bash
./scripts/package-release.sh --universal
```

Note:

- local universal builds require both `aarch64-apple-darwin` and `x86_64-apple-darwin` targets
- the GitHub release workflow installs both targets automatically

Smoke-test the release bundle install:

```bash
./scripts/smoke-release-install.sh
```

Run the release privacy audit directly:

```bash
./scripts/audit-release.sh
```

Artifacts:

- `dist/bsj-<version>-<target>/`
- `dist/bsj-<version>-<target>.tar.gz`
- `dist/bsj-<version>-<target>.tar.gz.sha256`
- `dist/bsj-<version>-<target>/packaging/homebrew/bsj.rb`

Automation:

- `.github/workflows/ci.yml` runs lint, tests, packaging, smoke install, and the release audit
- `.github/workflows/release.yml` builds a universal macOS bundle and publishes it on pushed `v*` tags

## Reference Docs

- `docs/SETUP_GUIDE.md`
- `docs/SETTINGS_GUIDE.md`
- `docs/DISTRIBUTION.md`
- `docs/config.example.json`
- `docs/bsj.1`

## Manual Smoke Checklist

Terminal.app:

1. Open a window at least `80x25`.
2. Launch `bsj`.
3. Verify the blue-screen layout, visible footer strip, and block cursor behavior.
4. Save a revision, lock with `F12`, unlock again, and confirm the entry reloads.
5. Trigger `F3`, `F5`, `F7`, `F8`, `F11`, and `F12`.

iTerm2:

1. Launch `bsj`.
2. Resize below `80x25` and confirm the warning screen appears without panic.
3. Resize back up and confirm the editor redraws cleanly.
4. Verify function keys and `Ctrl+S` / `Ctrl+F` fallbacks work.

## Development

```bash
just fmt
just clippy
just test
just audit-release
just package
just package-universal
just smoke-dist
```
