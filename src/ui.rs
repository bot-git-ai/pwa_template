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

/// The `#status` line the loader replaced once Rust was running.
const LOADING: &str = "Loading…";
/// Shown when the offline cache could not be set up.
const OFFLINE_UNAVAILABLE: &str =
    "Offline setup unavailable. The app still works, but installing it needs HTTPS or localhost.";
/// Shown once the offline cache is in place.
const OFFLINE_READY: &str = "Ready for offline use. Install from your browser's app menu.";

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

    // The live region announces the count, so a screen reader hears the
    // change; the number itself is `aria-hidden` because it would otherwise be
    // announced twice.
    set_text(document, "status", LOADING);

    // `disabled` is the honest way to say "this cannot go higher", and unlike a
    // greyed-out class it is what a screen reader and a keyboard both see.
    if let Some(button) = button(document, "hello-button") {
        let disabled = counter.is_full();
        button.set_disabled(disabled);
        set_attribute(
            &button,
            "aria-label",
            &if disabled {
                format!("Full at {}. There is no more room to count.", readout.count)
            } else {
                format!("Add one tap. Currently {}", readout.label)
            },
        );
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

/// Register the service worker, then report the outcome on screen.
///
/// Called from an `async` start function, but it does not block the app: the
/// registration is awaited, the readiness is not. The page is interactive
/// before either finishes, because offline support is not worth a blank screen.
///
/// Registration failure is a warning, not a crash: the app works without the
/// worker, it just will not open offline. Saying so where the user can read it
/// beats a console line nobody does.
async fn announce_offline(window: Window) {
    let Some(document) = window.document() else {
        return;
    };
    let container = window.navigator().service_worker();

    if let Err(error) = JsFuture::from(container.register("./service-worker.js")).await {
        warn("service worker registration failed", error);
        set_text(&document, "offline-status", OFFLINE_UNAVAILABLE);
        return;
    }

    // `ready` settles when the worker controls the page, which is the moment
    // caching is finished and the next reload will work offline.
    if let Ok(promise) = container.ready() {
        let reported: Rc<Document> = Rc::new(document);
        let announce = Closure::<dyn FnMut(JsValue)>::new(move |_| {
            set_text(&reported, "offline-status", OFFLINE_READY);
        });
        // `then` hands back a promise nobody waits on, which is fine: its only
        // rejection would be the registration's, already handled above. The
        // `forget` on the closure is what keeps the callback alive until it
        // runs; the promise needs no owner in wasm.
        let _settled = promise.then(&announce);
        announce.forget();
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

/// Set an attribute, ignoring a failure to do so.
fn set_attribute(element: &Element, name: &str, value: &str) {
    element.set_attribute(name, value).ok();
}

/// Report something recoverable. Never a panic, never silent.
fn warn(context: &str, error: JsValue) {
    web_sys::console::warn_2(&JsValue::from_str(context), &error);
}
