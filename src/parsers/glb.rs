//! GLB/glTF parser — pure `&[u8] → Result<Vec<MeshData>, GlbError>`.
//!
//! Extracts mesh geometry (positions, normals, UVs, indices) from glTF 2.0
//! binary (GLB) and text (glTF) formats via the `gltf` crate.
//!
//! Returns a `Vec<MeshData>` because glTF files can contain multiple meshes,
//! each with multiple primitives. Every primitive becomes one `MeshData`.
//!
//! New code extending seed from seaview's `bin/debug_loader.rs::parse_glb_stats`
//! and `bin/timeline_debug.rs::parse_glb_frame_size`, which walked accessors
//! for stats only. This parser reads the actual buffer data into `MeshData`.

use crate::error::GlbError;
use crate::mesh_data::MeshData;
use crate::parsers::stl::compute_smooth_normals;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse a GLB/glTF file from bytes, extracting all mesh primitives.
///
/// Returns one [`MeshData`] per primitive across all meshes in the file.
/// Multi-mesh GLB files will return multiple entries.
///
/// # Errors
///
/// Returns [`GlbError::Gltf`] if the data is malformed,
/// [`GlbError::EmptyFile`] if no meshes are found, or
/// [`GlbError::MissingPositions`] if a primitive lacks position data.
pub fn parse(bytes: &[u8]) -> Result<Vec<MeshData>, GlbError> {
    let (document, buffers, _images) = gltf::import_slice(bytes)?;

    let mut meshes = Vec::new();

    for mesh in document.meshes() {
        for primitive in mesh.primitives() {
            let mesh_data = extract_primitive(&primitive, &buffers)?;
            meshes.push(mesh_data);
        }
    }

    if meshes.is_empty() {
        return Err(GlbError::EmptyFile);
    }

    Ok(meshes)
}

/// Parse a GLB/glTF file, returning exactly one [`MeshData`] with all
/// primitives merged.
///
/// This is a convenience wrapper over [`parse`] that concatenates all
/// primitives into a single mesh with offset indices. Useful when you
/// want a single unified geometry (like STL/OBJ parsers return).
pub fn parse_merged(bytes: &[u8]) -> Result<MeshData, GlbError> {
    let primitives = parse(bytes)?;

    if primitives.len() == 1 {
        return Ok(primitives.into_iter().next().unwrap());
    }

    Ok(merge_meshes(primitives))
}

// ---------------------------------------------------------------------------
// Primitive extraction
// ---------------------------------------------------------------------------

