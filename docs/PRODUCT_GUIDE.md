# Product Guide

## What bsj is

BlueScreen Journal is a local-first encrypted journal for people who want the focus of an old full-screen word processor without giving up modern safety features.

In one sentence:
bsj is a blue-screen writing appliance for macOS terminals that keeps journal content encrypted at rest while preserving a fast keyboard-only flow.

Primary differentiators:

- always-on writing surface with menu discoverability instead of hidden command complexity
- append-only encrypted history rather than mutable plaintext files
- operational safety surfaces (sync, verify, backups, doctor) inside the same TUI

The product is built around four ideas:

1. Write immediately, without hunting for files or forms.
2. Keep journal content encrypted at rest, even when it syncs or backs up.
3. Preserve a distinctive blue-screen terminal feel instead of imitating a web app.
4. Keep history append-only so every save is recoverable and auditable.

## Who it is for

bsj fits people who:

- journal daily and want a keyboard-only flow
- prefer Terminal.app or iTerm2 over GUI note tools
- want private-at-rest storage instead of plaintext markdown folders
- care about revision history, recovery, and integrity verification
- like the spatial feel of DOS-era writing software

## Core product experience

When you launch `bsj`, the app opens as a centered blue-screen writing surface with a persistent header, menu bar, editor, and footer strip.

The intended loop is simple:

1. unlock once
2. start typing today's entry immediately
3. save revisions when you want milestones
4. let encrypted drafts protect unsaved work
5. jump by date, search, or index when you need to find older entries
6. lock the vault when you step away

## What the screen is doing

The TUI is organized like a classic word processor screen:

- header: current date/time, entry number, lock state, integrity state, save state
- menu bar: `FILE`, `EDIT`, `SEARCH`, `GO`, `TOOLS`, `SETUP`, `HELP`
- body: the active text buffer, plus optional Reveal Codes line and Closing Thought line
- footer: visible command strip with function-key shortcuts

The active workspace is intentionally constrained to a classic `80x25` style layout even on a larger terminal so it feels like a focused appliance rather than a fluid dashboard.

## First-use path

On the first launch, bsj opens an in-product setup wizard.

You choose:

1. vault path
2. passphrase
3. passphrase confirmation
4. optional legacy epoch date kept in vault metadata

After setup, the app creates an encrypted vault and opens directly into the selected day.

Default vault path:

```text
~/Documents/BlueScreenJournal
```

## Daily workflow

### Start writing

- Launch `bsj`
- Unlock the vault
- Type immediately into the editor

### Save with intent

- `F2` or `FILE -> Save Entry` writes a new append-only encrypted revision
- `**save**` on its own line saves and opens the next same-day entry on a clean page
- `Alt+N` opens the next blank day when you want to move forward to a new date
- autosave writes an encrypted draft every few seconds without creating revision spam

### Move through time

- `F3` or `GO -> Open Calendar` opens older dates intentionally
- `F7` or `GO -> Index Timeline` shows saved entry dates with previews for archive browsing
- `GO -> Jump to Today` returns to the current date

### Find and search

- `F4` or `EDIT -> Find in Entry` searches the current entry incrementally
- `F6` or `EDIT -> Replace in Entry` runs retro-style replace confirmation
- `F5` or `SEARCH -> Search Vault` searches across saved entries after unlock

### Review and timeline analytics

- `bsj review --from YYYY-MM-DD --to YYYY-MM-DD` scopes metrics to a period
- `bsj review --json` emits automation-friendly output
- `bsj timeline --format json|csv` supports script and spreadsheet workflows
- `bsj timeline --summary` returns aggregate counts, date range, and mood distribution
- timeline metadata filters (`--mood`, `--has-tags`, `--has-people`, `--has-project`, `--weekday`) make retrospective workflows precise

### Sync and verify

- `F8` or `TOOLS -> Sync Vault` syncs encrypted revision blobs
- `TOOLS -> Verify Integrity` checks the revision hashchain

