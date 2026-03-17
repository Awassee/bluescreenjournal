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
bsj settings set backup_retention.daily 14
```

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
  - user-chosen entry numbering epoch
  - `ENTRY NO.` is derived as days between `epochDate` and the entry date, plus one

`vault.json` is plaintext metadata only. Journal bodies are never stored there.

## Environment Variables

- `BSJ_PASSPHRASE`
  - optional passphrase source for CLI commands
  - if unset, CLI commands prompt securely
- `BSJ_SYNC_BACKEND`
  - optional default sync backend
  - values: `folder`, `s3`, `webdav`
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

Secrets are not written into `vault.json`.

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
