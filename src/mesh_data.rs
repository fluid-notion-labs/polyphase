//! Format-agnostic intermediate mesh representation.
//!
//! [`MeshData`] is the universal intermediate type that every parser produces
//! and every consumer (Bevy, CLI, pipeline) reads from. It carries geometry
//! data (positions, normals, UVs, indices) with no engine dependency.

use crate::capabilities::TopologyKind;
use thiserror::Error;

/// Format-agnostic mesh data produced by all loadfu parsers.
///
/// Every parser (`stl`, `obj`, `glb`) returns `Result<MeshData>`. Consumers
/// convert from `MeshData` into their target representation (e.g.
/// `Into<bevy::Mesh>` behind the `bevy` feature flag).
///
/// # Invariants
///
/// A valid `MeshData` satisfies:
/// - `positions` is non-empty.
/// - `indices` is non-empty and every index is `< positions.len()`.
/// - If `normals` is `Some`, its length equals `positions.len()`.
/// - If `uvs` is `Some`, its length equals `positions.len()`.
/// - If `vertex_colors` is `Some`, its length equals `positions.len()`.
/// - `indices.len()` is a multiple of the topology's stride (3 for triangle
///   list, 2 for line list, 1 for point list, or ≥ 3 for triangle strip).
#[derive(Debug, Clone, PartialEq)]
pub struct MeshData {
    /// Primitive topology of this mesh data.
    ///
    /// Defaults to [`TopologyKind::TriangleList`]. Parsers that produce line
    /// or point data set this explicitly.
    pub topology: TopologyKind,

    /// Vertex positions as `[x, y, z]`.
    pub positions: Vec<[f32; 3]>,

    /// Per-vertex normals as `[nx, ny, nz]`.
    ///
    /// `None` if the source format doesn't provide normals (rare — most
    /// consumers will want to compute them).
    pub normals: Option<Vec<[f32; 3]>>,

    /// Per-vertex texture coordinates as `[u, v]`.
    ///
    /// `None` for formats that never carry UVs (e.g. STL).
    pub uvs: Option<Vec<[f32; 2]>>,

    /// Per-vertex colors as linear `[r, g, b, a]`.
    ///
    /// `None` for formats that don't carry vertex colors. Common in PLY
    /// (scanner output) and supported by GLB.
    pub vertex_colors: Option<Vec<[f32; 4]>>,

    /// Indices into the `positions` (and `normals`/`uvs`) arrays.
    ///
    /// Interpretation depends on `topology`:
    /// - **TriangleList:** flat triangle list — length is a multiple of 3.
    /// - **LineList:** flat line segments — length is a multiple of 2.
    /// - **TriangleStrip:** strip — length ≥ 3.
    /// - **PointList:** individual points — any length ≥ 1.
    pub indices: Vec<u32>,

    /// Material index into [`AssetData::materials`](crate::AssetData::materials),
    /// when the format binds materials per-primitive/per-object (GLB, OBJ+MTL).
    ///
    /// `None` for geometry-only formats (STL, PLY) or unbound primitives.
    pub material_index: Option<usize>,
}

