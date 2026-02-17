//! Mesh format detection and classification.
//!
//! [`MeshFormat`] identifies recognised 3D asset formats and provides
//! helpers for extension-based detection, format classification,
//! human-readable labels, and pre-parse capability inference.
//!
//! Extracted from seaview's `MeshFileFormat` in `sequence/loader/assets.rs`.

use crate::capabilities::{AssetCapabilities, ParseCost, TopologyKind};

/// Recognised 3D asset formats that loadfu can parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MeshFormat {
    /// STL (binary or ASCII) — triangle soup, no UVs, no materials.
    Stl,
    /// OBJ (Wavefront) — indexed mesh with optional normals, UVs, and MTL materials.
    Obj,
    /// glTF 2.0 text format — full scene with PBR materials, multi-mesh support.
    Gltf,
    /// glTF 2.0 binary container — same capabilities as glTF, single-file packaging.
    Glb,
    // Future: DracoGlb, SvMesh, Ply
}

impl MeshFormat {
    /// Try to detect the format from a file extension string (case-insensitive).
    ///
    /// Returns `None` for unrecognised extensions.
    ///
    /// # Examples
    ///
    /// ```
    /// use loadfu::MeshFormat;
    ///
    /// assert_eq!(MeshFormat::from_extension("stl"), Some(MeshFormat::Stl));
    /// assert_eq!(MeshFormat::from_extension("GLB"), Some(MeshFormat::Glb));
    /// assert_eq!(MeshFormat::from_extension("fbx"), None);
    /// ```
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "stl" => Some(Self::Stl),
            "obj" => Some(Self::Obj),
            "gltf" => Some(Self::Gltf),
            "glb" => Some(Self::Glb),
            _ => None,
        }
    }

    /// Detect the format from a file path by inspecting its extension.
    ///
    /// Returns `None` if the path has no extension or the extension is unrecognised.
    ///
    /// # Examples
    ///
    /// ```
    /// use loadfu::MeshFormat;
    /// use std::path::Path;
    ///
    /// assert_eq!(MeshFormat::from_path(Path::new("model.stl")), Some(MeshFormat::Stl));
    /// assert_eq!(MeshFormat::from_path(Path::new("/data/scene.glb")), Some(MeshFormat::Glb));
    /// assert_eq!(MeshFormat::from_path(Path::new("no_ext")), None);
    /// ```
    pub fn from_path(path: &std::path::Path) -> Option<Self> {
        path.extension()
            .and_then(|ext| ext.to_str())
            .and_then(Self::from_extension)
    }

    /// Returns `true` if this format is a glTF variant (glTF text or GLB binary).
    ///
    /// glTF formats support full scenes: multiple meshes, PBR materials,
    /// textures, animations, and node hierarchies.
    pub fn is_gltf(&self) -> bool {
        matches!(self, Self::Gltf | Self::Glb)
    }

    /// Returns `true` if this format loads directly as a single mesh
    /// (not via a scene/glTF intermediate).
    ///
    /// Direct mesh formats (STL, OBJ) produce geometry without scene structure.
    /// glTF formats go through scene parsing and may contain multiple meshes.
    pub fn is_direct_mesh(&self) -> bool {
        matches!(self, Self::Stl | Self::Obj)
    }

    /// Human-readable label for display in UI and log messages.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Stl => "STL",
            Self::Obj => "OBJ",
            Self::Gltf => "glTF",
            Self::Glb => "GLB",
        }
    }

    /// File extension(s) associated with this format.
    ///
    /// Returns the canonical lowercase extension(s) used for asset loading.
    pub fn extensions(&self) -> &'static [&'static str] {
        match self {
            Self::Stl => &["stl"],
            Self::Obj => &["obj"],
            Self::Gltf => &["gltf"],
            Self::Glb => &["glb"],
        }
    }

    /// Returns an iterator over all known formats.
    pub fn all() -> impl Iterator<Item = MeshFormat> {
        [Self::Stl, Self::Obj, Self::Gltf, Self::Glb].into_iter()
    }

    /// Pre-parse capabilities knowable from the format alone.
    ///
    /// These are conservative defaults — "STL never has UVs", "GLB may have
    /// materials", etc. Post-parse capabilities from [`AssetData::capabilities`]
    /// are always more precise.
    ///
    /// [`AssetData::capabilities`]: crate::AssetData::capabilities
    pub fn static_capabilities(&self) -> AssetCapabilities {
        match self {
            Self::Stl => AssetCapabilities {
                topology: TopologyKind::TriangleList,
                has_normals: true, // STL always has face normals
                has_uvs: false,    // STL never has UVs
                has_vertex_colors: false,
                multi_object: false, // STL is always a single mesh
                has_materials: false,
                has_textures: false,
                parse_cost: ParseCost::Low, // binary STL is fast
                estimated_memory: None,
                supports_mmap: false,
                supports_streaming: false,
            },
            Self::Obj => AssetCapabilities {
                topology: TopologyKind::TriangleList,
                has_normals: true, // OBJ usually has normals (not guaranteed)
                has_uvs: true,     // OBJ often has UVs
                has_vertex_colors: false,
                multi_object: true,  // OBJ can have multiple objects/groups
                has_materials: true, // OBJ can reference MTL files
                has_textures: true,  // MTL can reference texture images
                parse_cost: ParseCost::Medium, // text parsing + indexing
                estimated_memory: None,
                supports_mmap: false,
                supports_streaming: false,
            },
            Self::Gltf => AssetCapabilities {
                topology: TopologyKind::TriangleList,
                has_normals: true,
                has_uvs: true,
                has_vertex_colors: true, // glTF supports vertex colors
                multi_object: true,
                has_materials: true, // full PBR materials
                has_textures: true,
                parse_cost: ParseCost::Medium, // JSON parse + binary buffer reads
                estimated_memory: None,
                supports_mmap: false,
                supports_streaming: false,
            },
            Self::Glb => AssetCapabilities {
                topology: TopologyKind::TriangleList,
                has_normals: true,
                has_uvs: true,
                has_vertex_colors: true,
                multi_object: true,
                has_materials: true,
                has_textures: true,
                parse_cost: ParseCost::Low, // binary container, faster than text glTF
                estimated_memory: None,
                supports_mmap: false,
                supports_streaming: false,
            },
        }
    }
}

