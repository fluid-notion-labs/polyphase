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
// AlphaMode
// ---------------------------------------------------------------------------

/// How transparency is handled by a material.
///
/// Maps directly to glTF's `alphaMode` and (later) Bevy's alpha handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlphaMode {
    /// Fully opaque — the default.
    Opaque,
    /// Hard cutoff: fragments with alpha below `MaterialData::alpha_cutoff`
    /// are discarded.
    Mask,
    /// Ordered blending — true partial transparency.
    Blend,
}

impl Default for AlphaMode {
    fn default() -> Self {
        Self::Opaque
    }
}

// ---------------------------------------------------------------------------
// Textures
// ---------------------------------------------------------------------------

/// MIME type of encoded texture data.
///
/// Core never decodes image data (no image-codec dependency) — this only
/// tags what the bytes contain so the consumer can pick a decoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureMime {
    /// PNG (`image/png`).
    Png,
    /// JPEG (`image/jpeg`).
    Jpeg,
}

/// Where a texture's data lives.
///
/// Core stays pure: it never opens a [`TextureSource::Uri`]. The Bevy layer
/// resolves URIs via its load context; core-only consumers that want URIs
/// resolved do their own IO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextureSource {
    /// Encoded image bytes carried inline (glTF/GLB buffer views, data URIs).
    Embedded {
        /// Raw, still-encoded image bytes (not decoded pixels).
        bytes: Vec<u8>,
        /// What codec the bytes belong to.
        mime: TextureMime,
    },
    /// File reference — resolved by the caller (MTL `map_Kd`, glTF URIs).
    Uri(String),
}

impl TextureSource {
    /// The MIME type of the source, if known.
    pub fn mime(&self) -> Option<TextureMime> {
        match self {
            Self::Embedded { mime, .. } => Some(*mime),
            Self::Uri(uri) => TextureMime::from_uri(uri),
        }
    }
}

/// A single texture reference extracted from an asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureData {
    /// Where the encoded bytes (or the file reference) live.
    pub source: TextureSource,
    /// Texture name (from the file, if any).
    pub name: Option<String>,
}

/// Texture sampling state, in raw OpenGL enum values (as stored in glTF).
///
/// glTF sampler fields are optional: `magFilter`/`minFilter` default to
/// "auto" filtering (`None` here — GL LINEAR / LINEAR_MIPMAP_LINEAR), and
/// `wrapS`/`wrapT` default to REPEAT. [`SamplerData::default_sampler`]
/// returns exactly those defaults, matching the glTF 2.0 spec's implicit
/// sampler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SamplerData {
    /// Magnification filter (GL enum: 9728 NEAREST, 9729 LINEAR).
    /// `None` = "auto" (LINEAR).
    pub mag_filter: Option<u32>,
    /// Minification filter (GL enum: 9728, 9729, 9984–9987 mipmap variants).
    /// `None` = "auto" (LINEAR_MIPMAP_LINEAR).
    pub min_filter: Option<u32>,
    /// s wrapping mode (GL enum: 33071 CLAMP_TO_EDGE, 33648 MIRRORED_REPEAT,
    /// 10497 REPEAT).
    pub wrap_s: u32,
    /// t wrapping mode (same values as `wrap_s`).
    pub wrap_t: u32,
}

impl SamplerData {
    /// The implicit default sampler per the glTF 2.0 spec: repeat wrapping,
    /// auto filtering.
    pub const fn default_sampler() -> Self {
        Self {
            mag_filter: None,
            min_filter: None,
            wrap_s: GL_WRAP_REPEAT,
            wrap_t: GL_WRAP_REPEAT,
        }
    }
}