/// Errors detected by [`MeshData::validate`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MeshDataError {
    /// `positions` is empty.
    #[error("MeshData has no positions")]
    EmptyPositions,

    /// `indices` is empty.
    #[error("MeshData has no indices")]
    EmptyIndices,

    /// `indices.len()` is not aligned to the topology's stride.
    #[error(
        "index count {count} is not a multiple of {stride} (required by {topology:?} topology)"
    )]
    IndicesNotAligned {
        /// Actual index count.
        count: usize,
        /// Required multiple (e.g. 3 for TriangleList, 2 for LineList).
        stride: usize,
        /// The topology that imposed this requirement.
        topology: TopologyKind,
    },

    /// For strip topologies, there aren't enough indices to form a primitive.
    #[error("index count {count} is too few for {topology:?} topology (need at least {minimum})")]
    InsufficientIndices {
        /// Actual index count.
        count: usize,
        /// Minimum required.
        minimum: usize,
        /// The topology that imposed this requirement.
        topology: TopologyKind,
    },

    /// An index references a vertex that doesn't exist.
    #[error("index {index} is out of bounds for {vertex_count} vertices")]
    IndexOutOfBounds {
        /// The offending index value.
        index: u32,
        /// Number of vertices (`positions.len()`).
        vertex_count: usize,
    },

    /// `normals` length doesn't match `positions` length.
    #[error("normals length ({normals}) doesn't match positions length ({positions})")]
    NormalsLengthMismatch {
        /// Number of normals provided.
        normals: usize,
        /// Number of positions (expected).
        positions: usize,
    },

    /// `uvs` length doesn't match `positions` length.
    #[error("UVs length ({uvs}) doesn't match positions length ({positions})")]
    UvsLengthMismatch {
        /// Number of UV coords provided.
        uvs: usize,
        /// Number of positions (expected).
        positions: usize,
    },

    /// `vertex_colors` length doesn't match `positions` length.
    #[error("vertex_colors length ({colors}) doesn't match positions length ({positions})")]
    VertexColorsLengthMismatch {
        /// Number of vertex colors provided.
        colors: usize,
        /// Number of positions (expected).
        positions: usize,
    },
}

impl MeshData {
    /// Create a new `MeshData` with triangle-list topology, positions, and
    /// indices only (no normals or UVs).
    pub fn new(positions: Vec<[f32; 3]>, indices: Vec<u32>) -> Self {
        Self {
            topology: TopologyKind::TriangleList,
            positions,
            normals: None,
            uvs: None,
            vertex_colors: None,
            indices,
            material_index: None,
        }
    }

    /// Create a new `MeshData` with explicit topology.
    pub fn with_topology(
        topology: TopologyKind,
        positions: Vec<[f32; 3]>,
        indices: Vec<u32>,
    ) -> Self {
        Self {
            topology,
            positions,
            normals: None,
            uvs: None,
            vertex_colors: None,
            indices,
            material_index: None,
        }
    }

    /// Builder-style setter for normals.
    pub fn with_normals(mut self, normals: Vec<[f32; 3]>) -> Self {
        self.normals = Some(normals);
        self
    }

    /// Builder-style setter for UVs.
    pub fn with_uvs(mut self, uvs: Vec<[f32; 2]>) -> Self {
        self.uvs = Some(uvs);
        self
    }

    /// Builder-style setter for vertex colors.
    pub fn with_vertex_colors(mut self, colors: Vec<[f32; 4]>) -> Self {
        self.vertex_colors = Some(colors);
        self
    }

    /// Number of vertices.
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// Number of primitives based on the topology.
    ///
    /// - **TriangleList:** `indices.len() / 3`
    /// - **TriangleStrip:** `indices.len().saturating_sub(2)`
    /// - **LineList:** `indices.len() / 2`
    /// - **PointList:** `indices.len()`
    pub fn primitive_count(&self) -> usize {
        match self.topology {
            TopologyKind::TriangleList => self.indices.len() / 3,
            TopologyKind::TriangleStrip => self.indices.len().saturating_sub(2),
            TopologyKind::LineList => self.indices.len() / 2,
            TopologyKind::PointList => self.indices.len(),
        }
    }

    /// Number of triangles (indices / 3).
    ///
    /// Returns 0 if the topology is not a triangle type or if
    /// `indices.len()` is not a multiple of 3 for triangle lists.
    pub fn triangle_count(&self) -> usize {
        match self.topology {
            TopologyKind::TriangleList => self.indices.len() / 3,
            TopologyKind::TriangleStrip => self.indices.len().saturating_sub(2),
            TopologyKind::LineList | TopologyKind::PointList => 0,
        }
    }

    /// Whether this mesh carries per-vertex normals.
    pub fn has_normals(&self) -> bool {
        self.normals.is_some()
    }

    /// Whether this mesh carries per-vertex UVs.
    pub fn has_uvs(&self) -> bool {
        self.uvs.is_some()
    }

