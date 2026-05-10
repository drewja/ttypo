// Cross-target re-exports. On wasm only Duration and Instant are reached
// (the SystemTime/UNIX_EPOCH symbols are used by progress.rs, which is
// native-only), so the wasm build re-exports them with the unused-import
// lint silenced.
#[cfg(not(target_arch = "wasm32"))]
pub use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(target_arch = "wasm32")]
#[allow(unused_imports)]
pub use web_time::{Duration, Instant, SystemTime, UNIX_EPOCH};