/// GL enum: `GL_NEAREST` (mag or min filter).
pub const GL_NEAREST: u32 = 9728;
/// GL enum: `GL_LINEAR` (mag or min filter).
pub const GL_LINEAR: u32 = 9729;
/// GL enum: `GL_NEAREST_MIPMAP_NEAREST` (min filter).
pub const GL_NEAREST_MIPMAP_NEAREST: u32 = 9984;
/// GL enum: `GL_LINEAR_MIPMAP_NEAREST` (min filter).
pub const GL_LINEAR_MIPMAP_NEAREST: u32 = 9985;
/// GL enum: `GL_NEAREST_MIPMAP_LINEAR` (min filter).
pub const GL_NEAREST_MIPMAP_LINEAR: u32 = 9986;
/// GL enum: `GL_LINEAR_MIPMAP_LINEAR` (min filter).
pub const GL_LINEAR_MIPMAP_LINEAR: u32 = 9987;
/// GL enum: `GL_CLAMP_TO_EDGE` (wrap mode).
pub const GL_CLAMP_TO_EDGE: u32 = 33071;
/// GL enum: `GL_MIRRORED_REPEAT` (wrap mode).
pub const GL_MIRRORED_REPEAT: u32 = 33648;
/// GL enum: `GL_REPEAT` (wrap mode).
pub const GL_WRAP_REPEAT: u32 = 10497;

/// One material texture slot: which image, which UV set, and how to sample.
///
/// glTF binds samplers per *texture object*, and a texture object pairs an
/// image with a sampler — the same image can appear under several texture
/// objects with different samplers. Slots therefore carry the resolved
/// [`SamplerData`] inline rather than an index into a separate list (the
/// struct is tiny; duplicating it per slot costs nothing and spares
/// consumers a default-sampler lookup).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureSlot {
    /// Image index into [`AssetData::textures`].
    pub image: usize,
    /// Which UV set the slot reads (`TEXCOORD_n`).
    pub tex_coord: u32,
    /// Sampling state — already resolved, so a texture object with no
    /// explicit sampler just yields [`SamplerData::default_sampler`].
    pub sampler: SamplerData,
}

impl Default for TextureSlot {
    fn default() -> Self {
        Self {
            image: 0,
            tex_coord: 0,
            sampler: SamplerData::default_sampler(),
        }
    }
}

impl TextureMime {
    /// Parse a MIME type string (e.g. from glTF `mimeType` fields).
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "image/png" => Some(Self::Png),
            "image/jpeg" | "image/jpg" => Some(Self::Jpeg),
            _ => None,
        }
    }

    /// Guess the MIME type from a file path or URI extension.
    pub fn from_uri(uri: &str) -> Option<Self> {
        let path = uri.split(['?', '#']).next().unwrap_or(uri);
        let ext = path.rsplit('.').next().unwrap_or("");
        match ext.to_ascii_lowercase().as_str() {
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpeg),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// MaterialData
// ---------------------------------------------------------------------------

/// Minimal material description extracted from an asset.
///
/// Intentionally simple for v0.1 — captures PBR base properties that GLB
/// and OBJ/MTL can provide. Extensible for future formats.
///
/// Texture references are [`TextureSlot`]s — an image index plus UV set and
/// sampler — so one texture can be shared by several materials.
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
    /// Emissive color as linear RGB.
    pub emissive: [f32; 3],
    /// How transparency is handled.
    pub alpha_mode: AlphaMode,
    /// Alpha threshold for [`AlphaMode::Mask`].
    pub alpha_cutoff: f32,
    /// Whether back faces should be rendered.
    pub double_sided: bool,
    /// Whether this material references texture data.
    pub has_textures: bool,
    /// Base color texture slot (image index, UV set, sampler).
    pub base_color_texture: Option<TextureSlot>,
    /// Normal map texture slot.
    pub normal_texture: Option<TextureSlot>,
    /// Metallic-roughness texture slot.
    pub metallic_roughness_texture: Option<TextureSlot>,
    /// Emissive texture slot.
    pub emissive_texture: Option<TextureSlot>,
}

