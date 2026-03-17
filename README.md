# bsj

BlueScreen Journal is a full-screen Rust TUI journal for macOS with a nostalgic blue-screen editing surface, encrypted local storage, append-only revisions, and encrypted sync targets.

## Quickstart

```bash
cargo run --
```

Useful local commands:

```bash
just fmt
just clippy
just test
just run
```

Useful CLI commands:

```bash
cargo run -- --help
cargo run -- --debug
cargo run -- export 2026-03-16
cargo run -- backup
```

## Vibe Features

- `F9` edits the dedicated `Closing Thought` field
- `F11` toggles `Reveal Codes`
- `F12` locks the vault, drops the in-memory key, wipes the in-memory search index, and returns to the passphrase prompt
- Reveal mode shows a retro metadata strip such as:
  - `⟦DATE:2026-03-16⟧ ⟦ENTRY:0000016⟧ ⟦TAG:work⟧ ⟦MOOD:7⟧ ⟦CLOSE:See you tomorrow.⟧`
- Closing thoughts are encrypted inside revisions and drafts, and `bsj export YYYY-MM-DD` prints them as the final line

## Logging And Debug

- `--debug` enables verbose file logging
- Log file: `~/Library/Logs/bsj/bsj.log`
- Logs intentionally avoid journal plaintext and secrets

Examples:

```bash
cargo run -- --debug
cargo run -- --debug backup
```

## Backups

`bsj backup` creates an encrypted snapshot under `vault/backups/`.

- Snapshot contents are tar+zstd in memory, then encrypted before writing
- Included data:
  - `vault.json`
  - `devices/*.json`
  - encrypted entry revisions
  - encrypted drafts
- Backups exclude the `backups/` directory itself
- Backup retention uses the app config:
  - `daily`
  - `weekly`
  - `monthly`

Example config file: `~/Library/Application Support/bsj/config.json`

```json
{
  "backup_retention": {
    "daily": 7,
    "weekly": 4,
    "monthly": 6
  }
}
```

Roundtrip restore is implemented in code and covered by tests. There is no public `bsj restore` CLI yet.

## Macros

Macros live in the same config file and map a key binding to either inserted text or an internal command.

Example:

```json
{
  "macros": [
    {
      "key": "ctrl-j",
      "type": "insert_template",
      "text": "TODAY I NOTICED:\n\n"
    },
    {
      "key": "ctrl-d",
      "type": "command",
      "command": "insert_date_header"
    },
    {
      "key": "ctrl-g",
      "type": "command",
      "command": "jump_today"
    },
    {
      "key": "ctrl-o",
      "type": "command",
      "command": "insert_closing_line"
    }
  ]
}
```

Supported internal commands:

- `insert_date_header`
- `insert_closing_line`
- `jump_today`

Avoid binding macros to reserved controls such as `F1`-`F12`, which are used by the core UI.

## Packaging

Install locally:

```bash
cargo install --path .
```

Optional Homebrew formula template:

```ruby
class Bsj < Formula
  desc "BlueScreen Journal terminal journal"
  homepage "https://example.com/bsj"
  url "https://example.com/bsj/archive/v0.1.0.tar.gz"
  sha256 "REPLACE_WITH_TARBALL_SHA256"
  license "MIT"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: ".")
  end

  test do
    assert_match "BlueScreen Journal", shell_output("#{bin}/bsj --help")
  end
end
```

## Sync Backends

`bsj sync` moves only encrypted revision blobs plus plaintext vault metadata:

- `vault.json`
- `devices/<deviceId>.json`
- `entries/YYYY/YYYY-MM-DD/rev-*.bsj.enc`

It never uploads plaintext journal bodies, drafts, or a plaintext search index.

### Folder Backend

Use this for iCloud Drive, Dropbox, Syncthing, or any shared folder.

```bash
cargo run -- sync --backend folder --remote ~/Library/Mobile\ Documents/com~apple~CloudDocs/BlueScreenJournal
```

If you omit `--backend`, folder mode is the default unless `--remote` looks like `s3://...` or `https://...`.

### S3 Backend

Configuration uses standard AWS credentials from the environment or your normal AWS config chain. No AWS secrets are written to `vault.json`.

Required environment:

```bash
export BSJ_SYNC_BACKEND=s3
export BSJ_S3_BUCKET=your-bucket
export BSJ_S3_PREFIX=bluescreenjournal
export AWS_REGION=us-east-1
```

Then run:

```bash
cargo run -- sync
```

Or override the bucket/prefix directly:

```bash
cargo run -- sync --backend s3 --remote s3://your-bucket/bluescreenjournal
```

### WebDAV Backend

Configuration uses environment variables only by default. Credentials are not stored in `vault.json`.

```bash
export BSJ_SYNC_BACKEND=webdav
export BSJ_WEBDAV_URL=https://dav.example.com/BlueScreenJournal/
export BSJ_WEBDAV_USERNAME=your-user
export BSJ_WEBDAV_PASSWORD=your-password
```

Then run:

```bash
cargo run -- sync
```

Or override the URL directly:

```bash
cargo run -- sync --backend webdav --remote https://dav.example.com/BlueScreenJournal/
```

## Security Notes

- Journal bodies are encrypted at rest with Argon2id + XChaCha20-Poly1305.
- Closing thoughts are encrypted inside the same revision and draft payloads.
- Sync transports move encrypted revision blobs only.
- Backup snapshots are encrypted before they hit disk.
- `vault.json` contains vault metadata and KDF parameters, not journal plaintext.
- `F12` locking drops the vault key and wipes in-memory editor/search state.
- Credentials are expected from environment variables. Local secret storage is not written into the vault format.

## Tests

Run the full local suite with:

```bash
cargo test --all-targets
```

S3 and WebDAV smoke tests are skipped unless the corresponding environment variables are present:

- S3: `BSJ_S3_BUCKET`
- WebDAV: `BSJ_WEBDAV_URL`

## Manual Smoke Test Checklist

Run these in both Terminal.app and iTerm2:

1. Launch at `80x25` or larger and confirm the blue full-screen editor appears with header, body, and footer strip.
2. Type a short entry, set a closing thought with `F9`, save with `F2`, quit, reopen, and confirm both persist.
3. Press `F11` and confirm Reveal Codes appears; press it again and confirm normal view returns.
4. Press `F12` and confirm the app returns to the passphrase prompt; unlock again and confirm the saved entry reloads.
5. Resize below `80x25` and confirm the warning screen appears; resize back and confirm editing resumes cleanly.
6. Run `cargo run -- backup`, confirm an encrypted file appears under `vault/backups/`, and confirm `rg` does not find plaintext journal text in that backup file.
7. Run with `--debug` and confirm `~/Library/Logs/bsj/bsj.log` is written without journal plaintext.
