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

/// Errors from PLY parsing.
#[derive(Debug, Error)]
pub enum PlyError {
    /// The PLY header is missing, malformed, or contains unsupported declarations.
    #[error("PLY invalid header: {0}")]
    InvalidHeader(String),

    /// The PLY file uses an unsupported property type or encoding.
    #[error("PLY unsupported format: {0}")]
    UnsupportedFormat(String),

    /// The PLY file parsed successfully but contained no vertices or faces.
    #[error("PLY file contains no geometry")]
    EmptyMesh,

    /// I/O error reading PLY data.
    #[error("PLY I/O error: {0}")]
    Io(#[from] std::io::Error),
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

    /// The glTF JSON references external files (buffers or images with URIs
    /// that are not data URIs). Core parsers are pure — they cannot touch
    /// the filesystem.
    #[error(
        "glTF references external files ({what}); use GLB, or the Bevy loader which can resolve sibling files"
    )]
    ExternalReferences {
        /// Human-readable description of what was external (e.g. the URI).
        what: String,
    },

    /// A glTF image used a MIME type this parser cannot tag (only PNG and
    /// JPEG are supported).
    #[error("glTF image uses unsupported MIME type: {0}")]
    UnsupportedImageMime(String),

    /// A data URI was malformed (missing payload or undecodable base64).
    #[error("glTF malformed data URI: {0}")]
    DataUri(String),

    /// A buffer view referenced data outside its buffer.
    #[error("glTF buffer view is out of bounds")]
    InvalidBufferView,
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

    /// Error loading a PLY file.
    #[error("PLY: {0}")]
    Ply(#[from] PlyError),

    /// The format could not be recognised, or was recognised but is not
    /// supported by this build (e.g. VTK without the `vtk` feature).
    #[error("unsupported format{}", .detected.map(|f| format!(": {f}")).unwrap_or_default())]
    UnsupportedFormat {
        /// The format that was detected, if any.
        detected: Option<crate::MeshFormat>,
    },

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
            Self::Ply(_) => Some("PLY"),
            Self::Validation(_) | Self::UnsupportedFormat { .. } | Self::Io(_) => None,
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
    fn ply_error_display() {
        let err = PlyError::EmptyMesh;
        assert_eq!(err.to_string(), "PLY file contains no geometry");

        let err = PlyError::InvalidHeader("missing end_header".into());
        assert!(err.to_string().contains("missing end_header"));

        let err = PlyError::UnsupportedFormat("double precision".into());
        assert!(err.to_string().contains("double precision"));
    }

    #[test]
    fn load_error_from_ply() {
        let ply_err = PlyError::EmptyMesh;
        let load_err = LoadError::from(ply_err);
        assert!(matches!(load_err, LoadError::Ply(PlyError::EmptyMesh)));
        assert_eq!(load_err.format_name(), Some("PLY"));
    }

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
        let err = LoadError::UnsupportedFormat { detected: None };
        assert_eq!(err.to_string(), "unsupported format");
        assert_eq!(err.format_name(), None);

        let err = LoadError::UnsupportedFormat {
            detected: Some(crate::MeshFormat::Glb),
        };
        assert_eq!(err.to_string(), "unsupported format: GLB");
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
