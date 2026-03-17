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
- Sync transports move encrypted revision blobs only.
- `vault.json` contains vault metadata and KDF parameters, not journal plaintext.
- Credentials are expected from environment variables. Local secret storage is not written into the vault format.

## Tests

Run the full local suite with:

```bash
cargo test --all-targets
```

S3 and WebDAV smoke tests are skipped unless the corresponding environment variables are present:

- S3: `BSJ_S3_BUCKET`
- WebDAV: `BSJ_WEBDAV_URL`
