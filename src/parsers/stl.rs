//! STL parser — pure `&[u8] → Result<MeshData, StlError>`.
//!
//! Supports both binary and ASCII STL formats via the `stl_io` crate.
//! Produces indexed [`MeshData`] with positions, normals, and u32 indices.
//!
//! Extracted from seaview's `asset_loaders/stl_loader.rs`, with all Bevy
//! types replaced by [`MeshData`].

use crate::error::StlError;
use crate::mesh_data::MeshData;
use std::io::Cursor;

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

/// Options for STL parsing.
#[derive(Debug, Clone)]
pub struct StlOptions {
    /// If `true`, recompute smooth vertex normals from geometry instead of
    /// using the per-face normals stored in the STL file.
    ///
    /// Defaults to `false` (use STL face normals, accumulated per-vertex).
    pub recompute_normals: bool,
}

impl Default for StlOptions {
    fn default() -> Self {
        Self {
            recompute_normals: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse an STL file from bytes using default options.
///
/// Auto-detects binary vs ASCII STL.
///
/// # Errors
///
/// Returns [`StlError::Io`] if the data is malformed, or
/// [`StlError::EmptyMesh`] if the file contains zero triangles.
pub fn parse(bytes: &[u8]) -> Result<MeshData, StlError> {
    parse_with_options(bytes, &StlOptions::default())
}

/// Parse an STL file from bytes with explicit options.
///
/// See [`StlOptions`] for available settings.
pub fn parse_with_options(bytes: &[u8], options: &StlOptions) -> Result<MeshData, StlError> {
    let mut cursor = Cursor::new(bytes);
    let indexed_mesh = stl_io::read_stl(&mut cursor)?;

    if indexed_mesh.faces.is_empty() {
        return Err(StlError::EmptyMesh);
    }

    let mesh = if options.recompute_normals {
        build_mesh_auto_normals(&indexed_mesh)
    } else {
        build_mesh_with_stl_normals(&indexed_mesh)
    };

    Ok(mesh)
}

// ---------------------------------------------------------------------------
// Internal builders
// ---------------------------------------------------------------------------

/// Build [`MeshData`] using per-vertex normals accumulated from the STL's
/// per-face normals.
///
/// STL stores one normal per triangle. We accumulate (add) each face normal
/// onto every vertex the face references, then normalize. Vertices shared by
/// faces with similar normals end up smooth; vertices at hard edges get a
/// reasonable average.
fn build_mesh_with_stl_normals(stl: &stl_io::IndexedMesh) -> MeshData {
    let num_vertices = stl.vertices.len();

    let positions: Vec<[f32; 3]> = stl.vertices.iter().map(|v| (*v).into()).collect();

    // Accumulate face normals onto each vertex.
    let mut normals = vec![[0.0f32; 3]; num_vertices];
    for face in &stl.faces {
        let n: [f32; 3] = face.normal.into();
        for &vi in &face.vertices {
            normals[vi][0] += n[0];
            normals[vi][1] += n[1];
            normals[vi][2] += n[2];
        }
    }

    // Normalize. Fall back to +Y for degenerate cases.
    for normal in &mut normals {
        let len = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
        if len > f32::EPSILON {
            normal[0] /= len;
            normal[1] /= len;
            normal[2] /= len;
        } else {
            *normal = [0.0, 1.0, 0.0];
        }
    }

    let indices: Vec<u32> = stl
        .faces
        .iter()
        .flat_map(|face| face.vertices.iter().map(|&idx| idx as u32))
        .collect();

    MeshData {
        positions,
        normals: Some(normals),
        uvs: None,
        indices,
    }
}

/// Build [`MeshData`] with smooth normals computed from geometry.
///
/// Uses the indexed representation directly: for each triangle, compute the
/// face normal from the cross product of its edges, accumulate onto each
/// vertex, then normalize. This produces smooth normals from connectivity
/// alone, ignoring whatever normals the STL file stored.
fn build_mesh_auto_normals(stl: &stl_io::IndexedMesh) -> MeshData {
    let positions: Vec<[f32; 3]> = stl.vertices.iter().map(|v| (*v).into()).collect();

    let indices: Vec<u32> = stl
        .faces
        .iter()
        .flat_map(|face| face.vertices.iter().map(|&idx| idx as u32))
        .collect();

    let normals = compute_smooth_normals(&positions, &indices);

    MeshData {
        positions,
        normals: Some(normals),
        uvs: None,
        indices,
    }
}

/// Compute smooth per-vertex normals from positions and a triangle-list index buffer.
///
/// For each triangle, computes the face normal via cross product of edges,
/// accumulates it onto each of the triangle's vertices, then normalizes.
/// Degenerate normals fall back to `[0, 1, 0]`.
pub(crate) fn compute_smooth_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut normals = vec![[0.0f32; 3]; positions.len()];

    for tri in indices.chunks_exact(3) {
        let (i0, i1, i2) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
        let p0 = positions[i0];
        let p1 = positions[i1];
        let p2 = positions[i2];

        // Edge vectors
        let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let e2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];

        // Cross product = face normal (unnormalized — magnitude is proportional
        // to triangle area, which gives area-weighted accumulation for free).
        let n = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];

        for &vi in &[i0, i1, i2] {
            normals[vi][0] += n[0];
            normals[vi][1] += n[1];
            normals[vi][2] += n[2];
        }
    }

