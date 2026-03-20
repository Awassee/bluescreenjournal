# Settings Guide

## Settings Surfaces

BlueScreen Journal uses four settings surfaces:

1. `config.json` for user-editable app settings
2. `vault.json` for vault metadata and KDF parameters
3. environment variables for CLI and network sync
4. CLI flags such as `--debug`

Inspect current effective settings:

```bash
bsj settings
bsj settings --json
```

Initialize or repair the config file:

```bash
bsj settings init
bsj settings init --force
```

Read and update supported settings:

```bash
bsj settings get vault_path
bsj settings set sync_target_path ~/Documents/BlueScreenJournal-Sync
bsj settings set clock_12h true
bsj settings set show_ruler false
bsj settings set backup_retention.daily 14
```

Most of these settings are also editable from inside the TUI under `SETUP`.

## config.json

Path on macOS:

```text
~/Library/Application Support/bsj/config.json
```

Editable keys:

- `vault_path`
  - type: string path
  - default: `~/Documents/BlueScreenJournal`
  - purpose: local vault root
- `sync_target_path`
  - type: string path or `null`
  - default: `null`
  - purpose: remembered folder sync target for `bsj sync --backend folder`
- `device_nickname`
  - type: string
  - default: `This Mac`
  - purpose: nickname stored in `devices/<deviceId>.json`
- `typewriter_mode`
  - type: boolean
  - default: `false`
  - purpose: keeps cursor vertically centered while writing
- `clock_12h`
  - type: boolean
  - default: `false`
  - purpose: switch the header clock to `AM/PM`
- `show_seconds`
  - type: boolean
  - default: `false`
  - purpose: include seconds in the header clock
- `show_ruler`
  - type: boolean
  - default: `true`
  - purpose: show or hide the DOS-style ruler above the editor
- `show_footer_legend`
  - type: boolean
  - default: `true`
  - purpose: show or hide the function-key legend in the footer
- `soundtrack_source`
  - type: string URL/path
  - default: `https://www.midi-karaoke.info/21b56501.mid`
  - purpose: soundtrack source used by `TOOLS -> Toggle Soundtrack`
- `opening_line_template`
  - type: string template (blank disables)
  - default: `JOURNAL ENTRY {DATE}`
  - purpose: auto-seeds a persistent opening line on blank entry pages
  - tokens: `{DATE}`, `{DATE_LONG}`, `{TIME}`, `{ENTRY_NO}`, `{DEVICE}`, `[TODAYSDATE]`
- `daily_word_goal`
  - type: integer or `null`
  - default: `null`
  - purpose: progress target shown in the footer/dashboard
- `remember_passphrase_in_keychain`
  - type: boolean
  - default: `false`
  - purpose: allows macOS Keychain-backed passphrase recall for faster unlock
- `backup_retention.daily`
  - type: integer
  - default: `7`
- `backup_retention.weekly`
  - type: integer
  - default: `4`
- `backup_retention.monthly`
  - type: integer
  - default: `6`

Readable but app-managed:

- `local_device_id`
  - type: string or `null`
  - purpose: app-managed local device identity
  - recommendation: do not edit by hand unless you are deliberately migrating device identity

Also present in `config.json`:

- `export_history`
  - type: array
  - purpose: recent plaintext export destinations and formats for in-product recall
- `search_presets`
  - type: array
  - purpose: named saved global-search queries and date ranges
  - recommended management: `bsj search --save-preset`, `--preset`, `--list-presets`, `--delete-preset`
- `macros`
  - type: array
  - purpose: key bindings for template insertion or internal commands

Supported macro key syntax examples:

- `ctrl-j`
- `alt-j`
- `shift-tab`
- `f13`
- `enter`
- `esc`
- `backspace`

Supported macro actions:

- `insert_template`
- `command`

Supported macro commands:

- `insert_date_header`
- `insert_closing_line`
- `jump_today`

Do not bind macros to reserved core keys such as `F1` through `F12`.

## vault.json

Path:

```text
<vault_path>/vault.json
```

Fields:

- `version`
  - app-managed vault format version
- `createdAt`
  - vault creation timestamp
- `deviceId`
  - root device id created during setup
