// Copyright (c) 2026 Witalis Domitrz <witekdomitrz@gmail.com>
// AGPL License

//! A minimal Rust PWA, and the starting point for the next one.
//!
//! The split this crate exists to demonstrate:
//!
//! * [`counter`] — the app's logic. No `web-sys`, no `js-sys`, no
//!   `wasm-bindgen`, so it compiles natively and is unit tested by plain
//!   `cargo test`.
//! * [`ui`] — the browser layer, compiled only for `wasm32-unknown-unknown`.
//!   Every `document`, `localStorage` and service-worker call lives here, and
//!   nothing else does.
//!
//! `cargo build --release` writes the whole site to `dist/` through `build.rs`.
//! There is no server, no binary and no run step: the build is the product.

#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

pub mod counter;

#[cfg(target_arch = "wasm32")]
pub mod ui;

pub use counter::{Counter, Readout, MAX, MIN, STORAGE_KEY};