    /// Whether this mesh carries per-vertex colors.
    pub fn has_vertex_colors(&self) -> bool {
        self.vertex_colors.is_some()
    }

    /// Whether this mesh uses triangle topology (list or strip).
    pub fn is_triangle_topology(&self) -> bool {
        matches!(
            self.topology,
            TopologyKind::TriangleList | TopologyKind::TriangleStrip
        )
    }

    /// Whether this mesh uses line topology.
    pub fn is_line_topology(&self) -> bool {
        matches!(self.topology, TopologyKind::LineList)
    }

    /// Whether this mesh uses point topology.
    pub fn is_point_topology(&self) -> bool {
        matches!(self.topology, TopologyKind::PointList)
    }

    /// Validate the mesh data, returning the first error found (if any).
    ///
    /// This is intentionally cheap — it does a single pass over indices to
    /// check bounds, plus length comparisons for normals/UVs. Alignment
    /// checks are topology-aware.
    pub fn validate(&self) -> Result<(), MeshDataError> {
        if self.positions.is_empty() {
            return Err(MeshDataError::EmptyPositions);
        }

        if self.indices.is_empty() {
            return Err(MeshDataError::EmptyIndices);
        }

        // Topology-specific index count validation
        match self.topology {
            TopologyKind::TriangleList => {
                if self.indices.len() % 3 != 0 {
                    return Err(MeshDataError::IndicesNotAligned {
                        count: self.indices.len(),
                        stride: 3,
                        topology: self.topology,
                    });
                }
            }
            TopologyKind::LineList => {
                if self.indices.len() % 2 != 0 {
                    return Err(MeshDataError::IndicesNotAligned {
                        count: self.indices.len(),
                        stride: 2,
                        topology: self.topology,
                    });
                }
            }
            TopologyKind::TriangleStrip => {
                if self.indices.len() < 3 {
                    return Err(MeshDataError::InsufficientIndices {
                        count: self.indices.len(),
                        minimum: 3,
                        topology: self.topology,
                    });
                }
            }
            TopologyKind::PointList => {
                // Any non-empty count is valid for points (already checked above).
            }
        }

        let vertex_count = self.positions.len();

        // Check index bounds
        for &idx in &self.indices {
            if (idx as usize) >= vertex_count {
                return Err(MeshDataError::IndexOutOfBounds {
                    index: idx,
                    vertex_count,
                });
            }
        }

        // Check normals length
        if let Some(ref normals) = self.normals {
            if normals.len() != vertex_count {
                return Err(MeshDataError::NormalsLengthMismatch {
                    normals: normals.len(),
                    positions: vertex_count,
                });
            }
        }

        // Check UVs length
        if let Some(ref uvs) = self.uvs {
            if uvs.len() != vertex_count {
                return Err(MeshDataError::UvsLengthMismatch {
                    uvs: uvs.len(),
                    positions: vertex_count,
                });
            }
        }

        // Check vertex colors length
        if let Some(ref colors) = self.vertex_colors {
            if colors.len() != vertex_count {
                return Err(MeshDataError::VertexColorsLengthMismatch {
                    colors: colors.len(),
                    positions: vertex_count,
                });
            }
        }

        Ok(())
    }

    /// Validate and return `self` if valid. Convenience for parser pipelines.
    pub fn validated(self) -> Result<Self, MeshDataError> {
        self.validate()?;
        Ok(self)
    }

