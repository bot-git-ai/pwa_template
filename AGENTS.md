# pwa_template

The template for the family of small Rust PWAs: a copy-and-rename starting
point, plus the rules for using it. It is not a sixth app — the app in here is
a counter, and it exists so the template's claims are verified rather than
asserted.

`cargo build` generates the **static PWA** into `./dist` — a front-end-only site
any file host can serve, with no server and no binary. All application logic,
state and rendering is Rust. The only JavaScript is the generated wasm-bindgen
bindings, a six-line dynamic-import loader, and the service worker.

AGPL-3.0-only: see `LICENSE`.

This is a standalone private repository, holding the original history of the
public `github.com/wdomitrz/pwa_template` (upstream, read-only).

## Read this first

**`INSTRUCTIONS.md` is the deliverable.** It is the original template's coding
rules rewritten for this stack, in the same terse voice: what to build, how it
is built, what not to add, and how to check it. Copy this repository, rename the
crate, and `INSTRUCTIONS.md` tells you what to do. Everything below is context
for that list — useful when a rule's reason is not obvious, and not a
prerequisite for using it.

## Build and run

Two builds, because there are two targets. Nothing generated is committed.

```
# 1. the site: compile the crate to wasm and generate the JS bindings
rustup target add wasm32-unknown-unknown
cargo build --locked --lib --target wasm32-unknown-unknown --release
wasm-bindgen --target web --no-typescript --out-dir dist --out-name app \
  target/wasm32-unknown-unknown/release/pwa_template.wasm

# 2. the rest of the site
touch build.rs
cargo build --release --locked
```

Step 1 writes `dist/app.js` and `dist/app_bg.wasm`; step 2 adds the six files
`build.rs` owns and derives the service worker's cache version.

The order matters: the cache version is hashed from every other file in
`dist/`, the wasm included, so step 2 pins it to whatever the previous build
left behind if it runs first; and the `touch` is not redundant, because
`build.rs` writes into the source tree rather than `OUT_DIR`, so cargo skips the
second, otherwise identical build without it. `build.rs` writes in place, under
a scratch name, and never replaces `dist/` — an earlier version that swapped the
whole tree deleted the wasm and shipped a publishable-looking site with no app
in it.

There is **no run step and no server**: the build is the whole story.

`wasm-bindgen` installs to `~/.cargo/bin`, which is on `PATH` in a normal login
shell; in a bare or non-login shell call it by absolute path
(`~/.cargo/bin/wasm-bindgen`). The version is pinned to `=0.2.128` in
`Cargo.toml` and must match the CLI exactly — a mismatched generator produces
bindings the runtime will not load, and the page then fails to start with "The
app could not start", which looks like an app bug and is not one.

## No compiler flag is needed, and adding one would be wrong

There is no `.cargo/config.toml`, no `RUSTFLAGS`, and there should be neither:
**this template uses no unstable API** — `--cfg=web_sys_unstable_apis` is for
web-sys's unstable surface (web-sys 0.3.105) — and a workflow carrying the cfg
the crate is not built with makes the wasm clippy run lint a different program
from the one that ships. Why, and what an app that *does* add an unstable API
must move together, is told in full in `pages.yml`'s `env:` comment. Verified
2026-10-02: a wasm build with a fresh `CARGO_TARGET_DIR`, no `RUSTFLAGS` and no
`.cargo/config.toml` finishes clean, in 7m41s.

Two traps worth keeping here because their failure is silent: `.cargo/config.toml`
is the better long-term home for the flag (it travels with the repository), but
`build.rs` does not work for it — `cargo:rustc-cfg` applies only to the
*building* package's own units, so the cfg never reaches web-sys, while a
`compile_error!` probe inside the crate confirms the cfg *is* set on the local
crate, which is exactly why the technique looks like it works. And several
web-sys getters are typed differently behind the cfg (`MouseEvent::client_x` is
`i32` on the stable path, `f64` behind it), so a leg without the flag is a false
green.

## The static site

Eight files, from two builds, and two owners:

- `app.js` and `app_bg.wasm` are written by `wasm-bindgen`. They are the app
  itself and exist nowhere else in the tree.
- The other six are written by `build.rs` during step 2, from committed sources:
  `index.html` (from `src/ui.html`, byte for byte), `service-worker.js` (with
  its cache name pinned to a version derived from the bytes of every other file
  in the directory **and its own source**), `icon.svg`, `icon-192.png`,
  `icon-512.png`, and `manifest.webmanifest`.

Everything the shell references is relative (`./app.js`,
`new URL('./', self.location.href)`, `start_url: "./"`), so one build works from
any subdirectory. Any file host can publish it: nginx, Caddy, GitHub Pages,
`python3 -m http.server`.

`dist/` is gitignored. It is reproducible: the same sources and the same pinned
toolchain produce the same bytes.

## Publishing it

Two workflows, deliberately separate.

