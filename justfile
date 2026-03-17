fmt:
    cargo fmt --all

clippy:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

test:
    cargo test --workspace --all-targets --all-features

run:
    cargo run --

install:
    ./install.sh

package:
    ./scripts/package-release.sh

smoke-dist:
    ./scripts/smoke-release-install.sh
