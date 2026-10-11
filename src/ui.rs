// Copyright (c) 2026 Witalis Domitrz <witekdomitrz@gmail.com>
// AGPL License

//! The browser layer: the only file that talks to the DOM.
//!
//! Every decision — what the count becomes, when it stops, what the readout
//! says, what is worth storing — was made in [`crate::counter`] and is unit
//! tested there. What is left here is `get_element_by_id`, `set_text_content`,
//! `add_event_listener`, and nothing that could have been written down as a
//! rule.
//!
//! The entry point is `#[wasm_bindgen(start)]`, so the loader in `ui.html`
//! needs no argument list, no callback name and no glue: importing the module
//! runs the app.

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
///
/// # Errors
///
/// When there is no window or no document to build the app in — a headless
/// or non-browser context. The rejection is what the loader's `.catch`
/// reports; there is nothing else to do with it.
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
        warn("could not save the count", &error);
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
    // name, and a second one describing the same control differently is the
    // defect, not the fix.
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
            &JsValue::from_str("missing"),
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
        warn("could not bind the button", &error);
        return;
    }
    // `forget` is the correct owner here: the closure must outlive `bind` or the
    // listener becomes a dangling function pointer.
    closure.forget();
}

/// Register the service worker, and release any stale registration.
///
/// Deliberately silent. This used to report the outcome on the page, on a line
/// that existed only to hold it — and that line only ever reported good news
/// the reader did not ask about. The worker is a background concern: either it
/// works and the user notices nothing, or it does not and the app still runs.
/// Nothing is lost that the reader can act on.
///
/// Registration is awaited, but not before the app is interactive: offline
/// support is not worth a blank screen.
async fn announce_offline(window: Window) {
    let container = window.navigator().service_worker();

    // The scope is stated rather than inherited. Left to itself, a registration's
    // scope is the directory of the page that registered it: right today, and
    // silently wrong once the app is published somewhere else or opened
    // through a path resolving higher up the origin. A worker registered for
    // the whole origin answers for every page on it, including pages that have
    // nothing to do with this app. Naming the scope keeps that claim as small
    // as the app.
    let options = web_sys::RegistrationOptions::new();
    options.set_scope(SCOPE);
    let registered =
        JsFuture::from(container.register_with_options("./service-worker.js", &options)).await;
    let registered = match registered {
        Ok(value) => value,
        Err(error) => {
            warn("service worker registration failed", &error);
            return;
        }
    };

    // The promise resolves to the registration, which is the handle the
    // update nudge watches.
    if let Ok(registered) = registered.dyn_into::<web_sys::ServiceWorkerRegistration>() {
        nudge_worker_update(&registered).await;
    }

    release_stale_registrations(&window).await;
}

/// Ask this app's service worker to update as soon as it can take over.
///
/// The worker never swaps itself in unasked: an update waits for every page
/// it controls to close, because a live app must not run half old code under
/// half new. Left exactly there, an installed copy would update only when
/// every opening of it has closed — on a phone, possibly never. So this
/// asks, and the ask is safe by construction: the sender is the page itself,
/// which then reloads as the swap happens. The worker answers the message
/// with `skipWaiting`, the browser fires `controllerchange`, the page reloads
/// exactly once, and the old worker is redundant. No client ever straddles
/// the two versions — the one that asked is the one that stepped aside.
///
/// The message is sent for a worker already installed and waiting, and for
/// one still installing — sent on `installed` in the second case, after a
/// `statechange` wait — and a `controllerchange` arriving with no reload of
/// ours in flight is answered by one reload, so a version swap is never
/// better than one reload away, whatever path it came in on.
///
/// Best effort like everything around it: a failure is a console line, and an
/// app that stays on this version still runs.
async fn nudge_worker_update(registration: &web_sys::ServiceWorkerRegistration) {
    let Some(window) = web_sys::window() else {
        return;
    };

    // The reload that follows a swap. The flag keeps it one-per-swap, so a
    // late `updatefound` from the same registration cannot reload again
    // underneath it.
    watch_for_controller_change(&window);

    // A worker installed and waiting for pages like this one to close. This
    // is the state an update is left in when a visit ended before its
    // handover: nothing new will arrive until the served bytes change again,
    // so this is the one chance to ask it in.
    if let Some(worker) = registration.waiting() {
        if worker.state() == web_sys::ServiceWorkerState::Installed {
            ask_worker_in(&worker);
            return;
        }
    }

    // A worker installing right now — this visit's own update check found
    // one — or one that arrives later while the page stands.
    let installing = match registration.installing() {
        Some(worker) => Some(worker),
        None => update_found(registration).await,
    };
    let Some(worker) = installing else {
        return;
    };
    if wait_for_state_change(&worker).await.is_err() {
        warn("could not wait for the worker to install", &JsValue::NULL);
        return;
    }
    if worker.state() == web_sys::ServiceWorkerState::Installed {
        ask_worker_in(&worker);
    }
}

