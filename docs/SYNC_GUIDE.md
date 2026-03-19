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

Example:

```bash
bsj sync --backend folder --remote ~/Library/Mobile\ Documents/com~apple~CloudDocs/BlueScreenJournal
```

Good folder targets:

- iCloud Drive folders
- Dropbox folders
- other local sync folders that replicate files without rewriting contents

## S3 sync

Set environment variables:

```bash
export BSJ_SYNC_BACKEND=s3
export BSJ_S3_BUCKET=your-bucket
export BSJ_S3_PREFIX=bluescreenjournal
export AWS_REGION=us-east-1
bsj sync
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
