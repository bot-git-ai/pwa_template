# pwa_template

The template for the family of small Rust PWAs: a copy-and-rename starting
point, plus the rules for using it. It is not a sixth app — the app in here is
a counter, and it exists so the template's claims are verified rather than
asserted.

`cargo build` generates the **static PWA** into `./dist` — a front-end-only site
any file host can serve, with no server and no binary. All application logic,
state and rendering is Rust. The only JavaScript is the generated wasm-bindgen
bindings, a six-line dynamic-import loader, and the service worker.

AGPL-3.0-only. See `LICENSE`.

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

**The order matters**, and the `touch` is not redundant. The cache version is
hashed from the bytes of every other file in `dist/`, the wasm included, so
running step 2 first pins it to whatever the previous build left behind — a
worker whose cache never invalidates, shipping an app older than its own cache
name. And `build.rs` writes into the source tree rather than `OUT_DIR`, so cargo
cannot see that its output changed and would skip the second, otherwise
identical build, leaving `dist/` with the two wasm artefacts and none of the six
shell files. That is a half-built site, and it is the failure this shape has
already had.

`build.rs` writes only the files it owns, each under a scratch name and renamed
into place. It does not replace `dist/`, so it leaves the wasm alone: an earlier
version that swapped the whole tree deleted them and left a publishable-looking
site with no app in it.

There is **no run step and no server**: the build is the whole story.

`wasm-bindgen` installs to `~/.cargo/bin`, which is on `PATH` in a normal login
shell; in a bare or non-login shell call it by absolute path
(`~/.cargo/bin/wasm-bindgen`). The version is pinned to `=0.2.128` in
`Cargo.toml` and must match the CLI exactly — a mismatched generator produces
bindings the runtime will not load, and the page then fails to start with "The
app could not start", which looks like an app bug and is not one.

## No compiler flag is needed, and adding one would be wrong

There is no `.cargo/config.toml`, no `RUSTFLAGS`, and there should be neither.
`--cfg=web_sys_unstable_apis` is for web-sys's unstable surface — the Screen
Wake Lock API, whose `WakeLockSentinel` type is behind that cfg in web-sys
0.3.105 and is unreachable without it even with the cargo features enabled.
**This template uses no unstable API.** Verified 2026-10-02: a wasm build with
a fresh `CARGO_TARGET_DIR`, no `RUSTFLAGS` and no `.cargo/config.toml` finishes
clean, in 7m41s.

So a copy-and-rename should not acquire the flag, and a reviewer should distrust
a workflow that carries it. The reason is not tidiness. Several web-sys getters
are typed *differently* behind that cfg — `MouseEvent::client_x` is `i32` on
the stable path and `f64` behind it, and `PointerEvent` inherits the type — so
a workflow setting a cfg the crate is not built with makes the wasm clippy run
lint a **different program from the one that ships**. The host clippy run cannot
see it, because the host build compiles no web-sys code at all. `chwazi` sets no
`RUSTFLAGS` and says why at length in its own `build.yml`.

If a generated app *does* add an unstable API, three things have to move
together, and the cfg is the smallest of them:

- the `web-sys` cargo features for what it calls (necessary, and **not**
  sufficient on their own);
- `RUSTFLAGS: --cfg=web_sys_unstable_apis` in the workflows' top-level `env:`,
  not on one step, so the wasm clippy run sees it too;
- the app's AGENTS.md, saying which API and why.

`.cargo/config.toml` is the better long-term home for the flag — it travels with
the repository, so a fresh CI runner and a plain `cargo build` both get it
without anything to remember — but `build.rs` does **not** work, and it is worth
saying why, because the failure is silent. `cargo:rustc-cfg` applies only to the
*building* package's own units, and web-sys is a registry dependency compiled in
its own unit, so the cfg never reaches it. A `compile_error!` probe inside the
crate confirms the cfg *is* set on the local crate, which is exactly why the
technique looks like it works.

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

