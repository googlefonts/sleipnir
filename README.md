# Sleipnir

[![Build Status](https://github.com/googlefonts/sleipnir/actions/workflows/rust.yml/badge.svg)](https://github.com/googlefonts/sleipnir/actions/workflows/rust.yml)
[![Docs](https://docs.rs/sleipnir/badge.svg)](https://docs.rs/sleipnir)
[![Crates.io](https://img.shields.io/crates/v/sleipnir.svg)](https://crates.io/crates/sleipnir)

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

## Testing

If you change the output of a tool and need to update the expected test data, you can run tests with the `UPDATE_EXPECTED` environment variable set:

```shell
UPDATE_EXPECTED=1 cargo test
```

## Examples

### tohtml

`tohtml` is a command-line tool that converts all glyphs in a font file to
an HTML file of SVG.

```shell
# Convert all glyphs in a font to SVGs in /tmp/output
cargo run --example tosvg -- --font path/to/font.ttf --output-path /tmp/output.html

# Convert with specific variational coordinates (e.g., for a variable font)
cargo run --example tosvg -- --font path/to/font.ttf --output-path /tmp/output.html --coords 0.5,1.0
```

### pathstyle_report

`pathstyle_report` measures how switching `SvgPathStyle` affects SVG size and
rendering quality, and writes a self-contained HTML report (plus a JSON file of
aggregates). Every glyph is drawn with a reference style (default `unchanged`),
a baseline (default `compact`) and a candidate (default `minimized`) at a set of
variable-font locations. Sizes are full SVG bytes; quality is measured by
rasterizing with tiny-skia at several pixel sizes and comparing against the
reference (mean absolute alpha error, worst pixel, IoU, visibly changed pixels).

```shell
# Bundled test font; writes target/pathstyle_report/report.html
cargo run --release --example pathstyle_report

# Your fonts, specific locations, rasterize every glyph
cargo run --release --example pathstyle_report -- \
    --font path/to/a.ttf --font path/to/b.ttf \
    --var opsz,wght,FILL,GRAD,ROND@48,100,0,-25,100 --var wght@700 \
    --full --output /tmp/report.html

# Compare other styles
cargo run --release --example pathstyle_report -- --baseline minimized --candidate minimized-svgo
```

Without `--var`, each font is measured at its default location, every axis at
its min and max, and all axes at min, max and midpoint. Run
`cargo test --example pathstyle_report` to test the tool itself.
