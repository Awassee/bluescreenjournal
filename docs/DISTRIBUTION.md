# Distribution Guide

## Release Formats

This repo supports two install paths:

1. turnkey bootstrap install from a one-line shell command
2. source install with Cargo

The prebuilt release bundle is the intended end-user distribution format, and the bootstrap installer is the intended public entry point.

On macOS, the bootstrap installer now resolves the best published release asset for the current machine instead of assuming a single universal archive name.

For the user-facing product story and capability summary, pair this guide with:

- `docs/PRODUCT_GUIDE.md`
- `docs/DATASHEET.md`
- `docs/RELEASE_CERTIFICATION.md`

## Turnkey Public Install

Users should be able to paste:

```bash
curl -fsSL https://raw.githubusercontent.com/Awassee/bluescreenjournal/main/install.sh | bash
```

That bootstrap installer:

1. downloads the latest public macOS release bundle
2. verifies the `.sha256` file when present
3. extracts the bundle into a temp directory
4. runs the bundled `install.sh --prebuilt`

Pin a specific release:

```bash
curl -fsSL https://raw.githubusercontent.com/Awassee/bluescreenjournal/main/install.sh | bash -s -- --version v2.2.0
```

## Release Bundle Layout

`./scripts/package-release.sh` creates:

```text
dist/
  bsj-<version>-<target>/
    README.md
    LICENSE
    CHANGELOG.md
    SUPPORT.md
    SECURITY.md
    CONTRIBUTING.md
    ROADMAP.md
    VERSION
    TARGETS
    install.sh
    bin/
      bsj
    docs/
      assets/
        bsj-hero.gif
        bsj-editor.png
        bsj-search.png
        ...
      releases/
        v2.2.0.md
      SETUP_GUIDE.md
      PRODUCT_GUIDE.md
      DATASHEET.md
      START_HERE.md
      QUICKSTART.md
      FAQ.md
      COMPARE.md
      TROUBLESHOOTING.md
      SYNC_GUIDE.md
      BACKUP_RESTORE.md
      MACRO_GUIDE.md
      TERMINAL_GUIDE.md
      PRIVACY.md
      SETTINGS_GUIDE.md
      DISTRIBUTION.md
      bsj.1
      config.example.json
    completions/
      bash/
        bsj
      zsh/
        _bsj
      fish/
        bsj.fish
    packaging/
      homebrew/
        bsj.rb
        bsj.rb.template
  bsj-<version>-<target>.tar.gz
  bsj-<version>-<target>.tar.gz.sha256
  bsj-<target>.tar.gz
  bsj-<target>.tar.gz.sha256
```

## Build A Release Bundle

From the repo root:

```bash
./scripts/package-release.sh
```

Optional target override:

```bash
./scripts/package-release.sh --target aarch64-apple-darwin
```

Universal macOS bundle:

```bash
./scripts/package-release.sh --universal
```

Local requirement:

- both `aarch64-apple-darwin` and `x86_64-apple-darwin` targets must be installed
- if `rustup` is available, the script installs missing targets automatically

Optional output directory override:

```bash
./scripts/package-release.sh --output-dir /tmp/bsj-dist
```

Recommended free disk space before a universal packaging run:

- at least `3-5 GiB`

That avoids false-negative failures during `lipo`, archive creation, and installer smoke temp extraction.

## Validate A Release Bundle

Smoke-test the bundle install:

```bash
./scripts/smoke-release-install.sh
```

Smoke-test the public one-line installer against a pushed ref or tag:

```bash
./scripts/smoke-public-install.sh --ref v2.2.0 --version v2.2.0
```

Write a clean-account certification report for the release:

```bash
./scripts/certify-release.sh --ref v2.2.0 --version v2.2.0 --report docs/certification/v2.2.0.md
```

Run the release privacy audit directly:

```bash
./scripts/audit-release.sh
```

That script:

1. builds or reuses a release bundle
2. extracts it into a temp directory
3. runs the bundled installer in `--prebuilt` mode
4. runs the top-level piped installer path with `--archive`
5. verifies the installed binary, docs, man page, example config, shell completions, and binary privacy audit

The public installer smoke verifies:

1. the raw GitHub installer can be fetched from the chosen ref
2. the installer selects a valid published release asset
3. the install completes in a clean temp `HOME`
4. the installed binary reports the expected version
5. shell profile PATH integration is written and the misleading PATH warning does not appear

The release certification script adds:

1. `bsj --help` guide visibility checks
2. `bsj guide cheatsheet` rendering checks
3. a commit-friendly Markdown report for the release line

## Visual Artifact Review

CI and release workflows publish a nostalgia artifact bundle.

Reviewers should open:

```text
artifacts/tui-snapshots/index.html
```

That preview compares expected vs actual blue-screen snapshots for the most important nostalgic UI states.

## Install From A Release Bundle

After extracting the tarball:

```bash
./install.sh --prebuilt
```

Stable asset names published alongside versioned archives:

```text
bsj-aarch64-apple-darwin.tar.gz
bsj-aarch64-apple-darwin.tar.gz.sha256
bsj-x86_64-apple-darwin.tar.gz
bsj-x86_64-apple-darwin.tar.gz.sha256
bsj-universal-apple-darwin.tar.gz
bsj-universal-apple-darwin.tar.gz.sha256
```

The turnkey installer chooses the matching stable asset for the current Mac and falls back to a universal archive if one is published.

Default prebuilt install target:

```text
~/.local/bin/bsj
```

The bundled installer also installs:

- docs under `~/.local/share/doc/bsj`
- packaged root docs such as `README.md`, `CHANGELOG.md`, `SUPPORT.md`, `SECURITY.md`, `CONTRIBUTING.md`, and `ROADMAP.md`
- example config under `~/.local/share/bsj/examples`
- man page under `~/.local/share/man/man1`
- Bash completions under `~/.local/share/bash-completion/completions`
- Zsh completions under `~/.local/share/zsh/site-functions`
- Fish completions under `~/.local/share/fish/vendor_completions.d`

Override the install prefix if needed:

```bash
./install.sh --prebuilt --prefix "$HOME/.bsj"
```

You can also override the individual completion install directories:

```bash
./install.sh --prebuilt \
  --bash-completion-dir "$HOME/.local/share/bash-completion/completions" \
  --zsh-completion-dir "$HOME/.local/share/zsh/site-functions" \
  --fish-completion-dir "$HOME/.local/share/fish/vendor_completions.d"
```

## Source Install

From a checkout:

```bash
./install.sh --source
```

The source installer uses `cargo install --path . --locked --force` and also generates shell completions under the selected prefix.

## Marketing Assets

Product screenshots and the README hero GIF are generated from:

```text
scripts/render_marketing_assets.py
```

The generated files live under:

```text
docs/assets/
```

Release bundles and prebuilt installs preserve that doc asset tree so the packaged README keeps its screenshots intact.

## Homebrew Formula

The packaging script renders a formula stub at:

```text
dist/bsj-<version>-<target>/packaging/homebrew/bsj.rb
```

You still need to replace:

- the release URL
- the project homepage

The SHA256 is filled in automatically from the generated tarball.

## GitHub Actions

CI workflow:

```text
.github/workflows/ci.yml
```

- runs `./scripts/qa-gate.sh` (format, clippy, tests, package, smoke install)

Release workflow:

```text
.github/workflows/release.yml
```

- triggers on pushed `v*` tags
- builds a universal macOS bundle
- smoke-tests the bundled installer
- uploads `.tar.gz` and `.sha256` assets to the GitHub Release

Release automation flow:

```bash
git tag v2.2.0
git push origin main --tags
```

## Release Checklist

1. Run `cargo fmt --all`
2. Run `cargo clippy --all-targets -- -D warnings`
3. Run `cargo test --all-targets`
4. Run `./scripts/package-release.sh --universal`
5. Run `./scripts/smoke-release-install.sh`
6. Run `./scripts/smoke-public-install.sh --ref <tag> --version <tag>`
6. Run `./scripts/manual-smoke-gui-terminals.sh <version>`
7. Run `./scripts/audit-release.sh`
8. Run `bsj guide distribution` from the built binary
9. Update release notes in `docs/releases/vX.Y.Z.md`
10. Update public docs surfaces (`README.md`, `docs/START_HERE.md`, issue templates) if needed
11. Push a `v*` tag to trigger `.github/workflows/release.yml`
12. Update the Homebrew formula URL if you publish a formula

## Notes

- The GitHub landing page should point readers to the product guide, datasheet, setup guide, and turnkey install command.
- The installer and bundled docs are part of the release surface, not optional extras.

- No plaintext journal content is included in release artifacts.
- Release bundles include docs, completions, examples, and license text, not user vault data.
- If you plan public redistribution, add explicit project licensing before publishing.