impl Default for MaterialData {
    fn default() -> Self {
        Self {
            name: None,
            base_color: [1.0, 1.0, 1.0, 1.0],
            metallic: 0.0,
            roughness: 0.5,
            emissive: [0.0, 0.0, 0.0],
            alpha_mode: AlphaMode::Opaque,
            alpha_cutoff: 0.5,
            double_sided: false,
            has_textures: false,
            base_color_texture: None,
            normal_texture: None,
            metallic_roughness_texture: None,
            emissive_texture: None,
        }
    }
}

// ---------------------------------------------------------------------------
// NodeData
// ---------------------------------------------------------------------------

/// One node of a flattened scene hierarchy.
///
/// Node trees from formats like glTF are flattened into a single `Vec`:
/// children and mesh/material references are indices into the owning
/// [`AssetData`]. A mesh referenced by multiple nodes is instancing —
/// consumers that want unique geometry can duplicate it.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeData {
    /// Node name (from the file, if any).
    pub name: Option<String>,
    /// 4×4 column-major transform (feeds `glam::Mat4::from_cols_array`).
    pub transform: [f32; 16],
    /// Mesh index into [`AssetData::meshes`] (`None` = no mesh / empty node).
    pub mesh: Option<usize>,
    /// Per-node material override — index into [`AssetData::materials`].
    /// Rare; most formats bind materials per-mesh instead.
    pub material: Option<usize>,
    /// Child node indices into [`AssetData::nodes`].
    pub children: Vec<usize>,
}

impl Default for NodeData {
    fn default() -> Self {
        Self {
            name: None,
            transform: Self::IDENTITY,
            mesh: None,
            material: None,
            children: Vec::new(),
        }
    }
}

