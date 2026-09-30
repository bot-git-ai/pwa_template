# Rust PWA Template

The starting point for a small progressive web app written in Rust. Copy this
repository, rename the crate, and you have the next app: all the logic, state
and rendering are Rust, the HTML is a static shell, and `cargo build` writes a
publishable site into `dist/`.

The app in here is a counter. It exists so the template's claims are verified
rather than asserted — it builds, it installs, it works offline, and its tests
run under plain `cargo test` with no browser, no Node and no Chromium.

## Build

Two steps, because there are two targets. Nothing generated is committed.

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.128 --locked

# 1. the site: compile to wasm and generate the JS bindings
cargo build --locked --lib --target wasm32-unknown-unknown --release
wasm-bindgen --target web --no-typescript --out-dir dist --out-name app \
  target/wasm32-unknown-unknown/release/pwa_template.wasm

# 2. the rest of the site
touch build.rs
cargo build --release --locked
```

That leaves a publishable `dist/` of eight files. **The order matters**: step 2
derives the service worker's cache name from the wasm, so running it first would
pin that to the previous build. The `touch` matters too — `build.rs` writes into
the source tree, so cargo would otherwise skip the second build and copy
nothing.

`cargo test --locked` needs none of the above.

## Use it

Copy the repository, rename the crate in `Cargo.toml` and `Cargo.lock`, and
replace the contents of `counter.rs` (your logic), `ui.rs` (your DOM) and
`ui.html` (your shell). Then delete what you do not need and rename six element
ids. Nothing else has to move.

**`INSTRUCTIONS.md` is the guide** — the original template's coding rules,
rewritten for this stack. Read it before you write anything.

## Layout

    src/counter.rs         the app's logic, unit tested, no web-sys
    src/ui.rs              the browser layer: the only file that touches the DOM
    src/ui.html            the static shell: one style block, one script tag
    src/service-worker.js  install, activate, fetch; one __VERSION__ placeholder
    build.rs               writes six of the eight files in dist/, derives the rest
    assets/icon.svg        the icon, committed and authoritative
    dist/                  build output — the site itself, gitignored
    tests/shell.rs         invariants of the committed shell

## Gotchas

- `wasm-bindgen` is **not** on `PATH` in a non-login shell. Call it by absolute
  path, or add `~/.cargo/bin`.
- The generator version is pinned to `=0.2.128` and must match `Cargo.toml`
  exactly. A mismatch produces bindings the page will not load, and it fails with
  "The app could not start".
- After changing anything in `src/`, run both build steps again. The wasm is not
  committed, so nothing reminds you.
- `build.rs` writes into `dist/`, which is gitignored — so no test can assert on
  it. That is what `.github/workflows/build.yml` is for: it runs both steps and
  checks the site they produced.

## Licence

AGPL-3.0-only. See `LICENSE`. The icon is the original author's, unchanged.