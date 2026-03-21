//! Canonical metaworld_alpha product binary entrypoint.
//!
//! The full client implementation now lives in `src/client_app.rs`.
//! The legacy example build remains as a thin compatibility shim.

fn main() {
    metaverse_core::client_app::main();
}