    /// Estimate the heap memory usage in bytes (positions + normals + uvs + indices).
    pub fn estimated_memory_bytes(&self) -> usize {
        let positions_bytes = self.positions.len() * std::mem::size_of::<[f32; 3]>();
        let normals_bytes = self
            .normals
            .as_ref()
            .map_or(0, |n| n.len() * std::mem::size_of::<[f32; 3]>());
        let uvs_bytes = self
            .uvs
            .as_ref()
            .map_or(0, |u| u.len() * std::mem::size_of::<[f32; 2]>());
        let colors_bytes = self
            .vertex_colors
            .as_ref()
            .map_or(0, |c| c.len() * std::mem::size_of::<[f32; 4]>());
        let indices_bytes = self.indices.len() * std::mem::size_of::<u32>();

        positions_bytes + normals_bytes + uvs_bytes + colors_bytes + indices_bytes
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::sample_triangle;

    // ── Construction ──────────────────────────────────────────────────

    #[test]
    fn new_creates_minimal_mesh() {
        let mesh = MeshData::new(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 2],
        );
        assert_eq!(mesh.topology, TopologyKind::TriangleList);
        assert!(!mesh.has_normals());
        assert!(!mesh.has_uvs());
        assert_eq!(mesh.vertex_count(), 3);
        assert_eq!(mesh.triangle_count(), 1);
        assert_eq!(mesh.primitive_count(), 1);
        assert_eq!(mesh.material_index, None);
    }

    #[test]
    fn with_topology_creates_line_mesh() {
        let mesh = MeshData::with_topology(
            TopologyKind::LineList,
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            vec![0, 1],
        );
        assert_eq!(mesh.topology, TopologyKind::LineList);
        assert_eq!(mesh.primitive_count(), 1);
        assert_eq!(mesh.triangle_count(), 0);
        assert!(mesh.is_line_topology());
        assert!(!mesh.is_triangle_topology());
    }

    #[test]
    fn with_topology_creates_point_mesh() {
        let mesh = MeshData::with_topology(
            TopologyKind::PointList,
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
            vec![0, 1, 2],
        );
        assert_eq!(mesh.topology, TopologyKind::PointList);
        assert_eq!(mesh.primitive_count(), 3);
        assert_eq!(mesh.triangle_count(), 0);
        assert!(mesh.is_point_topology());
    }

