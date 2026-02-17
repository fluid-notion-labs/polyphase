//! **loadfu** — format-agnostic 3D asset loading.
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
//! Default features: *none*. `cargo check -p loadfu` compiles with zero Bevy
//! dependency.
//!
//! # Quick start (core only)
//!
//! ```rust
//! use loadfu::{MeshData, MeshFormat};
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

// ── Public API re-exports ─────────────────────────────────────────────

pub use capabilities::{AssetCapabilities, AssetData, MaterialData, ParseCost, TopologyKind};
pub use error::{GlbError, LoadError, ObjError, PlyError, StlError};
pub use format::MeshFormat;
pub use mesh_data::{MeshData, MeshDataError};

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
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            normals: Some(vec![[0.0, 0.0, 1.0]; 3]),
            uvs: Some(vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]),
            vertex_colors: None,
            indices: vec![0, 1, 2],
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
