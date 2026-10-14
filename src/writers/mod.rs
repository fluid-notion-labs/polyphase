//! Format writers — the inverse of the parsers.
//!
//! Same purity rules, mirrored direction: pure functions from
//! [`AssetData`](crate::AssetData) to bytes. No I/O, no filesystem, no Bevy.
//! The caller writes the returned bytes wherever it wants.
//!
//! ```ignore
//! let asset = polyphase::parse_asset(&bytes, MeshFormat::Glb)?;
//! let out = polyphase::writers::glb::write_glb(&asset)?;
//! std::fs::write("out.glb", out)?;
//! ```

pub mod glb;

use crate::mesh_data::MeshDataError;
use thiserror::Error;

/// Errors produced by the format writers.
#[derive(Debug, Error)]
pub enum WriteError {
    /// The asset failed structural validation before writing.
    #[error("validation: {0}")]
    Validation(#[from] MeshDataError),

    /// The asset contains something the target format cannot express.
    #[error("cannot write {format}: {reason}")]
    Unsupported {
        /// Target format (lowercase, e.g. `"glb"`).
        format: &'static str,
        /// Why the asset can't be expressed in it.
        reason: String,
    },

    /// A glTF-internal serialization failure (JSON generation, GLB container).
    #[error("glb serialization: {0}")]
    GlbSerialization(String),
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_error_displays() {
        let e = WriteError::Unsupported {
            format: "stl",
            reason: "multiple meshes".to_string(),
        };
        assert_eq!(e.to_string(), "cannot write stl: multiple meshes");

        let e = WriteError::GlbSerialization("bad json".to_string());
        assert_eq!(e.to_string(), "glb serialization: bad json");
    }
}
