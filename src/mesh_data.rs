//! Format-agnostic intermediate mesh representation.
//!
//! [`MeshData`] is the universal intermediate type that every parser produces
//! and every consumer (Bevy, CLI, pipeline) reads from. It carries geometry
//! data (positions, normals, UVs, indices) with no engine dependency.

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
/// - `indices.len()` is a multiple of 3 (triangle list).
#[derive(Debug, Clone, PartialEq)]
pub struct MeshData {
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

    /// Triangle indices into the `positions` (and `normals`/`uvs`) arrays.
    ///
    /// Always a flat triangle list — length is a multiple of 3.
    pub indices: Vec<u32>,
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

    /// `indices.len()` is not a multiple of 3.
    #[error("index count {count} is not a multiple of 3 (not a valid triangle list)")]
    IndicesNotTriangleAligned {
        /// Actual index count.
        count: usize,
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
}

impl MeshData {
    /// Create a new `MeshData` with only positions and indices (no normals or UVs).
    pub fn new(positions: Vec<[f32; 3]>, indices: Vec<u32>) -> Self {
        Self {
            positions,
            normals: None,
            uvs: None,
            indices,
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

    /// Number of vertices.
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// Number of triangles (indices / 3).
    ///
    /// Returns 0 if `indices.len()` is not a multiple of 3.
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Whether this mesh carries per-vertex normals.
    pub fn has_normals(&self) -> bool {
        self.normals.is_some()
    }

    /// Whether this mesh carries per-vertex UVs.
    pub fn has_uvs(&self) -> bool {
        self.uvs.is_some()
    }

    /// Validate the mesh data, returning the first error found (if any).
    ///
    /// This is intentionally cheap — it does a single pass over indices to
    /// check bounds, plus length comparisons for normals/UVs.
    pub fn validate(&self) -> Result<(), MeshDataError> {
        if self.positions.is_empty() {
            return Err(MeshDataError::EmptyPositions);
        }

        if self.indices.is_empty() {
            return Err(MeshDataError::EmptyIndices);
        }

        if self.indices.len() % 3 != 0 {
            return Err(MeshDataError::IndicesNotTriangleAligned {
                count: self.indices.len(),
            });
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
        let indices_bytes = self.indices.len() * std::mem::size_of::<u32>();

        positions_bytes + normals_bytes + uvs_bytes + indices_bytes
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
        assert!(!mesh.has_normals());
        assert!(!mesh.has_uvs());
        assert_eq!(mesh.vertex_count(), 3);
        assert_eq!(mesh.triangle_count(), 1);
    }

    #[test]
    fn builder_methods() {
        let mesh = MeshData::new(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 2],
        )
        .with_normals(vec![[0.0, 0.0, 1.0]; 3])
        .with_uvs(vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]);

        assert!(mesh.has_normals());
        assert!(mesh.has_uvs());
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
    fn indices_not_aligned() {
        let mesh = MeshData::new(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1], // only 2 indices
        );
        assert_eq!(
            mesh.validate(),
            Err(MeshDataError::IndicesNotTriangleAligned { count: 2 })
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
            positions: vec![[0.0; 3]; 100],
            normals: Some(vec![[0.0; 3]; 100]),
            uvs: Some(vec![[0.0; 2]; 100]),
            indices: vec![0; 300],
        };
        // positions: 100 * 12 = 1200
        // normals:   100 * 12 = 1200
        // uvs:       100 *  8 =  800
        // indices:   300 *  4 = 1200
        // total:                 4400
        assert_eq!(mesh.estimated_memory_bytes(), 4400);
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
}
