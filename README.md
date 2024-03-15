# Sleipnir

[![Build Status](https://github.com/googlefonts/sleipnir/actions/workflows/rust.yml/badge.svg)](https://github.com/googlefonts/sleipnir/actions/workflows/rust.yml)
[![Docs](https://docs.rs/sleipnir/badge.svg)](https://docs.rs/sleipnir)
[![Crates.io](https://img.shields.io/crates/v/sleipnir.svg?maxAge=2592000)](https://crates.io/crates/sleipnir)

## Name?

The name is a reference to [Sleipnir](https://en.wikipedia.org/wiki/Sleipnir), in keeping with other Norse names for our memory safe stuff.

## releasing

_copied from [fontations](https://github.com/googlefonts/fontations)_

We use [`cargo-release`] to help guide the release process. It can be installed
with `cargo install cargo-release`. You may need to install `pkg-config` via your
package manager for this to work.

Releasing involves the following steps:

1. Determine which crates may need to be published: run `cargo release changes`
   to see which crates have been modified since their last release.
1. Determine the new versions for the crates.
   * Before 1.0, breaking changes bump the *minor* version number, and non-breaking changes modify the *patch* number.
1. Update manifest versions and release. `./resources/scripts/bump-version.sh` orchestrates this process.
   * `cargo release` does all the heavy lifting

   ```shell
   # To see usage
   ./resources/scripts/bump-version.sh
   # To do the thing
   ./resources/scripts/bump-version.sh  sleipnir patch
   ```

1. Commit these changes to a new branch, get it approved and merged, and switch
   to the up-to-date `main`.
1. Publish the crates. `./resources/scripts/release.sh` orchestrates the process.
   * You will be prompted to review changes along the way

   ```shell
   # To see usage
   ./resources/scripts/release.sh
   # To do the thing
   ./resources/scripts/release.sh sleipnir
   ```
