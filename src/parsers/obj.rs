//! OBJ parser — pure `&[u8] → Result<MeshData, ObjError>`.
//!
//! Supports OBJ files with optional normals and texture coordinates via the
//! `tobj` crate. Produces indexed [`MeshData`] with positions, normals, and
//! optionally UVs.
//!
//! Multi-object OBJ files are merged into a single [`MeshData`] with index
//! offsets applied. MTL files are ignored — this is geometry only.
//!
//! Extracted from seaview's `asset_loaders/obj_loader.rs`, with all Bevy
//! types replaced by [`MeshData`].

use crate::error::ObjError;
use crate::mesh_data::MeshData;
use crate::parsers::stl::compute_smooth_normals;
use std::io::Cursor;

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

/// Options for OBJ parsing.
#[derive(Debug, Clone)]
pub struct ObjOptions {
    /// If `true`, recompute smooth vertex normals from geometry instead of
    /// using normals from the OBJ file (if present).
    ///
    /// Defaults to `false` (use OBJ normals when available).
    pub recompute_normals: bool,

    /// If `true`, triangulate non-triangle faces (quads, n-gons).
    ///
    /// Defaults to `true` since downstream consumers expect triangle lists.
    pub triangulate: bool,
}

impl Default for ObjOptions {
    fn default() -> Self {
        Self {
            recompute_normals: false,
            triangulate: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse an OBJ file from bytes using default options.
///
/// Merges all objects/groups into a single [`MeshData`]. MTL files are ignored.
///
/// # Errors
///
/// Returns [`ObjError::Parse`] if the data is malformed,
/// [`ObjError::EmptyFile`] if there are no models, or
/// [`ObjError::EmptyMesh`] if all models have zero vertices.
pub fn parse(bytes: &[u8]) -> Result<MeshData, ObjError> {
    parse_with_options(bytes, &ObjOptions::default())
}

/// Parse an OBJ file from bytes with explicit options.
///
/// See [`ObjOptions`] for available settings.
pub fn parse_with_options(bytes: &[u8], options: &ObjOptions) -> Result<MeshData, ObjError> {
    let mut cursor = Cursor::new(bytes);

    let load_options = tobj::LoadOptions {
        triangulate: options.triangulate,
        single_index: true, // single index for all attributes (required for MeshData)
        ..Default::default()
    };

    // Load without MTL file — we produce geometry only.
    let (models, _materials) = tobj::load_obj_buf(&mut cursor, &load_options, |_mtl_path| {
        tobj::MTLLoadResult::Ok((Vec::new(), Default::default()))
    })?;

    if models.is_empty() {
        return Err(ObjError::EmptyFile);
    }

    build_merged_mesh(&models, options.recompute_normals)
}

// ---------------------------------------------------------------------------
// Internal builder
// ---------------------------------------------------------------------------

/// Build a single [`MeshData`] by merging all models from the OBJ file.
///
/// Each model's vertices are appended, with indices offset accordingly.
fn build_merged_mesh(
    models: &[tobj::Model],
    recompute_normals: bool,
) -> Result<MeshData, ObjError> {
    // First pass: count totals.
    let total_vertices: usize = models.iter().map(|m| m.mesh.positions.len() / 3).sum();
    let total_indices: usize = models.iter().map(|m| m.mesh.indices.len()).sum();

    if total_vertices == 0 {
        return Err(ObjError::EmptyMesh);
    }

    // Check if all models carry normals and/or UVs.
    let has_normals = !recompute_normals
        && models
            .iter()
            .all(|m| m.mesh.normals.len() == m.mesh.positions.len());
    let has_uvs = models
        .iter()
        .all(|m| m.mesh.texcoords.len() / 2 == m.mesh.positions.len() / 3);

    // Allocate output buffers.
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(total_vertices);
    let mut normals_buf: Vec<[f32; 3]> =
        Vec::with_capacity(if has_normals { total_vertices } else { 0 });
    let mut uvs_buf: Vec<[f32; 2]> = Vec::with_capacity(if has_uvs { total_vertices } else { 0 });
    let mut indices: Vec<u32> = Vec::with_capacity(total_indices);

    let mut vertex_offset: u32 = 0;

    for model in models {
        let mesh = &model.mesh;
        let num_vertices = mesh.positions.len() / 3;

        // Copy positions.
        for i in 0..num_vertices {
            positions.push([
                mesh.positions[i * 3],
                mesh.positions[i * 3 + 1],
                mesh.positions[i * 3 + 2],
            ]);
        }

        // Copy normals (if present and not recomputing).
        if has_normals {
            for i in 0..num_vertices {
                normals_buf.push([
                    mesh.normals[i * 3],
                    mesh.normals[i * 3 + 1],
                    mesh.normals[i * 3 + 2],
                ]);
            }
        }

        // Copy UVs (if present in all models).
        if has_uvs {
            for i in 0..num_vertices {
                uvs_buf.push([mesh.texcoords[i * 2], mesh.texcoords[i * 2 + 1]]);
            }
        }

        // Copy indices with offset.
        for &idx in &mesh.indices {
            indices.push(idx + vertex_offset);
        }

        vertex_offset += num_vertices as u32;
    }

    // Resolve normals.
    let normals = if has_normals {
        Some(normals_buf)
    } else if !recompute_normals && positions.len() >= 3 && indices.len() >= 3 {
        // OBJ didn't have normals — compute them.
        Some(compute_smooth_normals(&positions, &indices))
    } else if recompute_normals {
        Some(compute_smooth_normals(&positions, &indices))
    } else {
        None
    };

    let uvs = if has_uvs { Some(uvs_buf) } else { None };

    Ok(MeshData {
        topology: crate::capabilities::TopologyKind::TriangleList,
        positions,
        normals,
        uvs,
        vertex_colors: None,
        indices,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::fixture;

    // ── Basic parsing ─────────────────────────────────────────────────

    #[test]
    fn parse_triangle() {
        let mesh = parse(&fixture("triangle.obj")).unwrap();

        assert!(mesh.validate().is_ok());
        assert_eq!(mesh.vertex_count(), 3);
        assert_eq!(mesh.triangle_count(), 1);
        assert!(mesh.has_normals()); // computed — OBJ has no normals
        assert!(!mesh.has_uvs());
    }

    #[test]
    fn parse_with_normals() {
        let mesh = parse(&fixture("triangle_normals.obj")).unwrap();

        assert!(mesh.validate().is_ok());
        assert_eq!(mesh.vertex_count(), 3);
        assert!(mesh.has_normals());

        let normals = mesh.normals.as_ref().unwrap();
        for n in normals {
            assert!((n[2] - 1.0).abs() < 0.01, "expected Z≈1.0, got {n:?}");
        }
    }

    #[test]
    fn parse_with_normals_and_uvs() {
        let mesh = parse(&fixture("triangle_full.obj")).unwrap();

        assert!(mesh.validate().is_ok());
        assert!(mesh.has_normals());
        assert!(mesh.has_uvs());
        assert_eq!(mesh.uvs.as_ref().unwrap().len(), 3);
    }

    // ── Multi-object merge ────────────────────────────────────────────

    #[test]
    fn parse_two_objects_merged() {
        let mesh = parse(&fixture("two_objects.obj")).unwrap();

        assert!(mesh.validate().is_ok());
        assert_eq!(mesh.vertex_count(), 6);
        assert_eq!(mesh.triangle_count(), 2);
    }

    // ── Quad triangulation ────────────────────────────────────────────

    #[test]
    fn parse_quad_triangulated() {
        let mesh = parse(&fixture("quad.obj")).unwrap();

        assert!(mesh.validate().is_ok());
        assert_eq!(mesh.vertex_count(), 4);
        assert_eq!(mesh.triangle_count(), 2);
    }

    // ── Options ───────────────────────────────────────────────────────

    #[test]
    fn recompute_normals() {
        let opts = ObjOptions {
            recompute_normals: true,
            ..Default::default()
        };
        let mesh = parse_with_options(&fixture("triangle_normals.obj"), &opts).unwrap();

        assert!(mesh.validate().is_ok());
        let normals = mesh.normals.as_ref().unwrap();
        for n in normals {
            assert!((n[2] - 1.0).abs() < 0.01, "expected Z≈1.0, got {n:?}");
        }
    }

    // ── Error cases ───────────────────────────────────────────────────

    #[test]
    fn empty_obj_returns_error() {
        let result = parse(b"# empty OBJ file\n");
        assert!(
            matches!(result, Err(ObjError::EmptyFile | ObjError::EmptyMesh)),
            "expected EmptyFile or EmptyMesh, got {result:?}",
        );
    }

    #[test]
    fn vertices_only_no_faces() {
        assert!(parse(b"v 0 0 0\nv 1 0 0\nv 0 1 0\n").is_err());
    }

    // ── Memory estimation ─────────────────────────────────────────────

    #[test]
    fn memory_estimate_reasonable() {
        let mesh = parse(&fixture("triangle.obj")).unwrap();
        let mem = mesh.estimated_memory_bytes();
        assert!(mem > 0);
        assert!(mem < 1024);
    }
}