`.github/workflows/build.yml` runs on every push and pull request, with
`contents: read` and nothing else. It runs both build steps in the order above
and inspects the `dist/` they produced.

`.github/workflows/pages.yml` publishes the site to GitHub Pages on every merge
to master, and on no other ref. It is a **separate file, not extra steps in
`build.yml`**, because a deploy needs `pages: write`, `id-token: write` and the
`github-pages` environment, and `build.yml` runs on pull requests from forks
where none of those exist. Folding them together would make the CI build
unrunnable by anyone who can push a branch, and would put the power to overwrite
the live site on every ref. Both publishing permissions are scoped to the
`deploy` job alone, never granted workflow-wide, so a build step or a
third-party action added later cannot spend them.

The gate has two halves, `github.ref == 'refs/heads/master' &&
!github.event.repository.fork`, in one `if:` — the fork clause is what keeps a
push to a fork's master (this repository *is* one) from trying to publish — and
`tests/shell.rs` asserts both halves from the comment-stripped workflow. Why the
artifact is rebuilt rather than passed across, why the gate keys on
`github.event.repository.fork` rather than a repository variable, and why the
artifact travels by name, are told in full in `pages.yml`'s comments.

**Pages must be enabled in the repository's settings** (Settings → Pages →
Source → GitHub Actions) before any of this deploys. That is a one-time action
by the repository's owner, and it is not something a workflow can do for itself.
Until then the deploy job fails and the build half of it is still true.

## Code map

- `counter.rs`: the app's logic — the count, its bounds, the wording of the
  readout, and what is worth persisting. No `web-sys`, no `js-sys`, no
  `wasm-bindgen`, so plain `cargo test` runs it natively. This is the template's
  argument in one file: the moment the domain module reaches for `document`, its
  tests stop running and the app becomes a blob.
- `lib.rs`: the two modules and the re-exports. `ui` is `#[cfg]`-gated to
  `wasm32-unknown-unknown`.
- `ui.rs`: the browser layer, and the only file that talks to the DOM. Reads the
  stored count, renders from state, binds one button, registers the service
  worker. It makes no decisions — everything it displays was decided and tested
  in `counter.rs`. It reports nothing to the page: the worker registers, and
  whether that worked is a console warning, not a line of the reader's screen.
- `ui.html`: the static shell. One inline `<style>`, one `<script type="module">`
  whose whole body is a dynamic import with a failure message. The README of the
  design is that a copy of this app renames four ids and deletes nothing.
- `service-worker.js`: install, activate, cache-first on an allowlist of `GET`s.
  No `skipWaiting`: an update takes over after the old tabs close, so a live app
  never swaps the wasm out from under itself.
- `build.rs`: writes the six files it owns, rasterizes the install icons from the
  committed SVG, assembles the manifest, and derives the worker's cache version.
- `assets/icon.svg`: the icon, and the source of every install icon. It is
  published to `dist/` unchanged and rasterized to the two PNGs at build time; it
  is never replaced by a PNG and never regenerated. Its SHA-256 is pinned in
  `tests/shell.rs` (with the story of its one change, on 2026-10-01, from the
  original template's `P` glyph to three bars in a mid grey — the old `#434343`
  was 1.9:1 on this project's own dark background), so changing it is a
  deliberate act.

## Tests

`cargo test --locked` needs neither the wasm target nor the bindings generator:
unit tests in `counter.rs` over the real logic, and `tests/shell.rs` — the
executable specification of the build's shape; read it. There are no browser
tests: the shell is written by the build, and nothing else would notice it
breaking.

## Verification

```
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo clippy --locked --lib --target wasm32-unknown-unknown -- -D warnings
```

Both clippy passes are required: the crate denies warnings, but only per target,
so a clean host build says nothing about the wasm one.

`cargo build` writes `dist/`, and the release gate does not carry it — it is
gitignored, so the exported tree the gate tests never has it and cannot build it.
That is why no test asserts on `dist/`: such a test would run only in a
developer's checkout, which is where it is least likely to catch anything.
`.github/workflows/build.yml` runs both build steps in the documented order on
every push and pull request, then inspects the `dist/` they produced — the only
automated coverage of the built site, aimed at the two failures this shape has
actually had. `tests/shell.rs` asserts the shape of both workflows from their
comment-stripped text, so commenting a line out instead of deleting it satisfies
an assertion that reads raw text.

## Known limitations

- `wasm-bindgen` must be installed separately and must match `=0.2.128` exactly.
  CI installs the pinned prebuilt binary and verifies it against both the
  release's own checksum and a digest recorded in the workflow.
- GitHub Pages is off until someone enables it in the repository's settings, so
  the Pages deploy's `deploy` job fails until they do. The build half of that
  workflow is still true and still checked.
- The counter is deliberately trivial. It is here to be replaced, not extended.
- Offline support needs one successful visit over HTTPS or localhost, and
  clearing site data removes it. That is the platform, not this app.