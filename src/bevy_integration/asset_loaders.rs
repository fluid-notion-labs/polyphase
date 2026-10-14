//! Bevy [`AssetLoader`] wrappers for STL, OBJ, and GLB formats.
//!
//! Each loader is a thin wrapper that:
//! 1. Reads bytes from the Bevy asset reader
//! 2. Delegates to the corresponding core parser (`parsers::stl`, etc.)
//! 3. Converts `MeshData` → `bevy::Mesh` via the `From` impl
//!
//! All parsing logic lives in the core layer — these wrappers just bridge
//! the Bevy asset pipeline to polyphase's pure parsers.

use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext};
use bevy::mesh::Mesh;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::error::{GlbError, ObjError, PlyError, StlError};
use crate::parsers::{glb, obj, ply, stl};

// ===========================================================================
// STL
// ===========================================================================

/// Settings for the STL asset loader.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StlLoaderSettings {
    /// If true, recompute smooth vertex normals from geometry instead of
    /// using the per-face normals stored in the STL file.
    #[serde(default)]
    pub recompute_normals: bool,
}

/// Bevy [`AssetLoader`] for `.stl` files.
///
/// Delegates to [`parsers::stl::parse`](crate::parsers::stl::parse) and
/// converts the resulting [`MeshData`](crate::MeshData) into a Bevy [`Mesh`].
#[derive(Default, Debug, Clone, Copy, bevy::reflect::TypePath)]
pub struct StlLoader;

impl AssetLoader for StlLoader {
    type Asset = Mesh;
    type Settings = StlLoaderSettings;
    type Error = StlError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        settings: &StlLoaderSettings,
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Mesh, StlError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await.map_err(StlError::Io)?;

        let opts = stl::StlOptions {
            recompute_normals: settings.recompute_normals,
        };
        let mesh_data = stl::parse_with_options(&bytes, &opts)?;

        Ok(mesh_data.into())
    }

    fn extensions(&self) -> &[&str] {
        &["stl"]
    }
}

// ===========================================================================
// OBJ
// ===========================================================================

/// Settings for the OBJ asset loader.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjLoaderSettings {
    /// If true, recompute smooth vertex normals from geometry instead of
    /// using normals from the OBJ file (if present).
    #[serde(default)]
    pub recompute_normals: bool,

    /// If true, triangulate non-triangle faces (quads, n-gons).
    /// Defaults to `true` since Bevy requires triangle meshes.
    #[serde(default = "default_true")]
    pub triangulate: bool,
}

impl Default for ObjLoaderSettings {
    fn default() -> Self {
        Self {
            recompute_normals: false,
            triangulate: true,
        }
    }
}

fn default_true() -> bool {
    true
}

/// Bevy [`AssetLoader`] for `.obj` files.
///
/// Delegates to [`parsers::obj::parse`](crate::parsers::obj::parse) and
/// converts the resulting [`MeshData`](crate::MeshData) into a Bevy [`Mesh`].
/// Multi-object OBJ files are merged into a single mesh.
#[derive(Default, Debug, Clone, Copy, bevy::reflect::TypePath)]
pub struct ObjLoader;

impl AssetLoader for ObjLoader {
    type Asset = Mesh;
    type Settings = ObjLoaderSettings;
    type Error = ObjError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        settings: &ObjLoaderSettings,
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Mesh, ObjError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await.map_err(ObjError::Io)?;

        let opts = obj::ObjOptions {
            recompute_normals: settings.recompute_normals,
            triangulate: settings.triangulate,
            // MTL is not resolved by the core parser; the Bevy layer could
            // resolve sibling files via the load context in the future.
            mtl: None,
        };
        let mesh_data = obj::parse_with_options(&bytes, &opts)?;

        Ok(mesh_data.into())
    }

    fn extensions(&self) -> &[&str] {
        &["obj"]
    }
}

// ===========================================================================
// GLB / glTF
// ===========================================================================

/// Settings for the GLB/glTF asset loader.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlbLoaderSettings {
    /// If true, merge all primitives into a single mesh.
    /// If false, only the first primitive is returned as the main asset;
    /// additional primitives are added as labeled sub-assets.
    /// Defaults to `true` for simplicity — most consumers want one mesh.
    #[serde(default = "default_true")]
    pub merge: bool,
}

impl Default for GlbLoaderSettings {
    fn default() -> Self {
        Self { merge: true }
    }
}

