# CLAUDE.md — AI Assistant Guide for BlueScreen Journal

BlueScreen Journal (`bsj`) is a macOS-only, encrypted journaling TUI application written in Rust. It renders a retro DOS-style blue screen interface and stores all content encrypted at rest using XChaCha20-Poly1305.

---

## Quick Reference

| Aspect | Details |
|--------|---------|
| Language | Rust 1.85+ (edition 2024) |
| Platform | macOS only (x86_64 + aarch64) |
| TUI framework | `ratatui` + `crossterm` |
| Encryption | XChaCha20-Poly1305 with Argon2id KDF |
| Build tool | `cargo` + `just` |
| Version | 1.0.2 |
| License | MIT |

---

## Repository Layout

```
src/
  main.rs          CLI entrypoint and command routing (959 lines)
  vault.rs         Core encryption & vault logic (2573 lines)
  config.rs        Configuration loading/saving (613 lines)
  search.rs        In-memory full-text search (396 lines)
  sync.rs          Multi-backend sync: folder/S3/WebDAV (1120 lines)
  help.rs          Embedded help and guide text (730 lines)
  doctor.rs        System diagnostics (426 lines)
  platform.rs      macOS-specific: keyboard, MIDI, keychain (385 lines)
  logging.rs       Debug logging setup (71 lines)
  secure_fs.rs     Atomic writes, secure temp file handling (191 lines)
  tui/
    app.rs         Main TUI state machine — largest file (10,274 lines)
    mod.rs         Crossterm event loop and render dispatch (1838 lines)
    buffer.rs      Text editor buffer with undo/redo (700 lines)
    calendar.rs    Calendar widget (122 lines)

docs/              15+ user-facing markdown guides
scripts/           Build, release, QA, and smoke-test scripts
packaging/         Homebrew formula and packaging configs
.github/workflows/ CI (ci.yml) and release (release.yml) pipelines
justfile           Task runner shortcuts
```

---

## Build and Development Commands

Use `just` as the primary task runner (wraps `cargo` commands):

```bash
just fmt           # cargo fmt --all
just clippy        # cargo clippy --all-targets -- -D warnings
just test          # cargo test --workspace --all-targets --all-features
just run           # cargo run
just qa            # Full QA gate: fmt + clippy + test + package + smoke test
just package       # Build host-arch release binary into dist/
just package-universal  # Build universal (Intel + ARM) macOS binary
just smoke-dist    # Run installer smoke test against dist/
just audit-release # Pre-release artifact validation
```

**Before opening a PR**, always run:
```bash
just qa
# or directly:
./scripts/qa-gate.sh
```

---

## CI/CD Pipeline

CI runs on **GitHub Actions with macOS-14 runners only** (not Linux, not Windows).

**ci.yml** (triggers on push to `main` and all PRs):
1. Install Rust stable + rustfmt + clippy
2. Run `./scripts/qa-gate.sh` (format check → clippy → tests → package → smoke)

**release.yml** (triggers on `v*` tags or manual dispatch):
1. Build universal macOS binary via `./scripts/package-release.sh --universal`
2. Run installer smoke test
3. Upload `.tar.gz` and `.sha256` artifacts
4. Publish GitHub Release

---

## Key Conventions

### Code Style
- **Formatter**: `cargo fmt --all` — enforced in CI, run before every commit
- **Linter**: `cargo clippy --all-targets -- -D warnings` — all warnings are errors
- No custom `.rustfmt.toml` or `.clippy.toml`; uses Rust defaults
- Use `thiserror` for error types; `Result<T, VaultError>` is the primary pattern
- Secrets must use `secrecy::SecretString` and be zeroized via `zeroize`

### Testing
- Tests live **inline** in source modules (no separate `tests/` directory)
- Every bug fix **requires a regression test** in the same PR — this is non-negotiable
- UX flow regression tests go in `src/tui/app.rs` under the `tui::app::tests` module
- Installer regressions go in `scripts/smoke-release-install.sh`
- Run specific test groups:
  ```bash
  cargo test keybinding_                 # Function key routing tests
  cargo test tui::app::tests             # UI flow regression tests
  ```

