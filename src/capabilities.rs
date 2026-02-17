//! Asset capabilities and metadata types.
//!
//! Provides two tiers of capability information:
//!
//! - **Pre-parse (static):** knowable from the format alone before reading any data.
//!   E.g. "STL never has UVs", "GLB may have materials".
//! - **Post-parse:** determined from the actual loaded data.
//!   E.g. "this GLB has 3 meshes and 2 PBR materials".
//!
//! See also [`MeshFormat::static_capabilities`](crate::MeshFormat::static_capabilities).

use crate::MeshData;

// ---------------------------------------------------------------------------
// TopologyKind
// ---------------------------------------------------------------------------

/// The primitive topology of the mesh data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TopologyKind {
    /// Triangle list — three indices per face. The universal default.
    TriangleList,
    /// Triangle strip — shared edges between consecutive triangles.
    TriangleStrip,
    /// Line list — two indices per segment.
    LineList,
    /// Point cloud — one index per point.
    PointList,
}

impl Default for TopologyKind {
    fn default() -> Self {
        Self::TriangleList
    }
}

// ---------------------------------------------------------------------------
// ParseCost
// ---------------------------------------------------------------------------

/// Rough heuristic for how expensive parsing a format is.
///
/// Used by scheduling/budgeting code to decide how many assets to load
/// concurrently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ParseCost {
    /// Near-instant: memory-mapped or pre-indexed binary.
    Trivial,
    /// Fast binary parse (e.g. binary STL, GLB).
    Low,
    /// Moderate parse (e.g. OBJ with indexing, ASCII STL).
    Medium,
    /// Expensive parse (e.g. Draco decompression, large glTF with textures).
    High,
}

impl Default for ParseCost {
    fn default() -> Self {
        Self::Medium
    }
}

// ---------------------------------------------------------------------------
// MaterialData
// ---------------------------------------------------------------------------

/// Minimal material description extracted from an asset.
///
/// Intentionally simple for v0.1 — captures PBR base properties that GLB
/// and OBJ/MTL can provide. Extensible for future formats.
#[derive(Debug, Clone)]
pub struct MaterialData {
    /// Material name (from the file, if any).
    pub name: Option<String>,
    /// Base color as linear RGBA.
    pub base_color: [f32; 4],
    /// Metallic factor (0.0 = dielectric, 1.0 = metallic).
    pub metallic: f32,
    /// Roughness factor (0.0 = smooth, 1.0 = rough).
    pub roughness: f32,
    /// Whether this material references texture data.
    pub has_textures: bool,
}

impl Default for MaterialData {
    fn default() -> Self {
        Self {
            name: None,
            base_color: [1.0, 1.0, 1.0, 1.0],
            metallic: 0.0,
            roughness: 0.5,
            has_textures: false,
        }
    }
}

// ---------------------------------------------------------------------------
// AssetCapabilities
// ---------------------------------------------------------------------------

/// What loadfu knows (or can infer) about an asset.
///
/// Two tiers:
/// - **Static:** from [`MeshFormat::static_capabilities`](crate::MeshFormat::static_capabilities)
///   — conservative defaults based on format alone.
/// - **Post-parse:** from [`AssetData::capabilities`] — precise values from
///   the actual loaded data.
#[derive(Debug, Clone)]
pub struct AssetCapabilities {
    // ── Geometry ──────────────────────────────────────────────────────
    /// Primitive topology of the mesh.
    pub topology: TopologyKind,
    /// Whether normals are present (or can be computed).
    pub has_normals: bool,
    /// Whether texture coordinates are present.
    pub has_uvs: bool,
    /// Whether vertex colors are present.
    pub has_vertex_colors: bool,
    /// Whether the asset contains multiple distinct meshes.
    pub multi_object: bool,

    // ── Materials ─────────────────────────────────────────────────────
    /// Whether any material data is present.
    pub has_materials: bool,
    /// Whether texture references (image data) are present.
    pub has_textures: bool,

    // ── Cost heuristics ───────────────────────────────────────────────
    /// Rough parse cost classification.
    pub parse_cost: ParseCost,
    /// Estimated memory footprint in bytes (from header or heuristic,
    /// before full parse). `None` if unknown.
    pub estimated_memory: Option<u64>,

    // ── Format-specific ───────────────────────────────────────────────
    /// Whether the format supports memory-mapped zero-copy access.
    pub supports_mmap: bool,
    /// Whether the format supports streaming / progressive loading.
    pub supports_streaming: bool,
}

