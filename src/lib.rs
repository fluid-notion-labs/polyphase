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

// Future: #[cfg(feature = "bevy")] pub mod bevy;

// ── Public API re-exports ─────────────────────────────────────────────

pub use capabilities::{AssetCapabilities, AssetData, MaterialData, ParseCost, TopologyKind};
pub use error::LoadError;
pub use format::MeshFormat;
pub use mesh_data::MeshData;
