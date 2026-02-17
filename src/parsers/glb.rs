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
        positions,
        normals,
        uvs,
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

        for &idx in &mesh.indices {
            indices.push(idx + vertex_offset);
        }

        vertex_offset += n_verts as u32;
    }

    MeshData {
        positions,
        normals,
        uvs,
        indices,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid GLB (glTF 2.0 binary) containing one triangle.
    ///
    /// GLB layout:
    /// - 12-byte header: magic, version, total length
    /// - JSON chunk: minimal glTF JSON
    /// - BIN chunk: interleaved position + index data
    fn make_minimal_glb() -> Vec<u8> {
        // ── Buffer data (BIN chunk payload) ───────────────────────────
        // 3 positions: VEC3 float × 3 = 36 bytes
        // 3 indices: u16 × 3 = 6 bytes, padded to 8 for alignment
        let mut bin_data: Vec<u8> = Vec::new();

        // Position 0: [0, 0, 0]
        bin_data.extend_from_slice(&0.0f32.to_le_bytes());
        bin_data.extend_from_slice(&0.0f32.to_le_bytes());
        bin_data.extend_from_slice(&0.0f32.to_le_bytes());
        // Position 1: [1, 0, 0]
        bin_data.extend_from_slice(&1.0f32.to_le_bytes());
        bin_data.extend_from_slice(&0.0f32.to_le_bytes());
        bin_data.extend_from_slice(&0.0f32.to_le_bytes());
        // Position 2: [0, 1, 0]
        bin_data.extend_from_slice(&0.0f32.to_le_bytes());
        bin_data.extend_from_slice(&1.0f32.to_le_bytes());
        bin_data.extend_from_slice(&0.0f32.to_le_bytes());

        // Indices: [0, 1, 2] as u16
        bin_data.extend_from_slice(&0u16.to_le_bytes());
        bin_data.extend_from_slice(&1u16.to_le_bytes());
        bin_data.extend_from_slice(&2u16.to_le_bytes());

        // Pad to 4-byte alignment
        while bin_data.len() % 4 != 0 {
            bin_data.push(0);
        }

        let bin_len = bin_data.len() as u32; // 44

        // ── JSON chunk ────────────────────────────────────────────────
        let json_str = format!(
            r#"{{
  "asset": {{ "version": "2.0" }},
  "buffers": [{{ "byteLength": {bin_len} }}],
  "bufferViews": [
    {{ "buffer": 0, "byteOffset": 0, "byteLength": 36, "target": 34962 }},
    {{ "buffer": 0, "byteOffset": 36, "byteLength": 6, "target": 34963 }}
  ],
  "accessors": [
    {{
      "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
      "max": [1.0, 1.0, 0.0], "min": [0.0, 0.0, 0.0]
    }},
    {{
      "bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR"
    }}
  ],
  "meshes": [{{
    "primitives": [{{
      "attributes": {{ "POSITION": 0 }},
      "indices": 1,
      "mode": 4
    }}]
  }}],
  "nodes": [{{ "mesh": 0 }}],
  "scenes": [{{ "nodes": [0] }}],
  "scene": 0
}}"#
        );

        let mut json_bytes = json_str.into_bytes();
        // Pad JSON to 4-byte alignment with spaces (per GLB spec).
        while json_bytes.len() % 4 != 0 {
            json_bytes.push(b' ');
        }

        let json_chunk_len = json_bytes.len() as u32;

        // ── Assemble GLB ──────────────────────────────────────────────
        let total_length = 12 // GLB header
            + 8 + json_chunk_len  // JSON chunk header + data
            + 8 + bin_len; // BIN chunk header + data

        let mut glb = Vec::with_capacity(total_length as usize);

        // GLB header
        glb.extend_from_slice(b"glTF"); // magic
        glb.extend_from_slice(&2u32.to_le_bytes()); // version
        glb.extend_from_slice(&total_length.to_le_bytes()); // total length

        // JSON chunk
        glb.extend_from_slice(&json_chunk_len.to_le_bytes()); // chunk length
        glb.extend_from_slice(&0x4E4F534Au32.to_le_bytes()); // chunk type: JSON
        glb.extend_from_slice(&json_bytes);

        // BIN chunk
        glb.extend_from_slice(&bin_len.to_le_bytes()); // chunk length
        glb.extend_from_slice(&0x004E4942u32.to_le_bytes()); // chunk type: BIN
        glb.extend_from_slice(&bin_data);

        glb
    }

    /// Build a GLB with two mesh primitives.
    fn make_two_primitive_glb() -> Vec<u8> {
        // Two triangles in the same buffer, separate primitives.
        let mut bin_data: Vec<u8> = Vec::new();

        // Primitive 0 positions (3 verts)
        for &v in &[0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            bin_data.extend_from_slice(&v.to_le_bytes());
        }
        // Primitive 0 indices
        for &i in &[0u16, 1, 2] {
            bin_data.extend_from_slice(&i.to_le_bytes());
        }
        // Pad to 4-byte alignment
        while bin_data.len() % 4 != 0 {
            bin_data.push(0);
        }

        let prim1_idx_offset = bin_data.len();

        // Primitive 1 positions (3 verts)
        for &v in &[2.0f32, 0.0, 0.0, 3.0, 0.0, 0.0, 2.0, 1.0, 0.0] {
            bin_data.extend_from_slice(&v.to_le_bytes());
        }
        // Primitive 1 indices
        for &i in &[0u16, 1, 2] {
            bin_data.extend_from_slice(&i.to_le_bytes());
        }
        while bin_data.len() % 4 != 0 {
            bin_data.push(0);
        }

        let bin_len = bin_data.len() as u32;

        let json_str = format!(
            r#"{{
  "asset": {{ "version": "2.0" }},
  "buffers": [{{ "byteLength": {bin_len} }}],
  "bufferViews": [
    {{ "buffer": 0, "byteOffset": 0, "byteLength": 36, "target": 34962 }},
    {{ "buffer": 0, "byteOffset": 36, "byteLength": 6, "target": 34963 }},
    {{ "buffer": 0, "byteOffset": {prim1_idx_offset}, "byteLength": 36, "target": 34962 }},
    {{ "buffer": 0, "byteOffset": {prim1_idx_end}, "byteLength": 6, "target": 34963 }}
  ],
  "accessors": [
    {{
      "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
      "max": [1.0, 1.0, 0.0], "min": [0.0, 0.0, 0.0]
    }},
    {{
      "bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR"
    }},
    {{
      "bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC3",
      "max": [3.0, 1.0, 0.0], "min": [2.0, 0.0, 0.0]
    }},
    {{
      "bufferView": 3, "componentType": 5123, "count": 3, "type": "SCALAR"
    }}
  ],
  "meshes": [{{
    "primitives": [
      {{ "attributes": {{ "POSITION": 0 }}, "indices": 1, "mode": 4 }},
      {{ "attributes": {{ "POSITION": 2 }}, "indices": 3, "mode": 4 }}
    ]
  }}],
  "nodes": [{{ "mesh": 0 }}],
  "scenes": [{{ "nodes": [0] }}],
  "scene": 0
}}"#,
            prim1_idx_end = prim1_idx_offset + 36,
        );

        let mut json_bytes = json_str.into_bytes();
        while json_bytes.len() % 4 != 0 {
            json_bytes.push(b' ');
        }

        let json_chunk_len = json_bytes.len() as u32;
        let total_length = 12 + 8 + json_chunk_len + 8 + bin_len;

        let mut glb = Vec::with_capacity(total_length as usize);

        glb.extend_from_slice(b"glTF");
        glb.extend_from_slice(&2u32.to_le_bytes());
        glb.extend_from_slice(&total_length.to_le_bytes());

        glb.extend_from_slice(&json_chunk_len.to_le_bytes());
        glb.extend_from_slice(&0x4E4F534Au32.to_le_bytes());
        glb.extend_from_slice(&json_bytes);

        glb.extend_from_slice(&bin_len.to_le_bytes());
        glb.extend_from_slice(&0x004E4942u32.to_le_bytes());
        glb.extend_from_slice(&bin_data);

        glb
    }

    // ── Basic parsing ─────────────────────────────────────────────────

    #[test]
    fn parse_minimal_glb_single_triangle() {
        let glb = make_minimal_glb();
        let meshes = parse(&glb).unwrap();

        assert_eq!(meshes.len(), 1);
        let mesh = &meshes[0];
        assert!(mesh.validate().is_ok());
        assert_eq!(mesh.vertex_count(), 3);
        assert_eq!(mesh.triangle_count(), 1);
        // No normals in the GLB → computed normals
        assert!(mesh.has_normals());
        assert!(!mesh.has_uvs());
    }

    #[test]
    fn parse_minimal_glb_positions_correct() {
        let glb = make_minimal_glb();
        let meshes = parse(&glb).unwrap();
        let mesh = &meshes[0];

        assert_eq!(mesh.positions[0], [0.0, 0.0, 0.0]);
        assert_eq!(mesh.positions[1], [1.0, 0.0, 0.0]);
        assert_eq!(mesh.positions[2], [0.0, 1.0, 0.0]);
    }

    #[test]
    fn parse_minimal_glb_indices_correct() {
        let glb = make_minimal_glb();
        let meshes = parse(&glb).unwrap();
        let mesh = &meshes[0];

        assert_eq!(mesh.indices, vec![0, 1, 2]);
    }

    // ── Multi-primitive ───────────────────────────────────────────────

    #[test]
    fn parse_two_primitives() {
        let glb = make_two_primitive_glb();
        let meshes = parse(&glb).unwrap();

        assert_eq!(meshes.len(), 2);
        for mesh in &meshes {
            assert!(mesh.validate().is_ok());
            assert_eq!(mesh.vertex_count(), 3);
            assert_eq!(mesh.triangle_count(), 1);
        }
    }

    // ── Merged parse ──────────────────────────────────────────────────

    #[test]
    fn parse_merged_single() {
        let glb = make_minimal_glb();
        let mesh = parse_merged(&glb).unwrap();

        assert!(mesh.validate().is_ok());
        assert_eq!(mesh.vertex_count(), 3);
        assert_eq!(mesh.triangle_count(), 1);
    }

    #[test]
    fn parse_merged_two_primitives() {
        let glb = make_two_primitive_glb();
        let mesh = parse_merged(&glb).unwrap();

        assert!(mesh.validate().is_ok());
        assert_eq!(mesh.vertex_count(), 6);
        assert_eq!(mesh.triangle_count(), 2);
        // Second triangle indices should be offset by 3
        assert_eq!(mesh.indices[3], 3);
        assert_eq!(mesh.indices[4], 4);
        assert_eq!(mesh.indices[5], 5);
    }

    // ── Merge helper ──────────────────────────────────────────────────

    #[test]
    fn merge_preserves_normals() {
        let m1 = MeshData {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            normals: Some(vec![[0.0, 0.0, 1.0]; 3]),
            uvs: None,
            indices: vec![0, 1, 2],
        };
        let m2 = MeshData {
            positions: vec![[2.0, 0.0, 0.0], [3.0, 0.0, 0.0], [2.0, 1.0, 0.0]],
            normals: Some(vec![[0.0, 0.0, -1.0]; 3]),
            uvs: None,
            indices: vec![0, 1, 2],
        };

        let merged = merge_meshes(vec![m1, m2]);
        assert!(merged.validate().is_ok());
        assert_eq!(merged.vertex_count(), 6);

        let normals = merged.normals.as_ref().unwrap();
        assert_eq!(normals[0], [0.0, 0.0, 1.0]);
        assert_eq!(normals[3], [0.0, 0.0, -1.0]);
    }

    #[test]
    fn merge_fills_missing_normals() {
        let m1 = MeshData {
            positions: vec![[0.0; 3]; 3],
            normals: Some(vec![[0.0, 0.0, 1.0]; 3]),
            uvs: None,
            indices: vec![0, 1, 2],
        };
        let m2 = MeshData {
            positions: vec![[1.0; 3]; 3],
            normals: None, // missing
            uvs: None,
            indices: vec![0, 1, 2],
        };

        let merged = merge_meshes(vec![m1, m2]);
        let normals = merged.normals.as_ref().unwrap();
        assert_eq!(normals.len(), 6);
        // Filled normals default to [0, 1, 0]
        assert_eq!(normals[3], [0.0, 1.0, 0.0]);
    }

    // ── Error cases ───────────────────────────────────────────────────

    #[test]
    fn empty_glb_returns_error() {
        // Valid GLB header but no meshes
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

        let result = parse(&glb);
        assert!(matches!(result, Err(GlbError::EmptyFile)));
    }

    #[test]
    fn garbage_bytes_returns_error() {
        let result = parse(b"this is not a glb file");
        assert!(result.is_err());
    }

    // ── Computed normals ──────────────────────────────────────────────

    #[test]
    fn computed_normals_are_reasonable() {
        let glb = make_minimal_glb();
        let meshes = parse(&glb).unwrap();
        let mesh = &meshes[0];

        let normals = mesh.normals.as_ref().unwrap();
        // Triangle in XY plane → normal should be ≈ [0, 0, 1]
        for n in normals {
            assert!((n[2] - 1.0).abs() < 0.01, "expected Z ≈ 1.0, got {:?}", n);
        }
    }
}