impl Default for AssetCapabilities {
    fn default() -> Self {
        Self {
            topology: TopologyKind::TriangleList,
            has_normals: false,
            has_uvs: false,
            has_vertex_colors: false,
            multi_object: false,
            has_materials: false,
            has_textures: false,
            parse_cost: ParseCost::Medium,
            estimated_memory: None,
            supports_mmap: false,
            supports_streaming: false,
        }
    }
}

// ---------------------------------------------------------------------------
// AssetData
// ---------------------------------------------------------------------------

/// Full output of parsing an asset — geometry and (optionally) materials.
///
/// Every parser ultimately produces an `AssetData`. Simple formats (STL)
/// yield a single mesh and no materials; complex formats (GLB) may yield
/// many meshes and full PBR materials.
#[derive(Debug, Clone)]
pub struct AssetData {
    /// One or more meshes extracted from the asset.
    pub meshes: Vec<MeshData>,
    /// Material descriptions, if the format provides them.
    pub materials: Vec<MaterialData>,
}

impl AssetData {
    /// Create an `AssetData` from a single mesh with no materials.
    pub fn single(mesh: MeshData) -> Self {
        Self {
            meshes: vec![mesh],
            materials: Vec::new(),
        }
    }

    /// Total vertex count across all meshes.
    pub fn total_vertices(&self) -> usize {
        self.meshes.iter().map(|m| m.positions.len()).sum()
    }

    /// Total triangle count across all meshes.
    ///
    /// Only counts meshes with triangle topology (list or strip).
    /// Line and point meshes contribute zero.
    pub fn total_triangles(&self) -> usize {
        self.meshes.iter().map(|m| m.triangle_count()).sum()
    }

    /// Total primitive count across all meshes (topology-aware).
    ///
    /// Triangles, line segments, and points are all counted according to
    /// their respective topology.
    pub fn total_primitives(&self) -> usize {
        self.meshes.iter().map(|m| m.primitive_count()).sum()
    }