    // Normalize
    for normal in &mut normals {
        let len = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
        if len > f32::EPSILON {
            normal[0] /= len;
            normal[1] /= len;
            normal[2] /= len;
        } else {
            *normal = [0.0, 1.0, 0.0];
        }
    }

    normals
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::fixture;

    // ── Binary STL ────────────────────────────────────────────────────

    #[test]
    fn parse_binary_triangle() {
        let mesh = parse(&fixture("triangle.stl")).unwrap();

        assert!(mesh.validate().is_ok());
        assert_eq!(mesh.vertex_count(), 3);
        assert_eq!(mesh.triangle_count(), 1);
        assert!(mesh.has_normals());
        assert!(!mesh.has_uvs());
    }

    #[test]
    fn parse_binary_quad() {
        let mesh = parse(&fixture("quad.stl")).unwrap();

        assert!(mesh.validate().is_ok());
        assert_eq!(mesh.vertex_count(), 4);
        assert_eq!(mesh.triangle_count(), 2);
    }

    // ── ASCII STL ─────────────────────────────────────────────────────

    #[test]
    fn parse_ascii_triangle() {
        let mesh = parse(&fixture("triangle_ascii.stl")).unwrap();

        assert!(mesh.validate().is_ok());
        assert_eq!(mesh.vertex_count(), 3);
        assert_eq!(mesh.triangle_count(), 1);
        assert!(mesh.has_normals());
    }

    // ── Normals ───────────────────────────────────────────────────────

    #[test]
    fn face_normals_point_up() {
        let mesh = parse(&fixture("triangle.stl")).unwrap();
        let normals = mesh.normals.as_ref().unwrap();

        for n in normals {
            assert!((n[2] - 1.0).abs() < 0.001, "expected ≈ [0,0,1], got {n:?}");
        }
    }

    #[test]
    fn recompute_normals() {
        let opts = StlOptions { recompute_normals: true };
        let mesh = parse_with_options(&fixture("triangle.stl"), &opts).unwrap();

        assert!(mesh.validate().is_ok());
        let normals = mesh.normals.as_ref().unwrap();
        for n in normals {
            assert!((n[2] - 1.0).abs() < 0.01, "expected Z≈1.0, got {n:?}");
        }
    }

    // ── Error cases ───────────────────────────────────────────────────

    #[test]
    fn empty_stl_returns_error() {
        // Valid header, 0 triangles
        let mut data = vec![0u8; 80];
        data.extend_from_slice(&0u32.to_le_bytes());
        assert!(matches!(parse(&data), Err(StlError::EmptyMesh)));
    }

    #[test]
    fn garbage_returns_error() {
        assert!(parse(b"not a valid stl file at all").is_err());
    }

    // ── compute_smooth_normals ────────────────────────────────────────

    #[test]
    fn smooth_normals_single_triangle() {
        let positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let normals = compute_smooth_normals(&positions, &[0, 1, 2]);

        assert_eq!(normals.len(), 3);
        for n in &normals {
            assert!((n[2] - 1.0).abs() < 0.001, "expected ≈ [0,0,1], got {n:?}");
        }
    }

    #[test]
    fn smooth_normals_empty() {
        assert!(compute_smooth_normals(&[], &[]).is_empty());
    }
}
