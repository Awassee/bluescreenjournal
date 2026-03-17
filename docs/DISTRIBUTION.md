# Distribution Guide

## Release Formats

This repo supports two install paths:

1. source install with Cargo
2. prebuilt release bundle install without Cargo

The prebuilt release bundle is the intended end-user distribution format.

## Release Bundle Layout

`./scripts/package-release.sh` creates:

```text
dist/
  bsj-<version>-<target>/
    README.md
    VERSION
    install.sh
    bin/
      bsj
    docs/
      SETUP_GUIDE.md
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

Optional output directory override:

```bash
./scripts/package-release.sh --output-dir /tmp/bsj-dist
```

## Validate A Release Bundle

Smoke-test the bundle install:

```bash
./scripts/smoke-release-install.sh
```

That script:

1. builds or reuses a release bundle
2. extracts it into a temp directory
3. runs the bundled installer in `--prebuilt` mode
4. verifies the installed binary, docs, man page, example config, and shell completions

## Install From A Release Bundle

After extracting the tarball:

```bash
./install.sh --prebuilt
```

Default prebuilt install target:

```text
~/.local/bin/bsj
```

The bundled installer also installs:

- docs under `~/.local/share/doc/bsj`
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

## Homebrew Formula

The packaging script renders a formula stub at:

```text
dist/bsj-<version>-<target>/packaging/homebrew/bsj.rb
```

You still need to replace:

- the release URL
- the project homepage

The SHA256 is filled in automatically from the generated tarball.

## Release Checklist

1. Run `cargo fmt --all`
2. Run `cargo clippy --all-targets -- -D warnings`
3. Run `cargo test --all-targets`
4. Run `./scripts/package-release.sh`
5. Run `./scripts/smoke-release-install.sh`
6. Run `bsj guide distribution` from the built binary
7. Upload the tarball and `.sha256` file
8. Update the Homebrew formula URL if you publish a formula

## Notes

- No plaintext journal content is included in release artifacts.
- Release bundles include docs, completions, and examples, not user vault data.
- If you plan public redistribution, add explicit project licensing before publishing.
