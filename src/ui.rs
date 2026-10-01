// Copyright (c) 2026 Witalis Domitrz <witekdomitrz@gmail.com>
// AGPL License

//! The browser layer: the only file that talks to the DOM.
//!
//! It is deliberately thin and deliberately dumb. Every decision — what the
//! count becomes, when it stops, what the readout says, what is worth
//! storing — was made in [`crate::counter`] and is unit tested there. What is
//! left here is `get_element_by_id`, `set_text_content`, `add_event_listener`,
//! and nothing that could have been written down as a rule.
//!
//! The entry point is `#[wasm_bindgen(start)]`, so the six-line loader in
//! `ui.html` needs no argument list, no callback name and no glue: importing
//! the module runs the app. That is what "no handwritten JavaScript" means in
//! practice.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{Document, Element, HtmlButtonElement, Storage, Window};

use crate::counter::Counter;

/// The app's mutable state: one counter.
///
/// `Rc<RefCell<_>>` because wasm has no threads and the event handlers below
/// are plain closures, not `'static` Rust threads. The rule that keeps this
/// honest: a handler borrows, mutates, drops the borrow, and then renders. It
/// never renders from inside a borrow.
type State = Rc<RefCell<Counter>>;

/// The scope this app's worker is registered for.
///
/// Stated rather than inherited, and it is the one string that has to agree
/// with `src/service-worker.js`: the worker resolves its own directory the same
/// way, from `self.location`. See [`announce_offline`] for why this is not
/// optional.
const SCOPE: &str = "./";

/// Start the app.
///
/// Runs automatically when the page's dynamic import resolves, because the
/// loader in `ui.html` is `import('./app.js').then(m => m.default())` and
/// `default` is this start function.
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("no document"))?;

    let state: State = Rc::new(RefCell::new(load(&window, &document)));

    render(&document, &state);
    bind(&window, &document, &state);
    // Owned clones, because the spawned future outlives this function.
    let offline: Window = window.clone();
    spawn_local(announce_offline(offline));
    Ok(())
}

/// Read the persisted count, falling back to zero.
///
/// A `localStorage` that throws — Safari in private mode, a browser with site
/// data blocked — is a normal condition, not an error worth failing startup
/// over. The counter's own clamp and parse handle everything else.
fn load(window: &Window, _document: &Document) -> Counter {
    window
        .local_storage()
        .ok()
        .flatten()
        .map(|storage| Counter::from_stored(read(&storage).as_deref()))
        .unwrap_or_default()
}

/// Read one value, treating every failure as "not there".
fn read(storage: &Storage) -> Option<String> {
    storage.get_item(crate::counter::STORAGE_KEY).ok().flatten()
}

/// Write the count, or clear the key when the count is back to zero.
///
/// Clearing rather than writing `"0"` is what `Counter::to_stored` asks for:
/// an app the user has reset leaves nothing behind.
fn store(window: &Window, counter: Counter) {
    let Some(storage) = window.local_storage().ok().flatten() else {
        return;
    };
    let key = crate::counter::STORAGE_KEY;
    let result = match counter.to_stored() {
        Some(value) => storage.set_item(key, &value),
        None => storage.remove_item(key),
    };
    if let Err(error) = result {
        warn("could not save the count", error);
    }
}

/// Draw the current state into the shell.
///
/// Every text node and every attribute this app owns is written here, from the
/// state, on every change. Nothing in the DOM is a source of truth.
fn render(document: &Document, state: &State) {
    let counter = *state.borrow();
    let readout = counter.readout();

    // The count and its wording on one line. It is the only element the user
    // came for, and `role="status"` on the shell announces every change.
    set_text(
        document,
        "message",
        &format!("{} · {}", readout.count, readout.label),
    );

    // `disabled` is the honest way to say "this cannot go higher", and unlike a
    // greyed-out class it is what a screen reader and a keyboard both see.
    //
    // No `aria-label`: the button's visible text is already its accessible
    // name, and a second one describing the same control differently — a
    // sighted user reading "Add one", a screen reader user told "Add one tap.
    // Currently nothing yet" — is the defect, not the fix. `disabled` alone
    // conveys the ceiling.
    if let Some(button) = button(document, "hello-button") {
        button.set_disabled(counter.is_full());
    }
}

/// Bind the one button.
///
/// Handlers mutate the state and then call `render`. That is the whole
/// application loop; there is no framework, no virtual DOM and no diffing.
fn bind(window: &Window, document: &Document, state: &State) {
    let Some(button) = button(document, "hello-button") else {
        warn(
            "the shell has no #hello-button",
            JsValue::from_str("missing"),
        );
        return;
    };

    // The closures capture owned `Rc` clones, not the borrowed handles above:
    // a closure passed to `Closure::new` must be `'static`, and a listener that
    // outlives the call to `bind` is exactly the point.
    let tapped: State = Rc::clone(state);
    let handler_window: Rc<Window> = Rc::new(window.clone());
    let handler_document: Rc<Document> = Rc::new(document.clone());
    let closure = Closure::<dyn FnMut()>::new(move || {
        let next = {
            let mut counter = tapped.borrow_mut();
            *counter = counter.incremented();
            *counter
        };
        render(&handler_document, &tapped);
        store(&handler_window, next);
    });
    if let Err(error) =
        button.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
    {
        warn("could not bind the button", error);
        return;
    }
    // The closure must outlive this function or the listener becomes a dangling
    // function pointer; `forget` is the correct owner for a listener that lives
    // as long as the page does.
    closure.forget();
}