    #[test]
    fn builder_methods() {
        let mesh = MeshData::new(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 2],
        )
        .with_normals(vec![[0.0, 0.0, 1.0]; 3])
        .with_uvs(vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]])
        .with_vertex_colors(vec![[1.0, 0.0, 0.0, 1.0]; 3]);

        assert!(mesh.has_normals());
        assert!(mesh.has_uvs());
        assert!(mesh.has_vertex_colors());
    }

    // ── Topology classification ───────────────────────────────────────

    #[test]
    fn triangle_list_is_triangle_topology() {
        let mesh = MeshData::new(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 2],
        );
        assert!(mesh.is_triangle_topology());
        assert!(!mesh.is_line_topology());
        assert!(!mesh.is_point_topology());
    }

    #[test]
    fn triangle_strip_is_triangle_topology() {
        let mesh = MeshData::with_topology(
            TopologyKind::TriangleStrip,
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [1.0, 1.0, 0.0],
            ],
            vec![0, 1, 2, 3],
        );
        assert!(mesh.is_triangle_topology());
        assert_eq!(mesh.triangle_count(), 2);
        assert_eq!(mesh.primitive_count(), 2);
    }

    // ── Primitive count ───────────────────────────────────────────────

    #[test]
    fn primitive_count_triangle_list() {
        let mesh = MeshData::new(
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [1.0, 1.0, 0.0],
            ],
            vec![0, 1, 2, 1, 3, 2],
        );
        assert_eq!(mesh.primitive_count(), 2);
    }

    #[test]
    fn primitive_count_line_list() {
        let mesh = MeshData::with_topology(
            TopologyKind::LineList,
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [2.0, 0.0, 0.0],
                [3.0, 0.0, 0.0],
            ],
            vec![0, 1, 2, 3],
        );
        assert_eq!(mesh.primitive_count(), 2);
    }

    #[test]
    fn primitive_count_point_list() {
        let mesh = MeshData::with_topology(
            TopologyKind::PointList,
            vec![[0.0, 0.0, 0.0]; 5],
            vec![0, 1, 2, 3, 4],
        );
        assert_eq!(mesh.primitive_count(), 5);
    }

    #[test]
    fn primitive_count_triangle_strip() {
        let mesh = MeshData::with_topology(
            TopologyKind::TriangleStrip,
            vec![[0.0, 0.0, 0.0]; 5],
            vec![0, 1, 2, 3, 4],
        );
        // 5 indices → 3 triangles in a strip
        assert_eq!(mesh.primitive_count(), 3);
    }

    // ── Validation: valid cases ───────────────────────────────────────

    #[test]
    fn valid_triangle_passes() {
        assert!(sample_triangle().validate().is_ok());
    }

    #[test]
    fn valid_no_normals_no_uvs() {
        let mesh = MeshData::new(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 2],
        );
        assert!(mesh.validate().is_ok());
    }

    #[test]
    fn valid_two_triangles() {
        let mesh = MeshData::new(
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [1.0, 1.0, 0.0],
            ],
            vec![0, 1, 2, 1, 3, 2],
        );
        assert!(mesh.validate().is_ok());
        assert_eq!(mesh.triangle_count(), 2);
    }

    #[test]
    fn valid_line_list() {
        let mesh = MeshData::with_topology(
            TopologyKind::LineList,
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            vec![0, 1],
        );
        assert!(mesh.validate().is_ok());
    }

    #[test]
    fn valid_point_list() {
        let mesh = MeshData::with_topology(
            TopologyKind::PointList,
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            vec![0, 1],
        );
        assert!(mesh.validate().is_ok());
    }

    #[test]
    fn valid_triangle_strip() {
        let mesh = MeshData::with_topology(
            TopologyKind::TriangleStrip,
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [1.0, 1.0, 0.0],
            ],
            vec![0, 1, 2, 3],
        );
        assert!(mesh.validate().is_ok());
    }

    #[test]
    fn validated_returns_self() {
        let mesh = sample_triangle();
        let result = mesh.clone().validated();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), mesh);
    }

    // ── Validation: error cases ───────────────────────────────────────

    #[test]
    fn empty_positions() {
        let mesh = MeshData::new(vec![], vec![0, 1, 2]);
        assert_eq!(mesh.validate(), Err(MeshDataError::EmptyPositions));
    }

    #[test]
    fn empty_indices() {
        let mesh = MeshData::new(vec![[0.0, 0.0, 0.0]], vec![]);
        assert_eq!(mesh.validate(), Err(MeshDataError::EmptyIndices));
    }

    #[test]
    fn triangle_list_indices_not_aligned() {
        let mesh = MeshData::new(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1], // only 2 indices
        );
        assert_eq!(
            mesh.validate(),
            Err(MeshDataError::IndicesNotAligned {
                count: 2,
                stride: 3,
                topology: TopologyKind::TriangleList,
            })
        );
    }

    #[test]
    fn line_list_indices_not_aligned() {
        let mesh = MeshData::with_topology(
            TopologyKind::LineList,
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
            vec![0, 1, 2], // 3 indices, not a multiple of 2
        );
        assert_eq!(
            mesh.validate(),
            Err(MeshDataError::IndicesNotAligned {
                count: 3,
                stride: 2,
                topology: TopologyKind::LineList,
            })
        );
    }

    #[test]
    fn triangle_strip_insufficient_indices() {
        let mesh = MeshData::with_topology(
            TopologyKind::TriangleStrip,
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            vec![0, 1], // only 2 indices, need at least 3
        );
        assert_eq!(
            mesh.validate(),
            Err(MeshDataError::InsufficientIndices {
                count: 2,
                minimum: 3,
                topology: TopologyKind::TriangleStrip,
            })
        );
    }

    #[test]
    fn index_out_of_bounds() {
        let mesh = MeshData::new(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 5], // index 5 is invalid
        );
        assert_eq!(
            mesh.validate(),
            Err(MeshDataError::IndexOutOfBounds {
                index: 5,
                vertex_count: 3,
            })
        );
    }

    #[test]
    fn normals_length_mismatch() {
        let mut mesh = sample_triangle();
        mesh.normals = Some(vec![[0.0, 0.0, 1.0]; 2]); // 2 normals, 3 positions
        assert_eq!(
            mesh.validate(),
            Err(MeshDataError::NormalsLengthMismatch {
                normals: 2,
                positions: 3,
            })
        );
    }

    #[test]
    fn uvs_length_mismatch() {
        let mut mesh = sample_triangle();
        mesh.uvs = Some(vec![[0.0, 0.0]; 5]); // 5 UVs, 3 positions
        assert_eq!(
            mesh.validate(),
            Err(MeshDataError::UvsLengthMismatch {
                uvs: 5,
                positions: 3,
            })
        );
    }

    #[test]
    fn vertex_colors_length_mismatch() {
        let mut mesh = sample_triangle();
        mesh.vertex_colors = Some(vec![[1.0, 0.0, 0.0, 1.0]; 2]); // 2 colors, 3 positions
        assert_eq!(
            mesh.validate(),
            Err(MeshDataError::VertexColorsLengthMismatch {
                colors: 2,
                positions: 3,
            })
        );
    }

    #[test]
    fn validated_returns_error() {
        let mesh = MeshData::new(vec![], vec![]);
        assert!(mesh.validated().is_err());
    }

    // ── Memory estimation ─────────────────────────────────────────────

    #[test]
    fn estimated_memory_positions_only() {
        let mesh = MeshData::new(
            vec![[0.0, 0.0, 0.0]; 100],
            vec![0; 300], // 100 triangles
        );
        // 100 positions * 12 bytes + 300 indices * 4 bytes = 1200 + 1200 = 2400
        assert_eq!(mesh.estimated_memory_bytes(), 2400);
    }

    #[test]
    fn estimated_memory_full() {
        let mesh = MeshData {
            topology: TopologyKind::TriangleList,
            positions: vec![[0.0; 3]; 100],
            normals: Some(vec![[0.0; 3]; 100]),
            uvs: Some(vec![[0.0; 2]; 100]),
            vertex_colors: None,
            indices: vec![0; 300],
            material_index: None,
        };
        // positions: 100 * 12 = 1200
        // normals:   100 * 12 = 1200
        // uvs:       100 *  8 =  800
        // indices:   300 *  4 = 1200
        // total:                 4400
        assert_eq!(mesh.estimated_memory_bytes(), 4400);
    }

    #[test]
    fn estimated_memory_with_vertex_colors() {
        let mesh = MeshData {
            topology: TopologyKind::TriangleList,
            positions: vec![[0.0; 3]; 100],
            normals: None,
            uvs: None,
            vertex_colors: Some(vec![[1.0; 4]; 100]),
            indices: vec![0; 300],
            material_index: None,
        };
        // positions: 100 * 12 = 1200
        // colors:    100 * 16 = 1600
        // indices:   300 *  4 = 1200
        // total:                 4000
        assert_eq!(mesh.estimated_memory_bytes(), 4000);
    }

    // ── Display for errors ────────────────────────────────────────────

    #[test]
    fn error_display() {
        let err = MeshDataError::IndexOutOfBounds {
            index: 42,
            vertex_count: 10,
        };
        let msg = format!("{err}");
        assert!(msg.contains("42"));
        assert!(msg.contains("10"));
    }

    #[test]
    fn error_display_indices_not_aligned() {
        let err = MeshDataError::IndicesNotAligned {
            count: 5,
            stride: 3,
            topology: TopologyKind::TriangleList,
        };
        let msg = format!("{err}");
        assert!(msg.contains("5"));
        assert!(msg.contains("3"));
        assert!(msg.contains("TriangleList"));
    }

    #[test]
    fn error_display_insufficient_indices() {
        let err = MeshDataError::InsufficientIndices {
            count: 2,
            minimum: 3,
            topology: TopologyKind::TriangleStrip,
        };
        let msg = format!("{err}");
        assert!(msg.contains("2"));
        assert!(msg.contains("3"));
        assert!(msg.contains("TriangleStrip"));
    }
}
