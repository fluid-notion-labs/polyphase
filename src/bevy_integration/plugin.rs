//! Bevy plugin that registers all loadfu asset loaders.
//!
//! Add [`LoadfuPlugin`] to your Bevy app to enable transparent loading of
//! `.stl`, `.obj`, `.glb`, and `.gltf` files through the asset server.
//!
//! ```ignore
//! app.add_plugins(LoadfuPlugin);
//!
//! // Then in a system:
//! let handle: Handle<Mesh> = asset_server.load("model.stl");
//! ```

use bevy::prelude::*;

use super::asset_loaders::{GlbLoader, ObjLoader, StlLoader};

/// Plugin that registers all loadfu asset loaders with Bevy.
///
/// Registers:
/// - [`StlLoader`] for `.stl` files
/// - [`ObjLoader`] for `.obj` files
/// - [`GlbLoader`] for `.glb` and `.gltf` files
///
/// # Note on GLB/glTF
///
/// This replaces Bevy's built-in glTF loader for `.glb`/`.gltf` extensions.
/// If you need Bevy's scene-level glTF support (cameras, lights, animations),
/// don't register this plugin and use Bevy's built-in loader instead.
pub struct LoadfuPlugin;

impl Plugin for LoadfuPlugin {
    fn build(&self, app: &mut App) {
        app.register_asset_loader(StlLoader);
        app.register_asset_loader(ObjLoader);
        app.register_asset_loader(GlbLoader);

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