/// Extract a single glTF primitive into a [`MeshData`].
fn extract_primitive(
    primitive: &gltf::Primitive<'_>,
    buffers: &[gltf::buffer::Data],
) -> Result<MeshData, GlbError> {
    // Validate topology — we only support triangle lists.
    if primitive.mode() != gltf::mesh::Mode::Triangles {
        return Err(GlbError::UnsupportedTopology(format!(
            "{:?}",
            primitive.mode()
        )));
    }

    // ── Positions (required) ──────────────────────────────────────────

    let pos_accessor = primitive
        .get(&gltf::Semantic::Positions)
        .ok_or(GlbError::MissingPositions)?;

    let positions = read_vec3_accessor(&pos_accessor, buffers);

    if positions.is_empty() {
        return Err(GlbError::MissingPositions);
    }

    // ── Normals (optional) ────────────────────────────────────────────

    let normals = primitive
        .get(&gltf::Semantic::Normals)
        .map(|accessor| read_vec3_accessor(&accessor, buffers));

    // ── UVs (optional, first set only) ────────────────────────────────

    let uvs = primitive
        .get(&gltf::Semantic::TexCoords(0))
        .map(|accessor| read_vec2_accessor(&accessor, buffers));

    // ── Indices ───────────────────────────────────────────────────────

    let indices = match primitive.indices() {
        Some(accessor) => read_index_accessor(&accessor, buffers),
        None => {
            // No index buffer → generate sequential indices.
            (0..positions.len() as u32).collect()
        }
    };

    // ── Compute normals if missing ────────────────────────────────────

    let normals = match normals {
        Some(n) if n.len() == positions.len() => Some(n),
        _ => {
            if !positions.is_empty() && indices.len() >= 3 {
                Some(compute_smooth_normals(&positions, &indices))
            } else {
                None
            }
        }
    };

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
// Accessor reading
// ---------------------------------------------------------------------------

/// Read a VEC3 (f32×3) accessor into `Vec<[f32; 3]>`.
fn read_vec3_accessor(
    accessor: &gltf::Accessor<'_>,
    buffers: &[gltf::buffer::Data],
) -> Vec<[f32; 3]> {
    let view = match accessor.view() {
        Some(v) => v,
        None => return Vec::new(),
    };

    let buffer = &buffers[view.buffer().index()];
    let stride = view.stride().unwrap_or(std::mem::size_of::<[f32; 3]>());
    let offset = view.offset() + accessor.offset();
    let count = accessor.count();

    let mut result = Vec::with_capacity(count);
    for i in 0..count {
        let start = offset + i * stride;
        if start + 12 > buffer.0.len() {
            break;
        }
        let x = f32::from_le_bytes([
            buffer.0[start],
            buffer.0[start + 1],
            buffer.0[start + 2],
            buffer.0[start + 3],
        ]);
        let y = f32::from_le_bytes([
            buffer.0[start + 4],
            buffer.0[start + 5],
            buffer.0[start + 6],
            buffer.0[start + 7],
        ]);
        let z = f32::from_le_bytes([
            buffer.0[start + 8],
            buffer.0[start + 9],
            buffer.0[start + 10],
            buffer.0[start + 11],
        ]);
        result.push([x, y, z]);
    }
    result
}

/// Read a VEC2 (f32×2) accessor into `Vec<[f32; 2]>`.
fn read_vec2_accessor(
    accessor: &gltf::Accessor<'_>,
    buffers: &[gltf::buffer::Data],
) -> Vec<[f32; 2]> {
    let view = match accessor.view() {
        Some(v) => v,
        None => return Vec::new(),
    };

    let buffer = &buffers[view.buffer().index()];
    let stride = view.stride().unwrap_or(std::mem::size_of::<[f32; 2]>());
    let offset = view.offset() + accessor.offset();
    let count = accessor.count();

    let mut result = Vec::with_capacity(count);
    for i in 0..count {
        let start = offset + i * stride;
        if start + 8 > buffer.0.len() {
            break;
        }
        let u = f32::from_le_bytes([
            buffer.0[start],
            buffer.0[start + 1],
            buffer.0[start + 2],
            buffer.0[start + 3],
        ]);
        let v = f32::from_le_bytes([
            buffer.0[start + 4],
            buffer.0[start + 5],
            buffer.0[start + 6],
            buffer.0[start + 7],
        ]);
        result.push([u, v]);
    }
    result
}

/// Read an index accessor into `Vec<u32>`, handling u8/u16/u32 component types.
fn read_index_accessor(accessor: &gltf::Accessor<'_>, buffers: &[gltf::buffer::Data]) -> Vec<u32> {
    let view = match accessor.view() {
        Some(v) => v,
        None => return Vec::new(),
    };

    let buffer = &buffers[view.buffer().index()];
    let offset = view.offset() + accessor.offset();
    let count = accessor.count();

    match accessor.data_type() {
        gltf::accessor::DataType::U8 => {
            let stride = view.stride().unwrap_or(1);
            let mut result = Vec::with_capacity(count);
            for i in 0..count {
                let start = offset + i * stride;
                if start >= buffer.0.len() {
                    break;
                }
                result.push(buffer.0[start] as u32);
            }
            result
        }
        gltf::accessor::DataType::U16 => {
            let stride = view.stride().unwrap_or(2);
            let mut result = Vec::with_capacity(count);
            for i in 0..count {
                let start = offset + i * stride;
                if start + 2 > buffer.0.len() {
                    break;
                }
                let val = u16::from_le_bytes([buffer.0[start], buffer.0[start + 1]]);
                result.push(val as u32);
            }
            result
        }
        gltf::accessor::DataType::U32 => {
            let stride = view.stride().unwrap_or(4);
            let mut result = Vec::with_capacity(count);
            for i in 0..count {
                let start = offset + i * stride;
                if start + 4 > buffer.0.len() {
                    break;
                }
                let val = u32::from_le_bytes([
                    buffer.0[start],
                    buffer.0[start + 1],
                    buffer.0[start + 2],
                    buffer.0[start + 3],
                ]);
                result.push(val);
            }
            result
        }
        _ => Vec::new(), // Unsupported index type — return empty.
    }
}

// ---------------------------------------------------------------------------
// Merge helper
// ---------------------------------------------------------------------------

/// Merge multiple [`MeshData`] into one, offsetting indices.
fn merge_meshes(meshes: Vec<MeshData>) -> MeshData {
    let total_verts: usize = meshes.iter().map(|m| m.positions.len()).sum();
    let total_indices: usize = meshes.iter().map(|m| m.indices.len()).sum();

    let any_normals = meshes.iter().any(|m| m.normals.is_some());
    let any_uvs = meshes.iter().any(|m| m.uvs.is_some());
    let any_colors = meshes.iter().any(|m| m.vertex_colors.is_some());

    let mut positions = Vec::with_capacity(total_verts);
    let mut normals = if any_normals {
        Some(Vec::with_capacity(total_verts))
    } else {
        None
    };
    let mut uvs = if any_uvs {
        Some(Vec::with_capacity(total_verts))
    } else {
        None
    };
    let mut vertex_colors = if any_colors {
        Some(Vec::with_capacity(total_verts))
    } else {
        None
    };
    let mut indices = Vec::with_capacity(total_indices);

    let mut vertex_offset: u32 = 0;

    for mesh in meshes {
        let n_verts = mesh.positions.len();

        positions.extend_from_slice(&mesh.positions);

        if let Some(ref mut out_normals) = normals {
            match &mesh.normals {
                Some(n) => out_normals.extend_from_slice(n),
                None => out_normals.extend(std::iter::repeat([0.0, 1.0, 0.0]).take(n_verts)),
            }
        }

        if let Some(ref mut out_uvs) = uvs {
            match &mesh.uvs {
                Some(u) => out_uvs.extend_from_slice(u),
                None => out_uvs.extend(std::iter::repeat([0.0, 0.0]).take(n_verts)),
            }
        }

        if let Some(ref mut out_colors) = vertex_colors {
            match &mesh.vertex_colors {
                Some(c) => out_colors.extend_from_slice(c),
                None => out_colors.extend(std::iter::repeat([1.0, 1.0, 1.0, 1.0]).take(n_verts)),
            }
        }

        for &idx in &mesh.indices {
            indices.push(idx + vertex_offset);
        }

        vertex_offset += n_verts as u32;
    }

    MeshData {
        topology: crate::capabilities::TopologyKind::TriangleList,
        positions,
        normals,
        uvs,
        vertex_colors,
        indices,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::fixture;

    /// Minimal valid GLB with no meshes, for error testing.
    fn empty_glb() -> Vec<u8> {
        let json_str = r#"{"asset":{"version":"2.0"}}"#;
        let mut json_bytes = json_str.as_bytes().to_vec();
        while json_bytes.len() % 4 != 0 {
            json_bytes.push(b' ');
        }
        let json_chunk_len = json_bytes.len() as u32;
        let total_length = 12 + 8 + json_chunk_len;

        let mut glb = Vec::new();
        glb.extend_from_slice(b"glTF");
        glb.extend_from_slice(&2u32.to_le_bytes());
        glb.extend_from_slice(&total_length.to_le_bytes());
        glb.extend_from_slice(&json_chunk_len.to_le_bytes());
        glb.extend_from_slice(&0x4E4F534Au32.to_le_bytes());
        glb.extend_from_slice(&json_bytes);
        glb
    }

    // ── Cube fixture: 24 verts, 36 indices, 12 triangles ─────────────

    #[test]
    fn parse_cube() {
        let meshes = parse(&fixture("cube.glb")).unwrap();

        assert_eq!(meshes.len(), 1);
        let mesh = &meshes[0];
        assert!(mesh.validate().is_ok());
        assert_eq!(mesh.vertex_count(), 24);
        assert_eq!(mesh.triangle_count(), 12);
        assert!(mesh.has_normals());
        assert!(!mesh.has_uvs());
    }

    #[test]
    fn cube_positions_bounded() {
        let meshes = parse(&fixture("cube.glb")).unwrap();
        let mesh = &meshes[0];

        for pos in &mesh.positions {
            for &v in pos {
                assert!(
                    v.abs() <= 0.5 + f32::EPSILON,
                    "position out of range: {pos:?}"
                );
            }
        }
    }

    #[test]
    fn cube_normals_are_unit_length() {
        let meshes = parse(&fixture("cube.glb")).unwrap();
        let normals = meshes[0].normals.as_ref().unwrap();

        for n in normals {
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            assert!((len - 1.0).abs() < 0.01, "normal not unit length: {n:?}");
        }
    }

    // ── Merged parse ──────────────────────────────────────────────────

    #[test]
    fn parse_merged_cube() {
        let mesh = parse_merged(&fixture("cube.glb")).unwrap();

        assert!(mesh.validate().is_ok());
        assert_eq!(mesh.vertex_count(), 24);
        assert_eq!(mesh.triangle_count(), 12);
    }

    // ── Merge helper (unit) ───────────────────────────────────────────

    #[test]
    fn merge_preserves_normals() {
        let m1 = MeshData::new(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 2],
        )
        .with_normals(vec![[0.0, 0.0, 1.0]; 3]);
        let m2 = MeshData::new(
            vec![[2.0, 0.0, 0.0], [3.0, 0.0, 0.0], [2.0, 1.0, 0.0]],
            vec![0, 1, 2],
        )
        .with_normals(vec![[0.0, 0.0, -1.0]; 3]);

        let merged = merge_meshes(vec![m1, m2]);
        assert!(merged.validate().is_ok());
        assert_eq!(merged.vertex_count(), 6);

        let normals = merged.normals.as_ref().unwrap();
        assert_eq!(normals[0], [0.0, 0.0, 1.0]);
        assert_eq!(normals[3], [0.0, 0.0, -1.0]);
    }

    #[test]
    fn merge_fills_missing_normals() {
        let m1 =
            MeshData::new(vec![[0.0; 3]; 3], vec![0, 1, 2]).with_normals(vec![[0.0, 0.0, 1.0]; 3]);
        let m2 = MeshData::new(vec![[1.0; 3]; 3], vec![0, 1, 2]);

        let merged = merge_meshes(vec![m1, m2]);
        let normals = merged.normals.as_ref().unwrap();
        assert_eq!(normals.len(), 6);
        assert_eq!(normals[3], [0.0, 1.0, 0.0]); // default fill
    }

    // ── Error cases ───────────────────────────────────────────────────

    #[test]
    fn empty_glb_returns_error() {
        assert!(matches!(parse(&empty_glb()), Err(GlbError::EmptyFile)));
    }

    #[test]
    fn garbage_returns_error() {
        assert!(parse(b"this is not a glb file").is_err());
    }
}
