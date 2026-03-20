# Sync Guide

bsj is local-first. Sync is there to move encrypted revision blobs between places, not to turn the app into a server product.

## What sync moves

Sync is designed to move:

- `vault.json`
- `devices/<deviceId>.json`
- encrypted `entries/.../rev-*.bsj.enc`

It is not supposed to upload plaintext journal bodies.

## Folder sync

Folder sync is the simplest option and the best place to start.

Menu-first path:

- `SETUP -> Cloud Provider Setup`
- choose a provider folder preset
- verify from `TOOLS -> Sync Center` and `TOOLS -> Cloud Status`

Example:

```bash
bsj sync --backend folder --remote ~/Library/Mobile\ Documents/com~apple~CloudDocs/BlueScreenJournal
```

Good folder targets:

- iCloud Drive folders
- Dropbox folders
- other local sync folders that replicate files without rewriting contents

Provider presets (auto-detect desktop sync folders):

```bash
bsj sync --provider google-drive
bsj sync --provider dropbox
bsj sync --provider onedrive
bsj sync --provider icloud
bsj sync --provider box
```

You can set explicit provider target overrides if detection misses your custom folder:

```bash
export BSJ_GOOGLE_DRIVE_TARGET="$HOME/Library/CloudStorage/GoogleDrive-your@email/My Drive/BlueScreenJournal-Sync"
export BSJ_DROPBOX_TARGET="$HOME/Library/CloudStorage/Dropbox/BlueScreenJournal-Sync"
export BSJ_ONEDRIVE_TARGET="$HOME/Library/CloudStorage/OneDrive-Work/BlueScreenJournal-Sync"
export BSJ_ICLOUD_TARGET="$HOME/Library/Mobile Documents/com~apple~CloudDocs/BlueScreenJournal-Sync"
export BSJ_BOX_TARGET="$HOME/Library/CloudStorage/Box-Box/BlueScreenJournal-Sync"
```

## S3 sync

Set environment variables:

```bash
export BSJ_SYNC_BACKEND=s3
export BSJ_S3_BUCKET=your-bucket
export BSJ_S3_PREFIX=bluescreenjournal
export AWS_REGION=us-east-1
bsj sync
```

## Direct Google Drive API sync

Use this backend when you want bsj to talk to Google Drive directly (instead of syncing through a local Google Drive folder).

Menu-first path:

- `SETUP -> Cloud Provider Setup -> Use Direct Google Drive API`
- `SETUP -> Sync Backend Default -> gdrive`
- optional: `SETUP -> Google Drive Folder ID`
- `SETUP -> Cloud Provider Setup -> Store Google Drive Credentials`
- secrets stay out of config; use env vars and/or macOS Keychain

```bash
export BSJ_SYNC_BACKEND=gdrive
export BSJ_GDRIVE_ACCESS_TOKEN=ya29...
# Optional refresh flow:
export BSJ_GDRIVE_REFRESH_TOKEN=1//...
export BSJ_GDRIVE_CLIENT_ID=...
export BSJ_GDRIVE_CLIENT_SECRET=...
# Optional: folder target (default is appDataFolder)
export BSJ_GDRIVE_FOLDER_ID=appdata
bsj sync
```

If you prefer a full in-product setup flow, store the same credential fields in `SETUP -> Cloud Provider Setup -> Store Google Drive Credentials`. bsj saves them in macOS Keychain and uses env vars only as per-field overrides.

You can also pass a remote override:

```bash
bsj sync --backend gdrive --remote gdrive://<folder-id>
```

## Direct Dropbox API sync

Use this backend when you want bsj to talk to Dropbox directly (instead of syncing through a local Dropbox folder).

Menu-first path:

- `SETUP -> Cloud Provider Setup -> Use Direct Dropbox API`
- `SETUP -> Sync Backend Default -> dropbox`
- optional: `SETUP -> Dropbox Root`
- `SETUP -> Cloud Provider Setup -> Store Dropbox Credentials`
- secrets stay out of config; use env vars and/or macOS Keychain

```bash
export BSJ_SYNC_BACKEND=dropbox
export BSJ_DROPBOX_ACCESS_TOKEN=sl....
# Optional refresh flow:
export BSJ_DROPBOX_REFRESH_TOKEN=...
export BSJ_DROPBOX_APP_KEY=...
export BSJ_DROPBOX_APP_SECRET=...
# Optional root path (default /BlueScreenJournal-Sync)
export BSJ_DROPBOX_ROOT=/BlueScreenJournal-Sync
bsj sync
```

If you prefer a full in-product setup flow, store the same credential fields in `SETUP -> Cloud Provider Setup -> Store Dropbox Credentials`. bsj saves them in macOS Keychain and uses env vars only as per-field overrides.

You can also pass a remote override:

```bash
bsj sync --backend dropbox --remote dropbox:///BlueScreenJournal-Sync
```

## WebDAV sync

Set environment variables:

```bash
export BSJ_SYNC_BACKEND=webdav
export BSJ_WEBDAV_URL=https://dav.example.com/BlueScreenJournal/
export BSJ_WEBDAV_USERNAME=your-user
export BSJ_WEBDAV_PASSWORD=your-password
bsj sync
```

`https://` is the default requirement.

If you are testing against a trusted local lab server and explicitly want insecure HTTP anyway, you can opt in:

```bash
export BSJ_WEBDAV_ALLOW_INSECURE_HTTP=1
```

## How reconciliation works

## Backend selection order

bsj resolves sync backend in this order:

1. explicit CLI `--backend`
2. `BSJ_SYNC_BACKEND`
3. saved `sync_backend_preference`
4. inferred remote scheme
5. folder sync fallback

For direct Google Drive and Dropbox, target selection follows the same spirit:

1. explicit CLI `--remote`
2. env target override
3. saved Setup default
4. built-in default target

At a high level, sync does this:

1. pull remote revisions missing locally
2. reconcile heads per date
3. detect conflicts when heads diverge
4. push local revisions missing remotely

## Conflict model

Conflicts are preserved, not flattened away.

That is intentional.

If two devices create competing heads for the same date, bsj keeps both until you resolve them with the merge flow.

## Recommended operating pattern

1. unlock locally
2. save before switching devices
3. sync on one device
4. sync on the other device
5. resolve conflicts explicitly if they exist
6. run `bsj verify` occasionally

## Avoid these anti-patterns

- manually renaming or deleting revision files in a synced vault
- editing `vault.json` by hand unless you know exactly why
- sharing one live vault directory between tools that rewrite files unpredictably
- treating conflict warnings as safe to ignore forever

## After a sync problem

Run:

```bash
bsj sync
bsj verify
bsj doctor --unlock
bsj sysop sync-preview --backend folder --remote ~/Documents/BlueScreenJournal-Sync
```

Then inspect the affected date through the TUI.
