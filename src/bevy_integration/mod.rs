//! Bevy integration layer for polyphase.
//!
//! This module is gated behind `#[cfg(feature = "bevy")]` and provides:
//!
//! - [`LoadfuPlugin`] — registers all asset loaders with Bevy
//! - [`StlLoader`], [`ObjLoader`], [`GlbLoader`], [`PlyLoader`] — thin `AssetLoader` wrappers
//!   that delegate to core parsers and convert `MeshData` → `bevy::Mesh`
//! - `From<MeshData> for bevy::Mesh` — the conversion bridge
//!
//! # Usage
//!
//! ```ignore
//! use polyphase::bevy_integration::LoadfuPlugin;
//!
//! app.add_plugins(LoadfuPlugin);
//!
//! // Then in a system:
//! let handle: Handle<Mesh> = asset_server.load("model.stl");
//! ```

pub mod asset_loaders;
pub mod into_mesh;
pub mod plugin;

// ── Public API re-exports ─────────────────────────────────────────────

pub use asset_loaders::{
    GlbLoader, GlbLoaderSettings, ObjLoader, ObjLoaderSettings, PlyLoader, PlyLoaderSettings,
    StlLoader, StlLoaderSettings,
};
pub use plugin::LoadfuPlugin;