impl NodeData {
    /// Column-major identity matrix.
    pub const IDENTITY: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ];

    /// The translation part of the transform (column-major: elements 12–14).
    pub fn translation(&self) -> [f32; 3] {
        [self.transform[12], self.transform[13], self.transform[14]]
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
    /// Whether a node hierarchy is present.
    pub has_nodes: bool,

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
            has_nodes: false,
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

/// Full output of parsing an asset — geometry, materials, textures, and
/// (optionally) a node hierarchy.
///
/// Every parser ultimately produces an `AssetData`. Simple formats (STL)
/// yield a single mesh and nothing else; complex formats (GLB) may yield
/// many meshes, full PBR materials, textures, and a scene graph.
#[derive(Debug, Clone, Default)]
pub struct AssetData {
    /// One or more meshes extracted from the asset.
    pub meshes: Vec<MeshData>,
    /// Material descriptions, if the format provides them.
    pub materials: Vec<MaterialData>,
    /// Texture references, if the format provides them.
    ///
    /// Encoded bytes or file URIs — core never decodes image data.
    pub textures: Vec<TextureData>,
    /// Flattened scene nodes, if the format provides a hierarchy.
    pub nodes: Vec<NodeData>,
    /// Indices into [`AssetData::nodes`] that have no parent.
    pub root_nodes: Vec<usize>,
    /// Asset name (from the file, if any).
    pub name: Option<String>,
}

impl AssetData {
    /// Create an `AssetData` from a single mesh with no materials.
    pub fn single(mesh: MeshData) -> Self {
        Self {
            meshes: vec![mesh],
            ..Default::default()
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
        // Derived: textures live on the asset; `MaterialData::has_textures`
        // stays in sync for materials-only consumers.
        let has_textures =
            !self.textures.is_empty() || self.materials.iter().any(|m| m.has_textures);
        let has_nodes = !self.nodes.is_empty();
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
            has_nodes,
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

    // ── AlphaMode ───────────────────────────────────────────────────

    #[test]
    fn alpha_mode_default_is_opaque() {
        assert_eq!(AlphaMode::default(), AlphaMode::Opaque);
    }

    // ── TextureMime ──────────────────────────────────────────────

    #[test]
    fn texture_mime_parse() {
        assert_eq!(TextureMime::parse("image/png"), Some(TextureMime::Png));
        assert_eq!(TextureMime::parse("image/jpeg"), Some(TextureMime::Jpeg));
        assert_eq!(TextureMime::parse("image/jpg"), Some(TextureMime::Jpeg));
        assert_eq!(TextureMime::parse("image/webp"), None);
    }

    #[test]
    fn texture_mime_from_uri() {
        assert_eq!(
            TextureMime::from_uri("textures/brick.png"),
            Some(TextureMime::Png)
        );
        assert_eq!(TextureMime::from_uri("photo.JPG"), Some(TextureMime::Jpeg));
        assert_eq!(TextureMime::from_uri("noext"), None);
    }

    #[test]
    fn texture_source_embedded_mime() {
        let src = TextureSource::Embedded {
            bytes: vec![1, 2, 3],
            mime: TextureMime::Png,
        };
        assert_eq!(src.mime(), Some(TextureMime::Png));
    }

    #[test]
    fn texture_source_uri_mime_guessed() {
        let src = TextureSource::Uri("textures/brick.png".to_string());
        assert_eq!(src.mime(), Some(TextureMime::Png));
    }

    // ── NodeData ─────────────────────────────────────────────────

    #[test]
    fn node_data_default_is_identity() {
        let node = NodeData::default();
        assert_eq!(node.transform, NodeData::IDENTITY);
        assert_eq!(node.translation(), [0.0, 0.0, 0.0]);
        assert!(node.mesh.is_none());
        assert!(node.children.is_empty());
    }

    #[test]
    fn node_data_translation_reads_column_major() {
        let mut node = NodeData::default();
        node.transform[12] = 1.0;
        node.transform[13] = 2.0;
        node.transform[14] = 3.0;
        assert_eq!(node.translation(), [1.0, 2.0, 3.0]);
    }

    // ── AssetData with textures/nodes ────────────────────────────

    #[test]
    fn asset_data_with_textures_and_nodes_round_trips() {
        let data = AssetData {
            meshes: vec![sample_triangle()],
            textures: vec![TextureData {
                source: TextureSource::Embedded {
                    bytes: vec![0x89, b'P'],
                    mime: TextureMime::Png,
                },
                name: Some("red".into()),
            }],
            nodes: vec![NodeData {
                name: Some("root".into()),
                mesh: Some(0),
                ..Default::default()
            }],
            root_nodes: vec![0],
            name: Some("thing".into()),
            ..Default::default()
        };

        // Debug round-trip
        let debug = format!("{:?}", data);
        assert!(debug.contains("red"));

        // Clone round-trip
        let clone = data.clone();
        assert_eq!(clone.textures.len(), 1);
        assert_eq!(clone.nodes.len(), 1);
        assert_eq!(clone.root_nodes, vec![0]);
    }

    #[test]
    fn capabilities_derive_has_textures_from_asset_textures() {
        let data = AssetData {
            meshes: vec![sample_triangle()],
            textures: vec![TextureData {
                source: TextureSource::Uri("t.png".into()),
                name: None,
            }],
            ..Default::default()
        };
        assert!(data.capabilities().has_textures);
    }

    #[test]
    fn capabilities_derive_has_nodes() {
        let data = AssetData {
            nodes: vec![NodeData::default()],
            root_nodes: vec![0],
            ..Default::default()
        };
        let cap = data.capabilities();
        assert!(cap.has_nodes);
        assert!(!cap.has_materials);
    }

    // ── AssetCapabilities defaults ───────────────────────────────

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
        assert!(!cap.has_nodes);
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
        };
        let cap = data.capabilities();
        assert!(cap.has_materials);
        assert!(!cap.has_textures);
    }

    // ── Empty AssetData ───────────────────────────────────────────────

    #[test]
    fn empty_asset_data() {
        let data = AssetData::default();
        assert_eq!(data.total_vertices(), 0);
        assert_eq!(data.total_triangles(), 0);
        let cap = data.capabilities();
        assert!(!cap.has_normals);
        assert!(!cap.multi_object);
        assert_eq!(cap.estimated_memory, Some(0));
    }
}