/// Send the message the worker answers with `skipWaiting`.
fn ask_worker_in(worker: &web_sys::ServiceWorker) {
    worker.post_message(&JsValue::from_str("SKIP_WAITING")).ok();
}

/// Reload once when the worker in charge changes.
///
/// The guard is the in-flight flag: an ask sent twice would fire the event
/// once, and a second registration's late update could fire it again under a
/// page already reloading. One reload per swap, and never two in a row, is
/// what keeps this from becoming the loop the naive version of this code is:
/// a page that reloads unasked on `installed` never lets the waiting worker
/// activate, because a reloading page still counts as a client.
fn watch_for_controller_change(window: &web_sys::Window) {
    thread_local! {
        static WATCHING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    if WATCHING.with(std::cell::Cell::get) {
        return;
    }
    WATCHING.with(|watching| watching.set(true));

    let container = window.navigator().service_worker();
    let handler = Closure::<dyn FnMut(JsValue)>::new(move |_event: JsValue| {
        if let Some(window) = web_sys::window() {
            window.location().reload().ok();
        }
    });
    let _ = container
        .add_event_listener_with_callback("controllerchange", handler.as_ref().unchecked_ref());
    // Never dropped: exactly one listener for the page's lifetime, and the
    // guard above makes re-registering a no-op.
    handler.forget();
}

/// The worker this registration announces through `updatefound`, if one
/// arrives while the page stands.
///
/// The event carries the registration, not the worker; the worker is its
/// `installing` one. `None` resolves the wait empty if the registration is
/// ever torn down between the two states this cares about.
async fn update_found(
    registration: &web_sys::ServiceWorkerRegistration,
) -> Option<web_sys::ServiceWorker> {
    let event = JsFuture::from(js_sys::Promise::new(&mut |resolve, _reject| {
        let resolved = Closure::<dyn FnMut(JsValue)>::new(move |value: JsValue| {
            resolve.call1(&JsValue::NULL, &value).ok();
        });
        let _ = registration
            .add_event_listener_with_callback("updatefound", resolved.as_ref().unchecked_ref());
        // Never dropped: the promise settles at most once, and the
        // registration outlives the page anyway.
        resolved.forget();
    }))
    .await
    .ok()?;
    let event = event.dyn_into::<web_sys::Event>().ok()?;
    let announced = event
        .target()?
        .dyn_into::<web_sys::ServiceWorkerRegistration>()
        .ok()?;
    announced.installing()
}

/// Resolve at the worker's next state change, whichever it is.
async fn wait_for_state_change(worker: &web_sys::ServiceWorker) -> Result<(), JsValue> {
    JsFuture::from(js_sys::Promise::new(&mut |resolve, _reject| {
        let resolved = Closure::<dyn FnMut(JsValue)>::new(move |value: JsValue| {
            resolve.call1(&JsValue::NULL, &value).ok();
        });
        let _ = worker
            .add_event_listener_with_callback("statechange", resolved.as_ref().unchecked_ref());
        // Never dropped, like the one above: at most one state change is
        // waited for here, and the worker outlives the page.
        resolved.forget();
    }))
    .await
    .map(|_| ())
}

/// Hand this app's own URLs back to the current worker.
///
/// A registration is kept by the browser, not the tab, so it outlives the page
/// that made it and keeps answering for its scope until something explicitly
/// unregisters it. That is how a page on this origin comes to be served by a
/// worker installed for a *different* app, long after that app was closed — and
/// a reload, a newer version or a newer worker installing itself cannot fix
/// it, because the newer worker only takes control where its own scope reaches.
///
/// So: unregister any registration whose scope covers this app's directory but
/// is not this app's directory. Failures are ignored — this is best-effort
/// cleanup of state this app did not create, and a browser that refuses leaves
/// the user no worse off.
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
        // What is left is a scope that is a *strict* prefix of ours: a worker that
        // would be consulted for this app's URLs while registered for more of
        // the origin. Only our own worker qualifies — a different app's worker
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
                Err(error) => warn("stale service worker could not be released", &error),
            }
        }
    }
}

/// Whether a worker script at `script` is this app's own worker, registered for
/// more of the origin than this app's directory.
///
/// A wider scope means the script sits at the root of this app's own directory
/// rather than below it: a sibling app's worker is in a sibling directory and
/// does not match. `strip_suffix`, not `trim_end_matches` — the latter strips a
/// *set of characters*, so it would eat a directory named `...e-worker.js` and
/// call it ours.
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

/// The typed form of [`element`], so button methods are available without a cast.
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
fn warn(context: &str, error: &JsValue) {
    web_sys::console::warn_2(&JsValue::from_str(context), error);
}