- `kdf.algorithm`
  - currently `argon2id`
- `kdf.memoryKiB`
  - Argon2 memory cost
- `kdf.iterations`
  - Argon2 time cost
- `kdf.parallelism`
  - Argon2 parallelism
- `kdf.saltHex`
  - per-vault random salt
- `options.epochDate`
  - optional legacy/import continuity date stored in vault metadata
  - current `ENTRY NO.` values advance with each saved revision instead of using `epochDate`

`vault.json` is plaintext metadata only. Journal bodies are never stored there.

## Environment Variables

- `BSJ_PASSPHRASE`
  - optional passphrase source for CLI commands
  - if unset, CLI commands prompt securely
- `BSJ_SYNC_BACKEND`
  - optional default sync backend
  - values: `folder`, `s3`, `webdav`, `gdrive`, `dropbox`
- `BSJ_S3_BUCKET`
  - required for S3 sync unless `--remote s3://bucket/prefix` is provided
- `BSJ_S3_PREFIX`
  - optional S3 key prefix
- `AWS_REGION`
  - typical AWS region setting for S3 clients
- `BSJ_WEBDAV_URL`
  - required for WebDAV sync unless `--remote https://server/path/` is provided
- `BSJ_WEBDAV_USERNAME`
  - optional WebDAV username
- `BSJ_WEBDAV_PASSWORD`
  - required when `BSJ_WEBDAV_USERNAME` is set
- `BSJ_GDRIVE_ACCESS_TOKEN`
  - direct Google Drive API access token
- `BSJ_GDRIVE_REFRESH_TOKEN`
  - optional refresh token for direct Google Drive API sync
- `BSJ_GDRIVE_CLIENT_ID`
  - optional OAuth client id used with refresh flow
- `BSJ_GDRIVE_CLIENT_SECRET`
  - optional OAuth client secret used with refresh flow
- `BSJ_GDRIVE_FOLDER_ID`
  - optional direct Google Drive target folder id
  - default if unset: `appDataFolder`
- `BSJ_DROPBOX_ACCESS_TOKEN`
  - direct Dropbox API access token
- `BSJ_DROPBOX_REFRESH_TOKEN`
  - optional refresh token for direct Dropbox API sync
- `BSJ_DROPBOX_APP_KEY`
  - optional OAuth app key used with refresh flow
- `BSJ_DROPBOX_APP_SECRET`
  - optional OAuth app secret used with refresh flow
- `BSJ_DROPBOX_ROOT`
  - optional direct Dropbox root path
  - default if unset: `/BlueScreenJournal-Sync`

Secrets are not written into `vault.json`.

## Config-backed sync defaults

These config keys are designed for menu-driven setup and non-secret defaults:

- `sync_target_path`
  - default folder sync location
- `sync_backend_preference`
  - saved backend default used when CLI args and `BSJ_SYNC_BACKEND` are absent
  - values: blank/auto, `folder`, `s3`, `webdav`, `gdrive`, `dropbox`
- `gdrive_folder_id`
  - saved default direct Google Drive folder id
- `dropbox_root`
  - saved default direct Dropbox root path

Secrets for direct cloud connectors do not belong in config.
Use env vars and/or `SETUP -> Cloud Provider Setup -> Store ... Credentials`; in-product stored values go to macOS Keychain, not config.

## Diagnostics And Logging

Diagnostics:

```bash
bsj doctor
bsj doctor --unlock
bsj doctor --unlock --json
```

Debug logging:

```bash
bsj --debug
```

Log path:

```text
~/Library/Logs/bsj/bsj.log
```

Logs intentionally avoid journal plaintext and secrets.

## Example Config

See `docs/config.example.json` for a complete editable example.

## Installed Reference Files

Prebuilt bundle installs place reference files under the chosen prefix:

- docs under `<prefix>/share/doc/bsj`
- example config under `<prefix>/share/bsj/examples/config.example.json`
- man page under `<prefix>/share/man/man1/bsj.1`
- completions under `<prefix>/share/bash-completion/completions`, `<prefix>/share/zsh/site-functions`, and `<prefix>/share/fish/vendor_completions.d`
