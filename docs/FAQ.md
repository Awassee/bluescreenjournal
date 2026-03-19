# FAQ

## Does bsj store journal content in plaintext on disk?

That is explicitly not the design goal. Saved entries, autosave drafts, backups, and sync blobs are intended to be encrypted before write.

## What is stored in plaintext?

Operational metadata only.

Examples:

- `vault.json` metadata and KDF parameters
- device nickname files
- local config values you choose to store
- logs that intentionally avoid journal text and secrets

## Does bsj write a plaintext search index?

No. Global search is designed around an in-memory index after unlock.

## Can I use iCloud Drive or Dropbox?

Yes, through folder sync. The target should hold encrypted revision blobs plus required vault metadata.

## Do I need AWS or WebDAV to use bsj?

No. Folder sync is the simplest path. S3 and WebDAV are optional backends.

## Can I export plaintext?

Yes, intentionally. `bsj export` writes plaintext or markdown when you ask for it. That is different from silent plaintext persistence in the vault.

## Can I automate weekly reviews and reports?

Yes. Use JSON/CSV-capable commands:

- `bsj review --from YYYY-MM-DD --to YYYY-MM-DD --json`
- `bsj timeline --summary --format json`
- `bsj timeline --format csv`
- `bsj prompts pick --json`

## Is bsj trying to replace a PKM system?

No. It is intentionally narrower than a general note app or personal knowledge manager.

## Why the DOS-style `80x25` layout?

Focus. The product deliberately limits visual sprawl so the terminal behaves more like a writing appliance.

## Can I use it entirely from the keyboard?

Yes. The product is built around keyboard-only flow.

## What if the terminal window is too small?

Resize to at least `80x25`. The app warns instead of trying to render into an unusable space.

## Where should I start if I am stuck?

1. In the app: `TOOLS -> Doctor Report`
2. In the app: `SETUP -> Settings Summary`
3. Optional CLI diagnostics: `bsj doctor --unlock`
4. `SUPPORT.md`
