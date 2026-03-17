# Datasheet

## Product snapshot

- Product: BlueScreen Journal (`bsj`)
- Category: encrypted terminal journaling application
- Primary platform: macOS
- Runtime surface: Terminal.app and iTerm2
- UI style: full-screen blue-screen TUI with DOS-era `80x25` feel
- Language: Rust
- TUI stack: `ratatui` + `crossterm`

## Core value proposition

bsj delivers a focused, keyboard-only daily journal that feels like an old word processor while keeping journal content encrypted at rest and sync-safe.

## User-facing capabilities

| Capability | What it does | Why it matters |
| --- | --- | --- |
| Full-screen editor | Opens directly into today's entry with persistent command strip | Removes friction between launch and writing |
| Menu-driven TUI | `FILE / EDIT / SEARCH / GO / TOOLS / SETUP / HELP` | Makes the app learnable without memorizing hotkeys |
| Append-only revisions | Manual saves create new revisions instead of overwriting | Preserves history and supports safer recovery |
| Encrypted drafts | Autosave writes encrypted draft state | Protects unsaved work without plaintext leakage |
| Crash recovery | Prompts to recover a newer draft | Reduces data loss after interruption |
| Entry numbering | Stable `ENTRY NO.` from epoch date + entry date | Keeps numbering consistent across machines |
| Date index and calendar | Browse existing entries by list or month grid | Supports journal-style navigation |
| In-entry find/replace | Incremental find and retro replace flow | Speeds editing without leaving the keyboard |
| Global search | Searches saved entries without plaintext disk index | Fast retrieval without plaintext index files |
| Reveal Codes | Shows derived metadata inline | Preserves retro workflow and structural visibility |
| Closing Thought | Dedicated final line field | Encourages deliberate entry endings |
| Integrity verify | Checks per-date revision hashchains | Detects tampering and missing history |
| Encrypted backup | Creates encrypted snapshot archives | Supports offline recovery and retention |
| Encrypted sync | Syncs encrypted blobs to folder, S3, or WebDAV | Keeps storage backend unaware of journal plaintext |
| Lock command | Wipes unlocked state and returns to passphrase prompt | Improves safety on shared or visible terminals |

## Security characteristics

- Journal bodies are not intended to be written to disk in plaintext.
- Vault metadata is stored separately from journal content.
- Passphrase derivation uses Argon2id with per-vault salt.
- Entry and draft encryption use an AEAD file format.
- In-memory search index is rebuilt after unlock; plaintext index persistence is avoided.
- Backup artifacts are encrypted before write.
- Sync backends only receive encrypted revision blobs plus plaintext metadata required for vault operation.

## Vault layout

```text
~/Documents/BlueScreenJournal/
  vault.json
  entries/YYYY/YYYY-MM-DD/rev-<deviceId>-000001.bsj.enc
  entries/YYYY/YYYY-MM-DD/draft-<deviceId>.bsj.enc
  devices/<deviceId>.json
  backups/backup-YYYYMMDDTHHMMSSZ.bsjbak.enc
```

## Terminal assumptions

- minimum supported terminal size: `80x25`
- preferred feel: centered classic `80x25` screen even inside a larger terminal window
- color behavior: truecolor when available, fallback for `256color` and basic terminals

## Install surface

Turnkey public install:

```bash
curl -fsSL https://raw.githubusercontent.com/Awassee/bluescreenjournal/main/install.sh | bash
```

Default prebuilt install target:

```text
~/.local/bin/bsj
```

## Packaging surface

- versioned release archives
- stable public archive names for the bootstrap installer
- bundled docs, man page, example config, and shell completions
- GitHub Actions CI and release workflows

## Built-in guides

- `bsj guide docs`
- `bsj guide quickstart`
- `bsj guide setup`
- `bsj guide product`
- `bsj guide datasheet`
- `bsj guide faq`
- `bsj guide support`
- `bsj guide settings`
- `bsj guide distribution`

## Operational commands

- `bsj`
- `bsj open YYYY-MM-DD`
- `bsj search "query" --from YYYY-MM-DD --to YYYY-MM-DD`
- `bsj export YYYY-MM-DD`
- `bsj sync`
- `bsj backup`
- `bsj restore ... --into ...`
- `bsj verify`
- `bsj settings`
- `bsj doctor`

## Non-goals

- cloud account system
- shared real-time collaboration
- mobile-first experience
- rich-text desktop publishing
- plugin marketplace or arbitrary shell scripting by default