/// Register the service worker, and release any stale registration.
///
/// Deliberately silent. This used to report the outcome on the page, and the
/// page carried a line saying the app was ready for offline use — which is a
/// thing the reader never asked to be told, on a line that existed only to hold
/// it. The worker is a background concern: it either works, and the user
/// notices nothing, or it does not, and the app still runs.
///
/// A registration failure is a console warning and nothing else. Nothing is
/// lost that the reader can act on: the app works, online, exactly as before.
///
/// Called from an `async` start function, but it does not block the app: the
/// registration is awaited, the page is interactive before it finishes, because
/// offline support is not worth a blank screen.
async fn announce_offline(window: Window) {
    let container = window.navigator().service_worker();

    // The scope is stated rather than inherited. Left to itself, a
    // registration's scope is the directory of the page that registered it,
    // which is right today and silently wrong the moment the app is published
    // somewhere else, or opened through a path that resolves higher up the
    // origin. A worker registered for the whole origin does not serve just its
    // own pages -- it answers for every page on that origin, including the
    // ones that have nothing to do with it. Naming the scope keeps that claim
    // as small as the app.
    let options = web_sys::RegistrationOptions::new();
    options.set_scope(SCOPE);
    if let Err(error) =
        JsFuture::from(container.register_with_options("./service-worker.js", &options)).await
    {
        warn("service worker registration failed", error);
        return;
    }

    release_stale_registrations(&window).await;
}

/// Hand this app's own URLs back to the current worker.
///
/// A service worker is a registration, and a registration outlives the page
/// that made it: it is kept by the browser, not by the tab, and it keeps
/// answering for its scope until something explicitly unregisters it. That is
/// how a page on this origin can come to be served by a worker installed for a
/// *different* page, long after the app that installed it was closed. A stale
/// registration is not corrected by a reload, by a newer version of the app, or
/// by a newer worker installing itself -- the newer worker only takes control
/// where its own scope reaches, and a wider stale one is still in the way.
///
/// So the repair is explicit: find any registration whose scope covers this
/// app's directory but is not this app's directory, and unregister it. This
/// app's own registration is left alone, and so is every other app on the
/// origin -- each is scoped to its own directory, and a sibling that never
/// covered us is not ours to remove.
///
/// Failures are ignored on purpose. This is best-effort cleanup of state this
/// app did not create, and a browser that refuses leaves the user no worse
/// off: the app still runs and still caches its own assets.
async fn release_stale_registrations(window: &Window) {
    let container = window.navigator().service_worker();
    let Ok(registrations) = JsFuture::from(container.get_registrations()).await else {
        return;
    };
    let Ok(array) = registrations.dyn_into::<js_sys::Array>() else {
        return;
    };

    // This app's own directory, as an absolute URL with a trailing slash. The
    // app is served from a subdirectory and every URL of ours is inside it.
    let Ok(home) = window.location().href() else {
        return;
    };
    let Ok(ours) = web_sys::Url::new_with_base(&home, "./") else {
        return;
    };
    let ours = ours.href();

    for entry in array.iter() {
        let Ok(registration) = entry.dyn_into::<web_sys::ServiceWorkerRegistration>() else {
            continue;
        };
        let scope = registration.scope();
        // Leave alone any scope that is this app's own, or narrower: a sibling
        // app mounted inside this directory is legitimate and separate, and
        // nothing there can intercept us. One test, because the two cases are
        // the same one: `ours` begins with `scope`.
        if ours.starts_with(&scope) {
            continue;
        }
        // What is left is a scope that is a *strict* prefix of ours: a worker
        // that would be consulted for this app's URLs while being registered
        // for more than this app. A worker is consulted for a URL exactly when
        // its scope is a prefix of that URL, which is the test above inverted.
        //
        // Of those, only our own worker qualifies: a different app's worker
        // lives in a different directory, so unregistering it would break the
        // app it belongs to.
        let script = registration
            .active()
            .map(|worker| worker.script_url())
            .unwrap_or_default();
        if script_belongs_to_app(&script, &ours) {
            match registration.unregister() {
                Ok(promise) => {
                    let _ = JsFuture::from(promise).await;
                }
                Err(error) => warn("stale service worker could not be released", error),
            }
        }
    }
}

/// Whether a worker script at `script` is this app's own worker, registered for
/// more of the origin than this app's directory.
///
/// A wider scope means the script sits at the root of this app's own directory
/// rather than anywhere below it: a sibling app's worker is in a sibling
/// directory and does not match. `strip_suffix`, not `trim_end_matches` — the
/// latter strips a *set of characters*, so it would happily eat a directory
/// named `...e-worker.js` and call it ours.
fn script_belongs_to_app(script: &str, ours: &str) -> bool {
    match web_sys::Url::new(script) {
        Ok(url) => url.href().strip_suffix("service-worker.js") == Some(ours),
        Err(_) => false,
    }
}

/// A document element, or `None`.
///
/// Never panics: the shell is a separate file that someone will edit, and a
/// missing id should degrade the app, not abort it.
fn element(document: &Document, id: &str) -> Option<Element> {
    document.get_element_by_id(id)
}

/// The same, typed, so button methods are available without a cast.
fn button(document: &Document, id: &str) -> Option<HtmlButtonElement> {
    element(document, id)?.dyn_into().ok()
}

/// Replace an element's text.
fn set_text(document: &Document, id: &str, text: &str) {
    if let Some(element) = element(document, id) {
        element.set_text_content(Some(text));
    }
}

/// Report something recoverable. Never a panic, never silent.
fn warn(context: &str, error: JsValue) {
    web_sys::console::warn_2(&JsValue::from_str(context), &error);
}
