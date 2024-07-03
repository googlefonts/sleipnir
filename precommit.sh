cargo test --all-targets --all-features;
cargo fmt;
cargo clippy --all-features --all-targets -- -D warnings;
cargo clippy --all-targets --no-default-features -- -D warnings;
cargo check --manifest-path=Cargo.toml --no-default-features;
cargo check --manifest-path=Cargo.toml;