The Pages workflow rebuilds the site rather than passing `build.yml`'s artifact
across: `upload-pages-artifact` is not `upload-artifact`, the Pages artifact is a
single tarball the deploy then finds *by name*, and `deploy-pages` v5 has no
`artifact_id` input — it warns `Unexpected input(s) 'artifact_id'`, ignores it,
and falls back to its own default. Both sides therefore state `artifact_name`
explicitly and `tests/shell.rs` asserts they are the same string, so a mismatch
is a diff rather than a bare `HttpError: Not Found` at run time.

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
  `tests/shell.rs`, so changing it is a deliberate act. It was the original
  template's `P` glyph until 2026-10-01, when three bars in a mid grey replaced
  it: a letter read as decoration, and the old `#434343` was 1.9:1 on this
  project's own dark background.

## Tests

`cargo test --locked` needs neither the wasm target nor the bindings generator.
Two kinds:

- Unit tests in `counter.rs`, over the real logic: saturation at both ends,
  clamping on the way in, corrupt or absent storage, and what is worth writing.
- `tests/shell.rs`, over the committed shell: the page loads generated bindings
  and has exactly one script tag; the loader does nothing but load and report
  failure; no absolute URLs; the worker template has exactly one `__VERSION__`;
  the icon is the committed SVG and the PNGs are nowhere but `dist/`; no
  generated artefact is tracked and `dist/` is ignored; the JS template's files
  are gone; and `INSTRUCTIONS.md` is still a short flat list.

Three of those are worth calling out, because they were written to catch real
bugs rather than to decorate the file:

- **`every_precached_file_is_published_by_the_build_script`** parses the worker's
  `ASSETS` array and `build.rs`'s own declarations, and asserts the two agree in
  both directions. This repository shipped a site whose page and worker both
  referenced `icon.svg` while `build.rs` never published it. The consequence was
  not a missing favicon: `caches.addAll` rejects the *entire* install if any
  listed URL 404s, so the app lost its service worker and all offline support,
  and presented as a caching bug with nothing in the build output pointing at
  the missing file.
- The expectations are **derived**, not duplicated. A hand-kept list of eight file
  names in the test is a second copy of the truth and would have agreed with
  itself while the site was broken — which is how the bug survived in the first
  place.
- **`the_whole_site_is_the_union_of_its_two_owners`** states the whole invariant
  in one assertion: the two build steps write exactly these eight files, and the
  worker allowlists exactly those.

There are no browser tests. The shell is written by the build, and nothing else
would notice it breaking.

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
every push and pull request, then inspects what came out: all eight files by
name, present and non-empty, **nothing unexpected**, the worker's `__VERSION__`
already substituted, the bindings carrying the exports the page's dynamic import
needs, `dist/icon.svg` identical to the committed icon, and a manifest whose icon
list matches what was actually rasterized.

That check is the only automated coverage of the built site, and it is aimed at
the two failures this shape has actually had: a `dist/` that looks publishable
and has no app in it, and a `dist/` that has everything except one file nobody
wrote. It fails on unexpected files as well as missing ones, because a stray
ships exactly as quietly as an absence does.

`tests/shell.rs` also asserts the *shape* of both workflows, reading them with
comments stripped — commenting a line out instead of deleting it is the mutation
most likely to be applied to any of them, and it satisfies an assertion that
reads raw text. Eleven of those: the generator pin matches `Cargo.toml` and is
declared rather than only used; no `RUSTFLAGS` where the crate needs none; master
is the only ref that can reach the live site; the publishing permissions are in
the `deploy` job and nowhere else; the deploy is handed the artifact the build
uploaded, by name; the deploy is not in `build.yml`; every action is pinned to a
40-character commit SHA; the site check names all eight files and greps the
things a green build cannot imply; and both workflows pin the icon digest this
repository actually has.

That last one is not decoration. The icon's SHA-256 lives in three places — the
`ICON_SHA256` constant in `tests/shell.rs` and one copy in each workflow's site
check — and the icon changed on 2026-10-01 from the `P` glyph to three bars.
That commit updated the constant and the icon, and **not** `build.yml`, so the
build sat red from then until 2026-10-02, when adding `pages.yml` forced the
check to be run for real rather than assumed green. The workflow copies now
carry the reason, and the test asserts both equal the file on disk, so the three
cannot drift apart again.

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