### Security — Critical Rules
- **Never write plaintext journal content to disk** — everything must go through the encryption layer in `vault.rs`
- **Never add a plaintext search index on disk** — the search index in `search.rs` is in-memory only
- Secrets must be cleared from memory using `zeroize` when done
- Raise the review bar for any changes touching: encryption, sync, backups, integrity verification
- The encrypted file format uses magic bytes `BSJE`; do not change the format without a migration path

### Architecture Patterns
- The TUI is a **single-file state machine** in `src/tui/app.rs` — all overlays (menu, editor, search, calendar, sync, settings) are variants of an `Overlay` enum
- Sync backends implement the `SyncBackend` trait; currently: `FolderBackend`, `S3Backend`, `WebDavBackend`
- macOS-specific code is isolated in `platform.rs` (keyboard, MIDI via `afplay`, keychain)
- Configuration is JSON-based at `~/.config/bsj/config.json`; the template is at `docs/config.example.json`

---

## Encryption Architecture

| Component | Detail |
|-----------|--------|
| Cipher | XChaCha20-Poly1305 (AEAD) |
| KDF | Argon2id — 3 iterations, 65 MB memory, 1 thread (configurable) |
| Nonce | 24-byte random per encryption |
| File format | `BSJE` magic bytes prefix |
| Vault structure | `vault.json` (encrypted metadata), `revisions/{DATE}-{SEQ}.enc`, `drafts/{DATE}.enc`, `backups/*.bsjbak.enc` |

All sync blobs are encrypted before leaving the device. The vault is unlocked in memory only; no passphrase or decrypted content is written to disk.

---

## Sync Backends

Three backends share a common `SyncBackend` trait interface:

| Backend | Config key | Notes |
|---------|-----------|-------|
| Folder | `sync_target_path` | Local folder or iCloud Drive mount |
| S3 | `sync_s3_bucket` | AWS SDK via `aws-sdk-s3` |
| WebDAV | `sync_webdav_url` | HTTP client via `reqwest` |

Sync uses an inventory-based approach: list all keys on both sides, push/pull diffs. Conflict detection uses `device_id` and per-entry metadata.

---

## Documentation Updates

When changing any user-facing feature or command:
1. Update the relevant file in `docs/`
2. Update embedded help text in `src/help.rs` if the keyboard reference or command list changes
3. Update `CHANGELOG.md` with a summary of changes

---

## PR Checklist

From `CONTRIBUTING.md`:

- [ ] `just qa` passes (or `./scripts/qa-gate.sh`)
- [ ] Regression test added for any bug fix
- [ ] Docs and `src/help.rs` updated if feature surface changed
- [ ] PR description explains: user problem, product reason, solution approach, what's tested, remaining risks
- [ ] Security-sensitive changes (crypto, sync, backup, integrity) noted explicitly for elevated review

---

## Common Pitfalls

- **Don't build for Linux** — this is macOS-only; CI uses `macos-14` runners
- **Don't persist plaintext** — any content written to disk must be encrypted via the vault layer
- **Don't add disk-based search indexes** — the search index must remain in-memory
- **Don't skip `just qa`** — CI will reject PRs that fail the QA gate
- **Don't modify `BSJE` file format** without adding a migration path in `vault.rs`
- **The `tui/app.rs` file is intentionally large** — don't extract overlays into separate files without a clear architectural reason; keep the state machine co-located

---

## Logging

Debug logs go to `~/Library/Logs/bsj/bsj.log`. Set up via `src/logging.rs`. Enable with `BSJ_LOG=debug bsj` (or equivalent env var).

---

## Release Process

1. Ensure `just qa` passes on a clean checkout
2. Update `Cargo.toml` version and `CHANGELOG.md`
3. Tag with `git tag v{VERSION}` and push the tag
4. GitHub Actions `release.yml` builds and publishes the release automatically
5. After release, verify artifacts with `./scripts/audit-release.sh`
