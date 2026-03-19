# Distribution Guide

## Release Formats

This repo supports two install paths:

1. turnkey bootstrap install from a one-line shell command
2. source install with Cargo

The prebuilt release bundle is the intended end-user distribution format, and the bootstrap installer is the intended public entry point.

For the user-facing product story and capability summary, pair this guide with:

- `docs/PRODUCT_GUIDE.md`
- `docs/DATASHEET.md`

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
curl -fsSL https://raw.githubusercontent.com/Awassee/bluescreenjournal/main/install.sh | bash -s -- --version v1.1.0
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
        v1.1.0.md
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

## Validate A Release Bundle

Smoke-test the bundle install:

```bash
./scripts/smoke-release-install.sh
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

## Install From A Release Bundle

After extracting the tarball:

```bash
./install.sh --prebuilt
```

Stable asset names published alongside versioned archives:

```text
bsj-universal-apple-darwin.tar.gz
bsj-universal-apple-darwin.tar.gz.sha256
```

Those stable names are what the turnkey installer downloads from GitHub Releases.

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
git tag v1.1.0
git push origin main --tags
```

## Release Checklist

1. Run `cargo fmt --all`
2. Run `cargo clippy --all-targets -- -D warnings`
3. Run `cargo test --all-targets`
4. Run `./scripts/package-release.sh --universal`
5. Run `./scripts/smoke-release-install.sh`
6. Run `./scripts/audit-release.sh`
7. Run `bsj guide distribution` from the built binary
8. Update release notes in `docs/releases/vX.Y.Z.md`
9. Update public docs surfaces (`README.md`, `docs/START_HERE.md`, issue templates) if needed
10. Push a `v*` tag to trigger `.github/workflows/release.yml`
11. Update the Homebrew formula URL if you publish a formula

## Notes

- The GitHub landing page should point readers to the product guide, datasheet, setup guide, and turnkey install command.
- The installer and bundled docs are part of the release surface, not optional extras.

- No plaintext journal content is included in release artifacts.
- Release bundles include docs, completions, examples, and license text, not user vault data.
- If you plan public redistribution, add explicit project licensing before publishing.
