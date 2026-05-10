// WASM entry point.
//
// On native targets this lib is just a no-op stub; the bin in main.rs is the
// real entry. On wasm32 it provides the `start` function that wasm-bindgen
// uses to initialise the browser-side ratzilla renderer.

// Some helpers (e.g. POLL_IDLE, Test::resume_at) are only used by the native
// event loop in main.rs. They share a compilation unit with the wasm lib but
// aren't reached on that target, so silence the dead-code lint instead of
// littering each item with an attribute.
#![allow(dead_code)]

#[cfg(target_arch = "wasm32")]
mod config;
#[cfg(target_arch = "wasm32")]
mod content;
#[cfg(target_arch = "wasm32")]
mod key;
#[cfg(target_arch = "wasm32")]
mod keyboard;
#[cfg(target_arch = "wasm32")]
mod resources;
#[cfg(target_arch = "wasm32")]
mod test;
#[cfg(target_arch = "wasm32")]
mod time;
#[cfg(target_arch = "wasm32")]
mod title;
#[cfg(target_arch = "wasm32")]
mod ui;
#[cfg(target_arch = "wasm32")]
mod web;
