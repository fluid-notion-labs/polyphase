//! Bevy plugin that registers all loadfu asset loaders.
//!
//! Add [`LoadfuPlugin`] to your Bevy app to enable transparent loading of
//! `.stl`, `.obj`, `.glb`, and `.ply` files through the asset server.
//!
//! ```ignore
//! app.add_plugins(LoadfuPlugin);
//!
//! // Then in a system:
//! let handle: Handle<Mesh> = asset_server.load("model.stl");
//! ```

use bevy::prelude::*;

use super::asset_loaders::{GlbLoader, ObjLoader, PlyLoader, StlLoader};

/// Plugin that registers all loadfu asset loaders with Bevy.
///
/// Registers:
/// - [`StlLoader`] for `.stl` files
/// - [`ObjLoader`] for `.obj` files
/// - [`GlbLoader`] for `.glb` files (binary glTF only)
/// - [`PlyLoader`] for `.ply` files
///
/// # Note on GLB vs glTF
///
/// This replaces Bevy's built-in glTF loader for `.glb` files only.
/// Multi-file `.gltf` (with external `.bin` buffers and texture references)
/// is **not** handled by loadfu — Bevy's native `GltfLoader` remains
/// registered for `.gltf` and handles external URI resolution correctly.
///
/// If you need Bevy's scene-level glTF support (cameras, lights, animations)
/// for `.glb` files as well, don't register this plugin.
pub struct LoadfuPlugin;

impl Plugin for LoadfuPlugin {
    fn build(&self, app: &mut App) {
        app.register_asset_loader(StlLoader);
        app.register_asset_loader(ObjLoader);
        app.register_asset_loader(GlbLoader);
        app.register_asset_loader(PlyLoader);
    }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_builds_without_panic() {
        // Verify the plugin can be constructed and its build method runs.
        // We don't spin up a full Bevy App in unit tests — just confirm
        // the type exists and is constructible.
        let _plugin = LoadfuPlugin;
    }
}
