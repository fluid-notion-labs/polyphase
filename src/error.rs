//! Error types for loadfu parsers.
//!
//! Each format has its own error type wrapping the underlying parsing library's
//! errors. [`LoadError`] is the top-level enum that unifies them all.

use crate::mesh_data::MeshDataError;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Per-format errors
// ---------------------------------------------------------------------------

/// Errors from STL parsing.
#[derive(Debug, Error)]
pub enum StlError {
    /// The underlying `stl_io` crate returned an I/O error.
    #[error("STL I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The STL file parsed successfully but contained no triangles.
    #[error("STL file contains no triangles")]
    EmptyMesh,
}

/// Errors from OBJ parsing.
#[derive(Debug, Error)]
pub enum ObjError {
    /// The underlying `tobj` crate returned a parse error.
    #[error("OBJ parse error: {0}")]
    Parse(#[from] tobj::LoadError),

    /// I/O error reading OBJ data.
    #[error("OBJ I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The OBJ file contained no models/objects.
    #[error("OBJ file contains no meshes")]
    EmptyFile,

    /// The OBJ file had models but zero vertices across all of them.
    #[error("OBJ file contains no geometry (0 vertices)")]
    EmptyMesh,
}

/// Errors from GLB/glTF parsing.
#[derive(Debug, Error)]
pub enum GlbError {
    /// The underlying `gltf` crate returned an error.
    #[error("glTF error: {0}")]
    Gltf(#[from] gltf::Error),

    /// I/O error reading GLB data.
    #[error("GLB I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The file contained no meshes.
    #[error("GLB file contains no meshes")]
    EmptyFile,

    /// A mesh primitive was missing required position data.
    #[error("GLB mesh primitive missing POSITION attribute")]
    MissingPositions,

    /// A mesh primitive used an unsupported topology (not triangles).
    #[error("GLB mesh primitive has unsupported topology: {0}")]
    UnsupportedTopology(String),
}

// ---------------------------------------------------------------------------
// Top-level error
// ---------------------------------------------------------------------------

/// Unified error type for all loadfu operations.
///
/// Wraps the per-format error variants so callers can match on the format
/// or just treat it as a generic load failure.
#[derive(Debug, Error)]
pub enum LoadError {
    /// Error loading an STL file.
    #[error("STL: {0}")]
    Stl(#[from] StlError),

    /// Error loading an OBJ file.
    #[error("OBJ: {0}")]
    Obj(#[from] ObjError),

    /// Error loading a GLB/glTF file.
    #[error("GLB: {0}")]
    Glb(#[from] GlbError),

    /// The file extension was not recognised or the format is unsupported.
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),

    /// Mesh data failed validation after parsing.
    #[error("validation: {0}")]
    Validation(#[from] MeshDataError),

    /// Generic I/O error not tied to a specific format.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

// ---------------------------------------------------------------------------
// Convenience conversions
// ---------------------------------------------------------------------------

impl LoadError {
    /// Returns the format name associated with this error, if known.
    pub fn format_name(&self) -> Option<&'static str> {
        match self {
            Self::Stl(_) => Some("STL"),
            Self::Obj(_) => Some("OBJ"),
            Self::Glb(_) => Some("GLB"),
            Self::Validation(_) | Self::UnsupportedFormat(_) | Self::Io(_) => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stl_error_display() {
        let err = StlError::EmptyMesh;
        assert_eq!(err.to_string(), "STL file contains no triangles");
    }

    #[test]
    fn obj_error_display() {
        let err = ObjError::EmptyFile;
        assert_eq!(err.to_string(), "OBJ file contains no meshes");
    }

    #[test]
    fn glb_error_display() {
        let err = GlbError::EmptyFile;
        assert_eq!(err.to_string(), "GLB file contains no meshes");

        let err = GlbError::MissingPositions;
        assert_eq!(
            err.to_string(),
            "GLB mesh primitive missing POSITION attribute"
        );
    }

    #[test]
    fn load_error_from_stl() {
        let stl_err = StlError::EmptyMesh;
        let load_err = LoadError::from(stl_err);
        assert!(matches!(load_err, LoadError::Stl(StlError::EmptyMesh)));
        assert_eq!(load_err.format_name(), Some("STL"));
    }

    #[test]
    fn load_error_from_obj() {
        let obj_err = ObjError::EmptyMesh;
        let load_err = LoadError::from(obj_err);
        assert!(matches!(load_err, LoadError::Obj(ObjError::EmptyMesh)));
        assert_eq!(load_err.format_name(), Some("OBJ"));
    }

    #[test]
    fn load_error_from_glb() {
        let glb_err = GlbError::EmptyFile;
        let load_err = LoadError::from(glb_err);
        assert!(matches!(load_err, LoadError::Glb(GlbError::EmptyFile)));
        assert_eq!(load_err.format_name(), Some("GLB"));
    }

    #[test]
    fn load_error_unsupported_format() {
        let err = LoadError::UnsupportedFormat("fbx".into());
        assert_eq!(err.to_string(), "unsupported format: fbx");
        assert_eq!(err.format_name(), None);
    }

    #[test]
    fn load_error_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = LoadError::Io(io_err);
        assert_eq!(err.format_name(), None);
        assert!(err.to_string().contains("file not found"));
    }

    #[test]
    fn load_error_from_validation() {
        let v_err = MeshDataError::EmptyPositions;
        let err = LoadError::from(v_err);
        assert!(matches!(err, LoadError::Validation(_)));
        assert_eq!(err.format_name(), None);
        assert!(err.to_string().contains("no positions"));
    }
}