    /// Post-parse capabilities derived from the actual loaded data.
    ///
    /// This is the second tier — more precise than
    /// [`MeshFormat::static_capabilities`](crate::MeshFormat::static_capabilities)
    /// because it inspects the real geometry and materials.
    pub fn capabilities(&self) -> AssetCapabilities {
        let has_normals = self.meshes.iter().any(|m| m.normals.is_some());
        let has_uvs = self.meshes.iter().any(|m| m.uvs.is_some());
        let has_vertex_colors = self.meshes.iter().any(|m| m.vertex_colors.is_some());
        let has_materials = !self.materials.is_empty();
        let has_textures = self.materials.iter().any(|m| m.has_textures);
        let multi_object = self.meshes.len() > 1;

        // Rough memory estimate: positions + normals + uvs + indices
        let mem: u64 = self
            .meshes
            .iter()
            .map(|m| {
                let pos_bytes = m.positions.len() as u64 * 12; // 3 × f32
                let norm_bytes = m.normals.as_ref().map_or(0, |n| n.len() as u64 * 12);
                let uv_bytes = m.uvs.as_ref().map_or(0, |u| u.len() as u64 * 8); // 2 × f32
                let color_bytes = m.vertex_colors.as_ref().map_or(0, |c| c.len() as u64 * 16); // 4 × f32
                let idx_bytes = m.indices.len() as u64 * 4; // u32
                pos_bytes + norm_bytes + uv_bytes + color_bytes + idx_bytes
            })
            .sum();

        // Determine the primary topology — use the first mesh's topology,
        // or default to TriangleList if there are no meshes.
        let topology = self.meshes.first().map(|m| m.topology).unwrap_or_default();

        AssetCapabilities {
            topology,
            has_normals,
            has_uvs,
            has_vertex_colors,
            multi_object,
            has_materials,
            has_textures,
            parse_cost: ParseCost::Low, // already parsed — cost is sunk
            estimated_memory: Some(mem),
            supports_mmap: false,
            supports_streaming: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{sample_triangle, sample_triangle_bare};

    // ── TopologyKind ──────────────────────────────────────────────────

    #[test]
    fn topology_default_is_triangle_list() {
        assert_eq!(TopologyKind::default(), TopologyKind::TriangleList);
    }

    // ── ParseCost ordering ────────────────────────────────────────────

    #[test]
    fn parse_cost_ordering() {
        assert!(ParseCost::Trivial < ParseCost::Low);
        assert!(ParseCost::Low < ParseCost::Medium);
        assert!(ParseCost::Medium < ParseCost::High);
    }

    // ── MaterialData defaults ─────────────────────────────────────────

    #[test]
    fn material_data_defaults() {
        let mat = MaterialData::default();
        assert!(mat.name.is_none());
        assert_eq!(mat.base_color, [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(mat.metallic, 0.0);
        assert!(!mat.has_textures);
    }

    // ── AssetCapabilities defaults ────────────────────────────────────

    #[test]
    fn capabilities_default() {
        let cap = AssetCapabilities::default();
        assert_eq!(cap.topology, TopologyKind::TriangleList);
        assert!(!cap.has_normals);
        assert!(!cap.has_uvs);
        assert!(!cap.has_vertex_colors);
        assert!(!cap.multi_object);
        assert!(!cap.has_materials);
        assert!(!cap.has_textures);
        assert_eq!(cap.parse_cost, ParseCost::Medium);
        assert!(cap.estimated_memory.is_none());
        assert!(!cap.supports_mmap);
        assert!(!cap.supports_streaming);
    }

    // ── AssetData::single ─────────────────────────────────────────────

    #[test]
    fn asset_data_single() {
        let data = AssetData::single(sample_triangle());
        assert_eq!(data.meshes.len(), 1);
        assert!(data.materials.is_empty());
    }

    // ── AssetData::total_vertices / total_triangles ───────────────────

    #[test]
    fn asset_data_totals() {
        let data = AssetData {
            meshes: vec![sample_triangle(), sample_triangle()],
            materials: Vec::new(),
        };
        assert_eq!(data.total_vertices(), 6);
        assert_eq!(data.total_triangles(), 2);
    }

    // ── AssetData::capabilities (post-parse) ──────────────────────────

    #[test]
    fn capabilities_from_data_with_normals_and_uvs() {
        let data = AssetData::single(sample_triangle());
        let cap = data.capabilities();
        assert!(cap.has_normals);
        assert!(cap.has_uvs);
        assert!(!cap.multi_object);
        assert!(!cap.has_materials);
        assert!(cap.estimated_memory.is_some());
        // 3 verts × 12 (pos) + 3 × 12 (norm) + 3 × 8 (uv) + 3 × 4 (idx) = 108
        assert_eq!(cap.estimated_memory, Some(108));
    }

    #[test]
    fn capabilities_from_data_without_normals() {
        let data = AssetData::single(sample_triangle_bare());
        let cap = data.capabilities();
        assert!(!cap.has_normals);
        assert!(!cap.has_uvs);
        // 3 verts × 12 (pos) + 3 × 4 (idx) = 48
        assert_eq!(cap.estimated_memory, Some(48));
    }

    #[test]
    fn capabilities_multi_object() {
        let data = AssetData {
            meshes: vec![sample_triangle(), sample_triangle_bare()],
            materials: Vec::new(),
        };
        let cap = data.capabilities();
        assert!(cap.multi_object);
        // One mesh has normals → has_normals is true (any)
        assert!(cap.has_normals);
    }

    #[test]
    fn capabilities_with_materials() {
        let data = AssetData {
            meshes: vec![sample_triangle()],
            materials: vec![
                MaterialData::default(),
                MaterialData {
                    has_textures: true,
                    ..Default::default()
                },
            ],
        };
        let cap = data.capabilities();
        assert!(cap.has_materials);
        assert!(cap.has_textures);
    }

    #[test]
    fn capabilities_materials_without_textures() {
        let data = AssetData {
            meshes: vec![sample_triangle()],
            materials: vec![MaterialData::default()],
        };
        let cap = data.capabilities();
        assert!(cap.has_materials);
        assert!(!cap.has_textures);
    }

    // ── Empty AssetData ───────────────────────────────────────────────

    #[test]
    fn empty_asset_data() {
        let data = AssetData {
            meshes: Vec::new(),
            materials: Vec::new(),
        };
        assert_eq!(data.total_vertices(), 0);
        assert_eq!(data.total_triangles(), 0);
        let cap = data.capabilities();
        assert!(!cap.has_normals);
        assert!(!cap.multi_object);
        assert_eq!(cap.estimated_memory, Some(0));
    }
}