impl std::fmt::Display for MeshFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::{ParseCost, TopologyKind};
    use std::path::Path;

    // ── from_extension ────────────────────────────────────────────────

    #[test]
    fn from_extension_lowercase() {
        assert_eq!(MeshFormat::from_extension("stl"), Some(MeshFormat::Stl));
        assert_eq!(MeshFormat::from_extension("obj"), Some(MeshFormat::Obj));
        assert_eq!(MeshFormat::from_extension("gltf"), Some(MeshFormat::Gltf));
        assert_eq!(MeshFormat::from_extension("glb"), Some(MeshFormat::Glb));
    }

    #[test]
    fn from_extension_uppercase() {
        assert_eq!(MeshFormat::from_extension("STL"), Some(MeshFormat::Stl));
        assert_eq!(MeshFormat::from_extension("OBJ"), Some(MeshFormat::Obj));
        assert_eq!(MeshFormat::from_extension("GLTF"), Some(MeshFormat::Gltf));
        assert_eq!(MeshFormat::from_extension("GLB"), Some(MeshFormat::Glb));
    }

    #[test]
    fn from_extension_mixed_case() {
        assert_eq!(MeshFormat::from_extension("Stl"), Some(MeshFormat::Stl));
        assert_eq!(MeshFormat::from_extension("gLtF"), Some(MeshFormat::Gltf));
    }

    #[test]
    fn from_extension_unknown() {
        assert_eq!(MeshFormat::from_extension("fbx"), None);
        assert_eq!(MeshFormat::from_extension("ply"), None);
        assert_eq!(MeshFormat::from_extension(""), None);
        assert_eq!(MeshFormat::from_extension("txt"), None);
    }

    // ── from_path ─────────────────────────────────────────────────────

    #[test]
    fn from_path_with_extension() {
        assert_eq!(
            MeshFormat::from_path(Path::new("model.stl")),
            Some(MeshFormat::Stl)
        );
        assert_eq!(
            MeshFormat::from_path(Path::new("/data/frames/frame_001.glb")),
            Some(MeshFormat::Glb)
        );
        assert_eq!(
            MeshFormat::from_path(Path::new("scene.gltf")),
            Some(MeshFormat::Gltf)
        );
    }

    #[test]
    fn from_path_no_extension() {
        assert_eq!(MeshFormat::from_path(Path::new("no_ext")), None);
        assert_eq!(MeshFormat::from_path(Path::new("/data/frames/")), None);
    }

    #[test]
    fn from_path_unknown_extension() {
        assert_eq!(MeshFormat::from_path(Path::new("model.fbx")), None);
    }

    // ── Classification helpers ────────────────────────────────────────

    #[test]
    fn is_gltf() {
        assert!(!MeshFormat::Stl.is_gltf());
        assert!(!MeshFormat::Obj.is_gltf());
        assert!(MeshFormat::Gltf.is_gltf());
        assert!(MeshFormat::Glb.is_gltf());
    }

    #[test]
    fn is_direct_mesh() {
        assert!(MeshFormat::Stl.is_direct_mesh());
        assert!(MeshFormat::Obj.is_direct_mesh());
        assert!(!MeshFormat::Gltf.is_direct_mesh());
        assert!(!MeshFormat::Glb.is_direct_mesh());
    }

    #[test]
    fn is_gltf_and_is_direct_mesh_are_complementary() {
        for format in MeshFormat::all() {
            assert_ne!(
                format.is_gltf(),
                format.is_direct_mesh(),
                "{format:?} should be exactly one of gltf or direct_mesh"
            );
        }
    }

    // ── Labels and display ────────────────────────────────────────────

    #[test]
    fn labels() {
        assert_eq!(MeshFormat::Stl.label(), "STL");
        assert_eq!(MeshFormat::Obj.label(), "OBJ");
        assert_eq!(MeshFormat::Gltf.label(), "glTF");
        assert_eq!(MeshFormat::Glb.label(), "GLB");
    }

    #[test]
    fn display_matches_label() {
        for format in MeshFormat::all() {
            assert_eq!(format.to_string(), format.label());
        }
    }

    // ── Extensions ────────────────────────────────────────────────────

    #[test]
    fn extensions_roundtrip() {
        for format in MeshFormat::all() {
            for ext in format.extensions() {
                assert_eq!(
                    MeshFormat::from_extension(ext),
                    Some(format),
                    "extension '{ext}' should round-trip to {format:?}"
                );
            }
        }
    }

    // ── All ───────────────────────────────────────────────────────────

    #[test]
    fn all_returns_every_variant() {
        let all: Vec<MeshFormat> = MeshFormat::all().collect();
        assert_eq!(all.len(), 4);
        assert!(all.contains(&MeshFormat::Stl));
        assert!(all.contains(&MeshFormat::Obj));
        assert!(all.contains(&MeshFormat::Gltf));
        assert!(all.contains(&MeshFormat::Glb));
    }

    // ── Static capabilities ───────────────────────────────────────────

    #[test]
    fn stl_static_capabilities() {
        let cap = MeshFormat::Stl.static_capabilities();
        assert_eq!(cap.topology, TopologyKind::TriangleList);
        assert!(cap.has_normals);
        assert!(!cap.has_uvs);
        assert!(!cap.has_vertex_colors);
        assert!(!cap.multi_object);
        assert!(!cap.has_materials);
        assert!(!cap.has_textures);
        assert_eq!(cap.parse_cost, ParseCost::Low);
    }

    #[test]
    fn obj_static_capabilities() {
        let cap = MeshFormat::Obj.static_capabilities();
        assert!(cap.has_normals);
        assert!(cap.has_uvs);
        assert!(cap.multi_object);
        assert!(cap.has_materials);
        assert_eq!(cap.parse_cost, ParseCost::Medium);
    }

    #[test]
    fn gltf_static_capabilities() {
        let cap = MeshFormat::Gltf.static_capabilities();
        assert!(cap.has_normals);
        assert!(cap.has_uvs);
        assert!(cap.has_vertex_colors);
        assert!(cap.multi_object);
        assert!(cap.has_materials);
        assert!(cap.has_textures);
        assert_eq!(cap.parse_cost, ParseCost::Medium);
    }

    #[test]
    fn glb_static_capabilities() {
        let cap = MeshFormat::Glb.static_capabilities();
        assert!(cap.has_normals);
        assert!(cap.has_uvs);
        assert!(cap.has_vertex_colors);
        assert!(cap.multi_object);
        assert!(cap.has_materials);
        assert!(cap.has_textures);
        assert_eq!(cap.parse_cost, ParseCost::Low);
    }

    #[test]
    fn all_formats_have_triangle_list_topology() {
        for format in MeshFormat::all() {
            assert_eq!(
                format.static_capabilities().topology,
                TopologyKind::TriangleList,
                "{format:?} should default to TriangleList"
            );
        }
    }

    #[test]
    fn all_formats_have_no_estimated_memory_statically() {
        for format in MeshFormat::all() {
            assert!(
                format.static_capabilities().estimated_memory.is_none(),
                "{format:?} should not know memory before parsing"
            );
        }
    }
}
