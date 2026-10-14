//! **polyphase** — format-agnostic 3D asset loading.
//!
//! Single crate, Bevy behind a feature flag. Core is useful standalone
//! (CLI tools, pipeline scripts) without pulling in any engine dependency.
//!
//! # Architecture
//!
//! - **Core layer** (`mesh_data`, `format`, `capabilities`, `error`, `parsers`) —
//!   pure logic. Parsers take `&[u8]` and return `Result<MeshData>`. No Bevy.
//! - **Bevy layer** (`bevy/`) — `#[cfg(feature = "bevy")]` only. Thin
//!   `AssetLoader` wrappers that delegate to core parsers, plus
//!   `Into<bevy::Mesh>` for `MeshData`.
//!
//! # Feature flags
//!
//! | Flag  | Effect |
//! |-------|--------|
//! | `bevy` | Enables `bevy/` integration layer (asset loaders, `Into<Mesh>`). |
//!
//! Default features: *none*. `cargo check -p polyphase` compiles with zero Bevy
//! dependency.
//!
//! # Quick start (core only)
//!
//! ```rust
//! use polyphase::{MeshData, MeshFormat};
//!
//! // Detect format from extension
//! let format = MeshFormat::from_extension("stl").unwrap();
//! assert_eq!(format.label(), "STL");
//! ```

// ── Module declarations ───────────────────────────────────────────────

pub mod capabilities;
pub mod error;
pub mod format;
pub mod mesh_data;
pub mod parsers;

#[cfg(feature = "bevy")]
pub mod bevy_integration;

// ── Top-level dispatch ────────────────────────────────────────────────

/// Parse bytes of a known format into a full [`AssetData`] — meshes,
/// materials, textures, and (where the format has one) a node hierarchy.
///
/// Free functions, not a `Parser` trait: a handful of formats with
/// heterogeneous options don't earn one.
///
/// # Errors
///
/// Returns the per-format error wrapped in [`LoadError`], or
/// [`LoadError::UnsupportedFormat`] if the format is known but has no
/// parser compiled in.
pub fn parse_asset(bytes: &[u8], format: MeshFormat) -> Result<AssetData, LoadError> {
    match format {
        MeshFormat::Stl => {
            let mesh = parsers::stl::parse(bytes)?;
            Ok(AssetData::single(mesh))
        }
        MeshFormat::Obj => Ok(parsers::obj::parse_asset(
            bytes,
            &parsers::obj::ObjOptions::default(),
        )?),
        MeshFormat::Gltf | MeshFormat::Glb => Ok(parsers::glb::parse_asset(bytes)?),
        MeshFormat::Ply => {
            let mesh = parsers::ply::parse(bytes)?;
            Ok(AssetData::single(mesh))
        }
    }
}

/// One-call load: detect the format from the path hint (extension),
/// falling back to content sniffing, then [`parse_asset`].
///
/// The hint is just a filename used for its extension — nothing is read
/// from disk. Without a hint (or with an unrecognised extension),
/// [`MeshFormat::sniff`] inspects the bytes themselves.
///
/// # Errors
///
/// Returns [`LoadError::UnsupportedFormat`] if neither hint nor sniffing
/// identifies a supported format.
pub fn load(bytes: &[u8], hint: Option<&std::path::Path>) -> Result<AssetData, LoadError> {
    let format = hint
        .and_then(MeshFormat::from_path)
        .or_else(|| MeshFormat::sniff(bytes))
        .ok_or(LoadError::UnsupportedFormat { detected: None })?;
    parse_asset(bytes, format)
}

// ── Public API re-exports ─────────────────────────────────────────────

pub use capabilities::{
    AlphaMode, AssetCapabilities, AssetData, MaterialData, NodeData, ParseCost, SamplerData,
    TextureData, TextureMime, TextureSlot, TextureSource, TopologyKind, GL_CLAMP_TO_EDGE,
    GL_LINEAR, GL_LINEAR_MIPMAP_LINEAR, GL_LINEAR_MIPMAP_NEAREST, GL_MIRRORED_REPEAT, GL_NEAREST,
    GL_NEAREST_MIPMAP_LINEAR, GL_NEAREST_MIPMAP_NEAREST, GL_WRAP_REPEAT,
};
pub use error::{GlbError, LoadError, ObjError, PlyError, StlError};
pub use format::MeshFormat;
pub use mesh_data::{MeshData, MeshDataError};
pub use parsers::{glb, obj, ply, stl};

// ── Bevy integration re-exports ───────────────────────────────────────

#[cfg(feature = "bevy")]
pub use bevy_integration::{
    GlbLoader, GlbLoaderSettings, LoadfuPlugin, ObjLoader, ObjLoaderSettings, PlyLoader,
    PlyLoaderSettings, StlLoader, StlLoaderSettings,
};

// ── Shared test utilities ─────────────────────────────────────────────

#[cfg(test)]
pub(crate) mod test_util {
    use crate::MeshData;

    /// Load a fixture file from `tests/fixtures/{name}`.
    pub fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!("tests/fixtures/{name}")).expect("fixture not found")
    }

    /// A minimal valid triangle — positions, normals, UVs, 3 indices.
    pub fn sample_triangle() -> MeshData {
        MeshData {
            topology: crate::TopologyKind::TriangleList,
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            normals: Some(vec![[0.0, 0.0, 1.0]; 3]),
            uvs: Some(vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]),
            vertex_colors: None,
            indices: vec![0, 1, 2],
            material_index: None,
        }
    }

    /// A minimal triangle with positions and indices only — no normals, no UVs.
    pub fn sample_triangle_bare() -> MeshData {
        MeshData::new(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 2],
        )
    }
}