### Lock when finished

- `F12` or `FILE -> Lock Vault` clears in-memory state and returns to the passphrase prompt

## Feature guide

### Encrypted vault

Value:
- journal content is not stored as plaintext on disk
- sync and backup targets only receive encrypted blobs

### Append-only revisions

Value:
- each intentional save creates history instead of overwriting the past
- conflict handling is safer because previous revisions remain available

### Encrypted autosave drafts and recovery

Value:
- crash recovery is built in
- unsaved work is protected without leaving plaintext behind

### Entry numbers

Value:
- each saved revision advances `ENTRY NO.`
- a fresh clean page shows the next serial before you save
- revision ordering stays deterministic across devices using the same vault

### Index and calendar navigation

Value:
- you can browse the vault as a journal, not just as a set of files
- date-based retrieval stays fast and predictable

### Global search with no plaintext disk index

Value:
- quick search after unlock
- avoids leaving a plaintext search database behind on disk

### Review/timeline JSON surfaces

Value:
- creates low-friction reporting workflows for weekly reviews and ops dashboards
- keeps analytics in the same toolchain without creating external plaintext indexes

### Folder, S3, and WebDAV sync

Value:
- you can keep the same encrypted vault shape across multiple storage backends
- folder sync works with iCloud Drive, Dropbox, Syncthing-style folders, and similar tools

### Integrity hashchain verification

Value:
- detects missing or tampered revisions
- gives the journal a verifiable internal history rather than best-effort trust

### Reveal Codes mode

Value:
- exposes metadata inline without changing the writing-first normal view
- preserves the retro workflow feel for users who like structural visibility

### Menu-driven TUI

Value:
- keeps the product learnable from inside the writing surface
- reduces reliance on memorized hotkeys while preserving keyboard speed

### Closing Thought

Value:
- gives entries a deliberate ending line without cluttering the main body
- exports cleanly as a final sentence or line

### Macros

Value:
- common repeated text or navigation can stay keyboard-only
- keeps the workflow fast without turning the app into a scripting surface

## Menus and keys

Menus are the discoverable surface.

- `Esc` opens the nostalgic menu bar
- arrows move between menus and menu items
- `Enter` triggers the selected command
- function keys remain available as direct shortcuts

Important shortcuts:

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

## Security model in practical terms

bsj is local-first, not cloud-first.

In practice that means:

- your vault is the source of truth
- `vault.json` stores metadata and KDF parameters, not journal plaintext
- entry content, drafts, and backups are encrypted before they touch disk
- search indexes are built in memory after unlock
- secrets are not intended to persist in logs or plaintext config

## Limits to understand

bsj is intentionally not trying to be everything.

It is not:

- a collaborative shared editor
- a mobile app
- a rich-text layout engine
- a cloud service with server-side accounts
- a markdown PKM system with backlinks, canvases, and plugins

## Recommended repo docs

- `docs/START_HERE.md` for the doc map
- `docs/QUICKSTART.md` for the fastest onboarding path
- `README.md` for install and quick orientation
- `docs/SETUP_GUIDE.md` for onboarding
- `docs/TROUBLESHOOTING.md` for failure recovery and operator fixes
- `docs/SYNC_GUIDE.md` for encrypted sync operations
- `docs/BACKUP_RESTORE.md` for backup drills and restore procedure
- `docs/TERMINAL_GUIDE.md` for Terminal.app and iTerm2 setup
- `docs/PRIVACY.md` for the plaintext boundary and operator precautions
- `docs/MACRO_GUIDE.md` for safe workflow acceleration
- `docs/FAQ.md` for common product and security questions
- `docs/COMPARE.md` for product-fit decisions
- `docs/SETTINGS_GUIDE.md` for config and environment details
- `docs/DATASHEET.md` for the concise capability sheet
- `docs/DISTRIBUTION.md` for release packaging and installer behavior
- `ROADMAP.md` for the planned product direction
- `CONTRIBUTING.md` for contributor workflow