/// Bevy [`AssetLoader`] for `.glb` (binary glTF) files.
///
/// Delegates to [`parsers::glb::parse`](crate::parsers::glb::parse) and
/// converts the resulting [`MeshData`](crate::MeshData) into Bevy [`Mesh`]
/// assets.
///
/// By default, all mesh primitives are merged into a single mesh. Set
/// `merge: false` in settings to get individual sub-assets.
///
/// **Note:** This loader handles `.glb` only (self-contained binary).
/// Multi-file `.gltf` (with external `.bin` buffers and texture references)
/// must go through Bevy's native `GltfLoader` which can resolve external
/// URIs.  polyphase's byte-slice parser cannot resolve external references.
#[derive(Default, Debug, Clone, Copy, bevy::reflect::TypePath)]
pub struct GlbLoader;

impl AssetLoader for GlbLoader {
    type Asset = Mesh;
    type Settings = GlbLoaderSettings;
    type Error = GlbError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        settings: &GlbLoaderSettings,
        load_context: &mut LoadContext<'_>,
    ) -> Result<Mesh, GlbError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await.map_err(GlbError::Io)?;

        if settings.merge {
            // Merge all primitives into one mesh
            let mesh_data = glb::parse_merged(&bytes)?;
            Ok(mesh_data.into())
        } else {
            // Multi-mesh: first primitive is the main asset, rest are labeled sub-assets
            let meshes = glb::parse(&bytes)?;
            let mut iter = meshes.into_iter().enumerate();

            // First primitive → main asset
            let (_, first) = iter.next().ok_or(GlbError::EmptyFile)?;
            let main_mesh: Mesh = first.into();

            // Remaining primitives → labeled sub-assets
            for (i, mesh_data) in iter {
                let label = format!("Primitive{i}");
                let sub_mesh: Mesh = mesh_data.into();
                let _ = load_context.labeled_asset_scope(label, |_| Ok::<Mesh, GlbError>(sub_mesh));
            }

            Ok(main_mesh)
        }
    }

    fn extensions(&self) -> &[&str] {
        &["glb"]
    }
}

// ===========================================================================
// PLY
// ===========================================================================

/// Settings for the PLY asset loader.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlyLoaderSettings {
    /// If true, recompute smooth vertex normals from geometry instead of
    /// using normals from the PLY file (if present).
    #[serde(default)]
    pub recompute_normals: bool,
}

/// Bevy [`AssetLoader`] for `.ply` files.
///
/// Delegates to [`parsers::ply::parse`](crate::parsers::ply::parse) and
/// converts the resulting [`MeshData`](crate::MeshData) into a Bevy [`Mesh`].
#[derive(Default, Debug, Clone, Copy, bevy::reflect::TypePath)]
pub struct PlyLoader;

impl AssetLoader for PlyLoader {
    type Asset = Mesh;
    type Settings = PlyLoaderSettings;
    type Error = PlyError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        settings: &PlyLoaderSettings,
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Mesh, PlyError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await.map_err(PlyError::Io)?;

        let opts = ply::PlyOptions {
            recompute_normals: settings.recompute_normals,
        };
        let mesh_data = ply::parse_with_options(&bytes, &opts)?;

        Ok(mesh_data.into())
    }

    fn extensions(&self) -> &[&str] {
        &["ply"]
    }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── Settings defaults ─────────────────────────────────────────────

    #[test]
    fn stl_settings_default() {
        let s = StlLoaderSettings::default();
        assert!(!s.recompute_normals);
    }

    #[test]
    fn obj_settings_default() {
        let s = ObjLoaderSettings::default();
        assert!(!s.recompute_normals);
        assert!(s.triangulate);
    }

    #[test]
    fn glb_settings_default() {
        let s = GlbLoaderSettings::default();
        assert!(s.merge);
    }

    // ── Extensions ────────────────────────────────────────────────────

    #[test]
    fn stl_loader_extensions() {
        let loader = StlLoader;
        assert_eq!(loader.extensions(), &["stl"]);
    }

    #[test]
    fn obj_loader_extensions() {
        let loader = ObjLoader;
        assert_eq!(loader.extensions(), &["obj"]);
    }

    #[test]
    fn glb_loader_extensions() {
        let loader = GlbLoader;
        assert_eq!(loader.extensions(), &["glb"]);
    }

    // ── PLY ───────────────────────────────────────────────────────────

    #[test]
    fn ply_settings_default() {
        let s = PlyLoaderSettings::default();
        assert!(!s.recompute_normals);
    }

    #[test]
    fn ply_loader_extensions() {
        let loader = PlyLoader;
        assert_eq!(loader.extensions(), &["ply"]);
    }
}
