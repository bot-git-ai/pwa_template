// Copyright (c) 2026 Witalis Domitrz <witekdomitrz@gmail.com>
// AGPL License

//! Invariants of the committed shell, and of what is and is not committed.
//!
//! Everything here reads committed files. `dist/` is build output and is
//! gitignored, so the release gate — which exports the candidate tree — never
//! has it and cannot build it: that needs the wasm target and a pinned
//! `wasm-bindgen` CLI. A test asserting on `dist/` would therefore run only in
//! a developer's checkout, which is exactly where it is least likely to catch
//! anything. What covers the built output is running the two build steps in the
//! order AGENTS.md gives them, and `.github/workflows/build.yml` does that on
//! every push.
//!
//! What these tests do protect is that the shell cannot quietly grow
//! application JavaScript again, cannot point anywhere absolute, cannot commit a
//! generated file, and cannot lose the icon that every install icon is derived
//! from. Each of those has been true of a real version of this repository.

use std::path::Path;

/// The repository root, as the tests see it.
fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// The app shell, as committed.
///
/// `build.rs` copies this into `dist/index.html` byte for byte, so asserting on
/// it asserts on exactly what gets published.
fn shell() -> String {
    let path = root().join("src/ui.html");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

/// The service worker template, as committed.
fn worker_template() -> String {
    let path = root().join("src/service-worker.js");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

/// Tracked file names, or `None` outside a checkout.
///
/// The release gate exports the candidate as a bare directory with no `.git`,
/// so there is no index to ask. Callers decide what that means.
fn tracked_files() -> Option<String> {
    let inside = std::process::Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(root())
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false);
    if !inside {
        return None;
    }
    let output = std::process::Command::new("git")
        .args(["ls-files"])
        .current_dir(root())
        .output()
        .expect("git ls-files");
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Every file `dist/` must contain once both build steps have run.
///
/// The two `wasm-bindgen` artefacts are named here because nothing in the tree
/// generates them; the other six are the files `build.rs` owns, and it asserts
/// those itself by parsing the worker's allowlist (see
/// `every_precached_file_is_published_by_the_build_script`). This list is what
/// the workflow's site check uses, and it is written out by name on purpose: a
/// count would not catch a file that is referenced but never written.
const DIST_FILES: [&str; 8] = [
    "app.js",
    "app_bg.wasm",
    "icon.svg",
    "icon-192.png",
    "icon-512.png",
    "index.html",
    "manifest.webmanifest",
    "service-worker.js",
];

/// The files `build.rs` publishes, read out of the script itself.
///
/// Derived, never duplicated: a hand-kept list of eight names is a second copy
/// of the truth, and it drifts — which is exactly how `icon.svg` came to be
/// referenced by the page and the worker but never written to `dist/`. A test
/// that duplicated the list would have agreed with itself and passed.
fn published_by_build_script() -> Vec<String> {
    let build = std::fs::read_to_string(root().join("build.rs")).expect("the build script");

    // The verbatim copies: `SHELL` names the destination first.
    let (_, shell) = build
        .split_once("const SHELL: &[(&str, &str)] = &[")
        .expect("build.rs declares the files it copies");
    let shell = shell
        .split_once("];")
        .expect("the end of the SHELL table")
        .0;
    let mut names: Vec<String> = shell
        .lines()
        .filter_map(|line| line.trim().strip_prefix("(\""))
        .filter_map(|rest| rest.split('"').next())
        .map(str::to_owned)
        .collect();

    // The icon is published under a constant, beside the PNGs derived from it.
    // This must come from the push, not from the constant's declaration: reading
    // the constant would accept a site that publishes the icon under a name
    // built at runtime, and would then agree with itself about a file the worker
    // cannot fetch. What has to be true is that *the bytes* reach `dist/` under
    // a name the allowlist uses.
    let mut published_icons: Vec<&str> = build
        .lines()
        .filter(|line| {
            line.trim_start().starts_with("built.push((") && line.contains("icon-{size}.png")
        })
        .filter_map(|line| line.split('"').nth(1))
        .collect();
    // Each entry must name the SVG as its content, and the allowlist is the
    // arbiter of what that name is, so the check below closes the loop.
    assert!(
        build.lines().any(|line| {
            line.trim_start().starts_with("built.push((")
                && line.contains("svg.clone()")
                && !line.contains("format!(\"icon-")
        }),
        "build.rs derives the install icons but never publishes assets/icon.svg itself; \
         the page links it as the favicon and the worker precaches it"
    );
    published_icons.sort_unstable();
    names.extend(published_icons.into_iter().map(str::to_owned));

    // And the icon itself. Its published name is decided by `ICON_OUT`, so read
    // the constant's *value* here: scanning for the constant's name instead
    // would happily accept `format!("{ICON_OUT}.bak")`, which writes the SVG
    // under a name no allowlist can fetch — and then agree with itself.
    let icon_out = build
        .lines()
        .find_map(|line| line.trim().strip_prefix("const ICON_OUT: &str = \""))
        .and_then(|rest| rest.split('"').next())
        .expect("build.rs declares the icon's published name")
        .to_owned();
    names.push(icon_out);

    // The manifest, assembled in Rust.
    if build.contains("\"manifest.webmanifest\"") {
        names.push("manifest.webmanifest".to_owned());
    }

    // The install PNGs, named with a format string at each declared size.
    let declared = build
        .lines()
        .find_map(|line| line.trim().strip_prefix("const ICON_SIZES: [u32; 2] = ["))
        .and_then(|rest| rest.split(']').next())
        .unwrap_or_default();
    for size in declared
        .split(',')
        .filter_map(|size| size.trim().parse::<u32>().ok())
    {
        names.push(format!("icon-{size}.png"));
    }

    // The service worker, written from the template.
    if build.contains("template.replace(\"__VERSION__\"") {
        names.push("service-worker.js".to_owned());
    }

    names.sort();
    names
}

/// The entries of the worker's `ASSETS` allowlist, parsed out of the template.
///
/// These are the URLs `caches.addAll` fetches on install. `addAll` rejects the
/// whole install if any one of them 404s, so a name that is allowlisted but
/// never published does not degrade to a missing icon: it takes the service
/// worker down with it, and in the browser it presents as "offline is broken"
/// with nothing in the build output pointing at the missing file.
fn precached_by_the_worker() -> Vec<String> {
    let worker = worker_template();
    let list = worker
        .split_once("const ASSETS = [")
        .expect("the worker declares an ASSETS allowlist")
        .1
        .split_once(']')
        .expect("the end of the ASSETS allowlist")
        .0;
    list.split('\'')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// Every URL the worker precaches is a file `build.rs` publishes.
///
/// This is the assertion that would have caught the missing `icon.svg`, and it
/// is deliberately derived from both sides rather than checked against a third
/// hand-written list: the worker's allowlist says what is fetched, `build.rs`
/// says what is written, and this is where they are made to agree.
#[test]
fn every_precached_file_is_published_by_the_build_script() {
    let published = published_by_build_script();
    assert!(
        !published.is_empty(),
        "could not read the published files out of build.rs; the assertion would pass vacuously"
    );
    assert!(
        published.iter().any(|name| name == "index.html"),
        "build.rs no longer publishes the shell: {published:?}"
    );

    for entry in precached_by_the_worker() {
        if entry == "./" {
            // The scope root, which the host serves as `index.html`.
            continue;
        }
        if WASM_BINDGEN_FILES.contains(&entry.as_str()) {
            assert!(
                !published.contains(&entry),
                "'{entry}' is written by wasm-bindgen, so build.rs must not also write it"
            );
            continue;
        }
        assert!(
            published.contains(&entry),
            "the service worker precaches '{entry}' but build.rs does not publish it; \
             caches.addAll rejects the entire install when any listed URL is missing, \
             so this costs the app its offline support rather than one icon. \
             build.rs publishes: {published:?}"
        );
    }
}

/// Nothing `build.rs` publishes is missing from the allowlist: a published
/// file the worker does not precache is a file the app still needs online.
#[test]
fn everything_the_build_publishes_is_precached_or_understood() {
    let precached = precached_by_the_worker();
    for name in published_by_build_script() {
        if name == "service-worker.js" {
            // A worker is fetched by the browser, never from its own cache.
            continue;
        }
        assert!(
            precached.contains(&name),
            "build.rs publishes '{name}' but the service worker does not precache it"
        );
    }
}

/// `dist/` is the union of the two owners, and the worker allowlists exactly
/// that. This is the test that reports a site in terms a reader can act on:
/// eight files, eight precached, nothing referenced and unwritten.
#[test]
fn the_whole_site_is_the_union_of_its_two_owners() {
    let mut site = published_by_build_script();
    site.extend(WASM_BINDGEN_FILES.iter().map(|name| (*name).to_owned()));
    site.sort();
    site.dedup();

    let mut documented: Vec<String> = DIST_FILES.iter().map(|name| (*name).to_owned()).collect();
    documented.sort();
    assert_eq!(
        site, documented,
        "the files the two build steps write must match the documented site exactly"
    );

    let precached = precached_by_the_worker();
    assert_eq!(
        precached.len(),
        DIST_FILES.len(),
        "the worker precaches every file in the site except itself"
    );
}

/// The two files `wasm-bindgen` writes into `dist/`, and `build.rs` does not.
///
/// They are build output with no committed source, which is exactly why they
/// are named here rather than left implicit: everything else in `dist/` belongs
/// to `build.rs`, and the assertions below are only meaningful if the two
/// ownership domains are both accounted for.
const WASM_BINDGEN_FILES: [&str; 2] = ["app.js", "app_bg.wasm"];

/// Run `git check-ignore` for `path`, or `None` outside a checkout.
fn is_ignored(path: &str) -> Option<bool> {
    tracked_files()?;
    Some(
        std::process::Command::new("git")
            .args(["check-ignore", "-q", path])
            .current_dir(root())
            .status()
            .expect("git check-ignore")
            .success(),
    )
}

/// The app is wasm. A hand-written ABI would mean the Rust in the page is not
/// the Rust in `src/`, which is the one thing this shape exists to prevent.
#[test]
fn the_page_loads_generated_bindings_not_a_manual_wasm_abi() {
    let page = shell();
    assert!(
        page.contains("<!DOCTYPE html>"),
        "the shell must be a document"
    );
    assert!(page.contains("<script type=\"module\">"), "a module script");
    assert!(
        page.contains("import('./app.js')"),
        "the page must load the generated bindings"
    );
    assert!(
        page.contains(".then(m => m.default())"),
        "the loader must hand over to the generated start function"
    );
    assert_eq!(
        page.matches("<script").count(),
        1,
        "exactly one script tag:\n{page}"
    );
    for obsolete in [
        "instantiateStreaming",
        "alloc_buf",
        "wasm.exports",
        "fetch(",
    ] {
        assert!(!page.contains(obsolete), "{obsolete} in the static shell");
    }
}

/// The loader's only job is to hand over and to say so when it cannot. There
/// must be no application logic left in it: a `addEventListener`, a state
/// variable or a second statement doing real work is how a JavaScript app grows
/// back.
#[test]
fn the_loader_does_nothing_but_load_and_report_failure() {
    let page = shell();
    let script = page
        .split_once("<script type=\"module\">")
        .expect("the module script")
        .1
        .split_once("</script>")
        .expect("the end of the module script")
        .0;
    assert!(
        script.contains("catch"),
        "the loader must handle the app failing to load"
    );
    assert!(
        script.contains("getElementById('error')"),
        "the failure branch must show something the user can read"
    );
    assert!(
        script.contains("could not start"),
        "the failure message must say the app could not start"
    );
    // It may declare locals in the failure branch, but it may not do anything
    // the app's logic could ever want: those would be state and events, which
    // are Rust's job. No comment either -- a loader that explains itself is
    // usually a loader that grew.
    for forbidden in [
        "//",
        "/*",
        "addEventListener",
        "querySelector",
        "localStorage",
        "function",
        "fetch(",
    ] {
        assert!(
            !script.contains(forbidden),
            "{forbidden} in the loader:\n{script}"
        );
    }
    assert!(
        !script.contains("let ") && script.matches("const ").count() <= 1,
        "the loader must declare no state of its own:\n{script}"
    );
}

/// An installable, accessible page: the install icon is linked, the dynamic
/// readout is announced, and the control is a real button with a visible focus
/// ring and a large enough target.
#[test]
fn the_page_is_accessible_and_installable() {
    let page = shell();
    for required in [
        "id=\"message\"",
        "id=\"hello-button\"",
        "id=\"offline-status\"",
        "id=\"error\"",
        "role=\"status\"",
        "aria-live",
        "<button id=\"hello-button\" type=\"button\">",
        "rel=\"manifest\"",
        "rel=\"icon\"",
        "rel=\"apple-touch-icon\"",
        "<noscript>",
    ] {
        assert!(page.contains(required), "the shell is missing {required}");
    }
    assert!(
        page.contains("outline: 3px solid"),
        "focus must stay visible"
    );
    assert!(
        page.contains("min-height: 44px"),
        "touch targets must be large enough for mobile use"
    );
    assert!(
        page.contains("prefers-color-scheme") && page.contains("prefers-reduced-motion"),
        "the shell must respect both media queries"
    );
}

/// No visible text may name the implementation.
///
/// An interface that tells the user it is "written in Rust", "compiled to
/// WebAssembly" or "loading the Rust app" is talking to nobody: the user came
/// to use a counter, and how it is built is neither useful nor anything they
/// can act on. It is also the single most common way a demo page ends up looking
/// unfinished, because the sentences are about the author, not the app.
///
/// Implementation vocabulary is fine in comments, docs and `AGENTS.md`. It is
/// not fine in anything a reader can see: markup, text, `alt`, the meta
/// description, or any string Rust writes into the DOM.
#[test]
fn no_visible_text_names_the_implementation() {
    let page = shell();
    // Rendered text only: comments are stripped, and `<style>`/`<script>` bodies
    // are dropped too, since neither is something a reader sees.
    let rendered = visible_text(&page);
    for banned in [
        "rust",
        "webassembly",
        "wasm",
        "bindings",
        "compiled",
        "compile",
    ] {
        assert!(
            !rendered.to_lowercase().contains(banned),
            "the rendered page says \"{banned}\"; user-visible text must not name \
             the implementation — say what the user gets or loses instead. \
             Visible text was:\n{rendered}"
        );
    }
    // "JavaScript" is the single exception, and only in a `<noscript>`: naming
    // the thing the user must go and switch on is the one case where the
    // implementation vocabulary *is* the instruction. Everywhere else it is
    // not actionable and is banned above.
    let outside_noscript = rendered.replace(noscript_body(&rendered).as_str(), "");
    assert!(
        !outside_noscript.to_lowercase().contains("javascript"),
        "only a <noscript> block may name JavaScript; everywhere else it must not"
    );
    // The same rule for every string the Rust layer can write into the DOM.
    let rust = std::fs::read_to_string(root().join("src/ui.rs")).expect("src/ui.rs");
    for line in rust.lines() {
        let trimmed = line.trim_start();
        // Skip doc comments and line comments; they are documentation.
        if !trimmed.starts_with("const ") || !trimmed.contains('"') {
            continue;
        }
        let value = trimmed.split('"').nth(1).unwrap_or_default().to_lowercase();
        for banned in ["rust", "webassembly", "wasm", "javascript", "compiled"] {
            assert!(
                !value.contains(banned),
                "a user-visible string in src/ui.rs says \"{banned}\": {trimmed}"
            );
        }
    }
}

/// A status line must be able to change. An element that can only ever show the
/// text it shipped with is decoration pretending to be information, and two
/// permanently-frozen lines under a working readout are most of what makes a
/// demo page look unfinished.
///
/// The rule this enforces: if a string is only ever set by the loader, it does
/// not belong on the page at all.
#[test]
fn no_status_line_is_frozen() {
    let page = shell();

    // Every `id` the page renders, so a new one cannot slip in unasserted.
    // `#error` is exempt: it is the failure box, filled by the loader's catch
    // and hidden until then. An element that only exists on failure is not a
    // frozen status line.
    let mut shown: Vec<&str> = page
        .match_indices("id=\"")
        .filter_map(|(at, _)| {
            let rest = &page[at + 4..];
            rest.find('"').map(|end| &rest[..end])
        })
        .filter(|id| !matches!(*id, "message" | "hello-button" | "error"))
        .collect();
    shown.sort_unstable();

    let rust = std::fs::read_to_string(root().join("src/ui.rs")).expect("src/ui.rs");
    for id in shown {
        assert!(
            rust.contains(&format!("\"{id}\"")),
            "#{id} is rendered by the shell but src/ui.rs never touches it, so it \
             can only ever show the text it shipped with — remove it or let the \
             app write to it"
        );
    }
}

/// The button's visible text is its accessible name. An `aria-label` describing
/// the same control differently gives a sighted user and a screen-reader user
/// two different descriptions of one button, which is worse than either.
#[test]
fn the_button_has_one_consistent_name() {
    let page = shell();
    let rust = std::fs::read_to_string(root().join("src/ui.rs")).expect("src/ui.rs");
    // Comments in both files discuss *why* there is no aria-label, so search the
    // markup only: strip comments from the shell, and take non-comment lines of
    // the Rust.
    let shell_markup = visible_text(&page);
    assert!(
        !shell_markup.contains("aria-label"),
        "the shell must not hard-code an aria-label: the visible text is already \
         the accessible name, and a second name contradicts it"
    );
    for (source, text) in [("src/ui.html", &page), ("src/ui.rs", &rust)] {
        for line in text.lines() {
            let trimmed = line.trim();
            // Skip comment lines and the continuation lines of block comments.
            let is_comment = trimmed.starts_with("//")
                || trimmed.starts_with("///")
                || trimmed.starts_with('*')
                || trimmed.starts_with("/*");
            if is_comment {
                continue;
            }
            assert!(
                !trimmed.contains("aria-label") && !trimmed.contains("aria_label"),
                "{source} sets an aria-label on a control that has visible text: {trimmed}\n\
                 The visible text is already the accessible name; a second one \
                 contradicts it"
            );
        }
    }

    // And the label must describe what the app does. "Say hello" is what the
    // original template's button said, on an app that only printed a greeting;
    // this app counts, so the label has to say so.
    let shell_button = page
        .split_once("<button id=\"hello-button\" type=\"button\">")
        .expect("the app's one button")
        .1
        .split_once("</button>")
        .expect("the end of the button")
        .0;
    assert_eq!(
        shell_button.trim(),
        "Add one",
        "the button's label must describe what it does, not what it used to do"
    );
}

/// The text inside the page's `<noscript>` element, or `""` if it has none.
fn noscript_body(page: &str) -> String {
    page.split_once("<noscript>")
        .and_then(|(_, rest)| rest.split_once("</noscript>"))
        .map_or(String::new(), |(body, _)| body.to_owned())
}

/// Everything a reader can actually see: HTML with comments, `<style>` bodies
/// and `<script>` bodies removed.
///
/// The `<noscript>` block is deliberately *kept*, even though it lives in a
/// comment-like position — a browser with scripting off renders exactly that
/// text, so it is user-visible text and is held to the same rule. What it must
/// not do is name the implementation: it says what the user loses.
fn visible_text(page: &str) -> String {
    let mut out = String::with_capacity(page.len());
    let mut rest = page;
    loop {
        let Some(start) = rest.find("<!--") else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..start]);
        rest = rest[start..]
            .find("-->")
            .map_or("", |end| &rest[start + end + 3..]);
    }
}

/// The site is mounted under an arbitrary prefix, so every URL in it is
/// relative. One build, any subdirectory, any file host.
#[test]
fn the_shell_is_mountable_anywhere() {
    let page = shell();
    assert!(
        !page.contains("http://") && !page.contains("https://"),
        "an absolute URL would break the site outside its own origin"
    );
    assert!(
        page.contains("./app.js"),
        "bindings must be referenced relatively"
    );
    assert!(
        page.contains("./icon.svg"),
        "the SVG icon must be linked directly"
    );
    assert!(
        page.contains("./manifest.webmanifest"),
        "the manifest must be linked relatively"
    );
    assert!(
        shell().contains("./icon.svg"),
        "the worker precaches the committed SVG, which is the icon's source of truth"
    );
    // Every file the page links must be one `build.rs` publishes. `./icon.svg`
    // is the one that is easy to miss: it is rasterized, so publishing it looks
    // like an extra step rather than a requirement.
    assert!(
        shell().contains("./icon-192.png") && shell().contains("./manifest.webmanifest"),
        "the page must reference the manifest and the apple-touch icon relatively"
    );
    // The worker's own URLs must resolve the same way.
    assert!(
        worker_template().contains("new URL('./', self.location.href)"),
        "the worker must resolve its cache from its own location"
    );
}

/// The service worker is a committed template with exactly one placeholder, and
/// `build.rs` substitutes it. No placeholder would mean the cache never
/// invalidates; two would mean the substitution is not the only edit.
#[test]
fn the_service_worker_template_has_exactly_one_placeholder() {
    let worker = worker_template();
    assert_eq!(
        worker.matches("__VERSION__").count(),
        1,
        "the template must carry exactly one version placeholder"
    );
    // Everything the worker precaches, including the SVG that is the icon's
    // source of truth and its own `index.html`, so the offline path serves the
    // same page the network path does.
    for file in DIST_FILES
        .iter()
        .filter(|file| **file != "service-worker.js")
    {
        assert!(
            worker.contains(&format!("'{file}'")),
            "the worker does not precache {file}, and a 404 in `caches.addAll` \
             rejects the whole install, so {file} must exist in dist/"
        );
    }
    // The worker is deliberately not in its own list: a worker is fetched by
    // the browser, never from its own cache.
    assert!(
        worker.contains("'icon.svg'"),
        "the worker does not precache the icon's source of truth"
    );
    // `skipWaiting` must not be *called*. The word appears in the template's
    // own comment explaining why it is absent, so match the call.
    assert!(
        !worker.contains("skipWaiting("),
        "an update must never swap the wasm under a live tab"
    );
}

/// The worker only ever answers for a URL inside its own app's directory.
///
/// This is the guard that stops one of these apps from taking over the pages it
/// shares an origin with. A service worker registered for a scope is consulted
/// for every URL under that scope, and these apps are all served from the same
/// origin as pages that are not apps at all — so "the scope is small" is a
/// promise, and this test is what keeps it one.
#[test]
fn the_worker_never_answers_outside_its_own_directory() {
    let worker = worker_template();
    assert!(
        worker.contains("new URL('./', self.location.href)"),
        "the worker must resolve its own directory from its location"
    );
    // The guard is a prefix test against that directory, on the request URL,
    // applied before the allowlist decides anything.
    assert!(
        worker.contains("IS_OWN(url)"),
        "the fetch handler must check the request is inside this app's directory; \
         without it a mis-scoped registration serves whatever it cached"
    );
    assert!(
        worker.contains("const IS_OWN = url => url.startsWith(ROOT.href)"),
        "the directory guard must be a prefix test against the worker's own root"
    );
}

/// The page states the worker's scope instead of inheriting it, and cleans up a
/// wider registration left behind by an earlier version.
///
/// A registration outlives the page that created it, and nothing short of an
/// explicit `unregister` takes one away. So the second half is what makes this
/// recoverable without the user clearing their browser: a stale registration
/// is not fixed by a reload, and the newer worker cannot take control of a
/// scope it does not own.
#[test]
fn the_page_states_the_scope_and_releases_a_wider_one() {
    let ui = std::fs::read_to_string(root().join("src/ui.rs")).expect("reading src/ui.rs");
    assert!(
        ui.contains("register_with_options"),
        "the worker must be registered with an explicit scope; left to default, \
         the scope is whatever directory the registering page sits in"
    );
    assert!(
        ui.contains("RegistrationOptions::new()") && ui.contains("set_scope(SCOPE)"),
        "the scope has to be actually stated, not merely a named constant"
    );
    assert!(
        ui.contains("get_registrations") && ui.contains("unregister"),
        "a stale wider registration survives a reload, a version bump and a \
         reinstall; only an explicit unregister clears it"
    );
}

/// A worker's script is compared by suffix, not by `trim_end_matches`.
///
/// `trim_end_matches` strips a *set of characters*, so a directory whose name
/// ends in those letters is silently treated as ours — and a registration
/// belonging to a sibling app would be torn down. This is a regression test for
/// a real bug in the first version of this code.
#[test]
fn the_script_comparison_strips_a_suffix_rather_than_a_character_set() {
    let ui = std::fs::read_to_string(root().join("src/ui.rs")).expect("reading src/ui.rs");
    // The prose in this file names the method to explain why it is not used, so
    // the assertion is about code: a call, not the word.
    let calls: Vec<&str> = ui
        .lines()
        .filter(|line| {
            let code = line.split("//").next().unwrap_or(line);
            code.contains("trim_end_matches(")
        })
        .collect();
    assert!(
        calls.is_empty(),
        "`trim_end_matches` strips a character set, not a filename: it would eat \
         any directory ending in those letters and tear down a sibling's worker. \
         Found: {calls:?}"
    );
    assert!(
        ui.contains("strip_suffix(\"service-worker.js\")"),
        "the comparison must strip the one filename it expects"
    );
}

/// The scope is named once, and the page and the worker agree on the directory.
///
/// Two independent resolutions of "where am I" — the page's `./` and the worker's
/// `new URL('./', self.location.href)`. They have to describe the same
/// directory, or the page registers a scope the worker's guard does not match.
#[test]
fn the_scope_is_a_relative_directory_shared_with_the_worker() {
    let ui = std::fs::read_to_string(root().join("src/ui.rs")).expect("reading src/ui.rs");
    assert!(
        ui.contains("const SCOPE: &str = \"./\";"),
        "the scope must be the app's own directory, relative — so one build works \
         from any subdirectory"
    );
    assert!(
        worker_template().contains("new URL('./', self.location.href)"),
        "the worker must resolve the same directory the page registered"
    );
}

/// The SVG is the icon. It is the committed source of truth, it is what the
/// page links, and both install PNGs are rasterized from it at build time, so
/// it is never replaced by a PNG and never regenerated.
///
/// The digest is the original template's, recorded so that an "improved" icon
/// is a deliberate act with a visible diff rather than a silent one.
/// The digest `assets/icon.svg` is recorded under.
///
/// The original template's own file, byte for byte. It is the source every
/// install icon is derived from, so it is pinned the way a lockfile pins a
/// dependency: a red test is the intended, deliberate way to change it.
const ICON_SHA256: &str = "fc82ef81db6f42ed9f098adb675218cfd8a980bfeb5c4e2509e08d5839f61851";

#[test]
fn the_icon_is_the_committed_svg() {
    let icon = root().join("assets/icon.svg");
    let bytes =
        std::fs::read(&icon).unwrap_or_else(|error| panic!("reading {}: {error}", icon.display()));
    assert_eq!(
        sha256_hex(&bytes),
        ICON_SHA256,
        "assets/icon.svg is the author's original icon and must not change"
    );
    assert!(
        String::from_utf8_lossy(&bytes).contains("#434343"),
        "the icon is a one-colour transparent glyph"
    );
    // The PNGs are build output: never committed, and never in the source tree
    // at all. They exist in `dist/`, which is the site and nothing else. The
    // SVG is the opposite: committed, authoritative, and published to `dist/`
    // unchanged.
    let Some(tracked) = tracked_files() else {
        return; // not a checkout: the gate's exported tree
    };
    assert!(
        tracked.lines().any(|line| line == "assets/icon.svg"),
        "assets/icon.svg is the committed source of truth and must be tracked"
    );
    for artefact in ["assets/icon-192.png", "assets/icon-512.png"] {
        assert!(
            !tracked.lines().any(|line| line == artefact),
            "{artefact} is tracked; install icons are derived at build time and must never be committed"
        );
        assert!(
            !root().join(artefact).exists(),
            "{artefact} exists in the source tree; it belongs in dist/ only"
        );
    }
}

/// Nothing generated may be tracked — not the site, not the wasm, not the
/// bindings. This is the test that would have caught them being committed.
#[test]
fn no_build_artifact_is_committed() {
    let Some(tracked) = tracked_files() else {
        return; // not a checkout
    };
    for artefact in [
        "dist",
        "dist/app.js",
        "dist/app_bg.wasm",
        "dist/index.html",
        "dist/service-worker.js",
        "dist/icon-192.png",
        "dist/icon-512.png",
        "dist/manifest.webmanifest",
    ] {
        assert!(
            !tracked.lines().any(|line| line == artefact),
            "{artefact} is tracked; generated artefacts must never be committed"
        );
    }
}

/// `target/` and `dist/` have to be ignored, or a build leaves the next commit
/// dirty and the release gate tries to ship a wasm.
#[test]
fn build_output_is_ignored() {
    let Some(_) = tracked_files() else {
        return; // not a checkout
    };
    for path in ["dist/", "target/"] {
        assert_eq!(is_ignored(path), Some(true), "{path} must be in .gitignore");
    }
}

/// The old JavaScript template's files are gone, and no build tool crept back in
/// to replace them. A copy-and-rename that kept `Makefile` and `eslint.config.mjs`
/// would be two templates in one repository, disagreeing.
#[test]
fn the_old_javascript_template_is_gone() {
    let Some(tracked) = tracked_files() else {
        return; // not a checkout
    };
    for obsolete in [
        "app.js",
        "sw.js",
        "style.css",
        "index.html",
        "manifest.json",
        "Makefile",
        "eslint.config.mjs",
    ] {
        assert!(
            !tracked.lines().any(|line| line == obsolete),
            "{obsolete} is still tracked; this is the Rust template"
        );
    }
    assert!(
        !root().join("Makefile").exists(),
        "a Makefile would bring back the build-free JS template's tooling"
    );
}

/// The one place the template states its own rules still exists, and it is
/// still a flat list of short imperatives rather than an essay. This is the
/// artefact a copy of this repo is supposed to inherit.
#[test]
fn the_instructions_are_still_a_short_flat_list() {
    let path = root().join("INSTRUCTIONS.md");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
    assert!(
        text.starts_with("# "),
        "INSTRUCTIONS.md must open with a heading"
    );
    let rules: Vec<&str> = text.lines().filter(|line| line.starts_with("- ")).collect();
    assert!(
        rules.len() >= 30 && rules.len() <= 90,
        "the rules must stay a short list, not an essay: {} rules",
        rules.len()
    );
    for required in [
        "- Build",
        "wasm",
        "wasm-bindgen",
        "prefers-color-scheme",
        "prefers-reduced-motion",
        "aria-live",
        "localStorage",
        "tests",
        // The JS-era tooling must be gone, not merely discouraged.
        "Prettier",
        "ESLint",
        "skipWaiting",
    ] {
        assert!(
            text.contains(required),
            "INSTRUCTIONS.md no longer covers {required}"
        );
    }
    // The tooling rules it replaces have to be rules too, or the reader is
    // left with a hole where verification used to be.
    for required in [
        "cargo test",
        "clippy",
        "touch build.rs",
        "dist/",
        "assets/icon.svg",
        // The family-wide rule prompted by a defect the user found: no
        // user-visible text may name the implementation.
        "name the implementation",
    ] {
        assert!(
            text.contains(required),
            "INSTRUCTIONS.md lost its verification guidance for {required}"
        );
    }
    assert!(
        !text.contains("npx "),
        "INSTRUCTIONS.md still sends the reader to npx"
    );
}

/// SHA-256 of `bytes`, lowercase hex.
///
/// Self-contained on purpose. Pinning a file's digest is worth a dependency in
/// an app; in a template whose whole argument is that it needs none, it is not
/// — so this is the FIPS 180-4 round constants and 64 rounds, and it is
/// checked against the published test vectors in the test below. If you ever
/// change this function, that test has to keep passing.
fn sha256_hex(bytes: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    // Pad to a multiple of 64 bytes: 0x80, zeros, then the bit length as u64be.
    let mut message = bytes.to_vec();
    let bits = (bytes.len() as u64) * 8;
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bits.to_be_bytes());

    for (block, _) in message.as_chunks::<64>().0.iter().map(|b| (b, ())) {
        let mut w = [0u32; 64];
        for (index, (word, _)) in block.as_chunks::<4>().0.iter().map(|w| (w, ())).enumerate() {
            w[index] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7)
                ^ w[index - 15].rotate_right(18)
                ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17)
                ^ w[index - 2].rotate_right(19)
                ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ (!e & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(choose)
                .wrapping_add(K[index])
                .wrapping_add(w[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(majority);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }

    state.iter().map(|word| format!("{word:08x}")).collect()
}

/// The digest function above is load-bearing for the icon test, so it is
/// checked against the published vectors rather than trusted.
#[test]
fn the_pinned_digest_function_is_correct() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        sha256_hex(b"The quick brown fox jumps over the lazy dog"),
        "d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592"
    );
}
