//! GLB/glTF parser — pure `&[u8] → Result<Vec<MeshData>, GlbError>`.
//!
//! Extracts mesh geometry (positions, normals, UVs, indices) from glTF 2.0
//! binary (GLB) and text (glTF) formats via the `gltf` crate.
//!
//! Returns a `Vec<MeshData>` because glTF files can contain multiple meshes,
//! each with multiple primitives. Every primitive becomes one `MeshData`.
//!
//! The full entry point is [`parse_asset`], which additionally extracts
//! PBR materials, encoded textures, and the flattened node hierarchy into
//! an [`AssetData`]. [`parse`] / [`parse_merged`] remain geometry-only
//! wrappers.
//!
//! New code extending seed from seaview's `bin/debug_loader.rs::parse_glb_stats`
//! and `bin/timeline_debug.rs::parse_glb_frame_size`, which walked accessors
//! for stats only. This parser reads the actual buffer data into `MeshData`.

use crate::capabilities::{
    AlphaMode, AssetData, MaterialData, NodeData, SamplerData, TextureData, TextureMime,
    TextureSlot, TextureSource, TopologyKind,
};
use crate::error::GlbError;
use crate::mesh_data::MeshData;
use crate::parsers::stl::compute_smooth_normals;
use base64::Engine;

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
            let mesh_data = extract_primitive(&primitive, &buffers, true)?;
            meshes.push(mesh_data);
        }
    }

    if meshes.is_empty() {
        return Err(GlbError::EmptyFile);
    }

    Ok(meshes)
}

/// Parse a GLB/glTF file into a full [`AssetData`] — meshes, materials,
/// textures, and the flattened node hierarchy.
///
/// - One [`MeshData`] per primitive, with `material_index` bound to the
///   primitive's material.
/// - One [`MaterialData`](crate::MaterialData) per glTF material.
/// - One [`TextureData`](crate::TextureData) per glTF **image** (in image
///   index order, which is what material texture references index into).
///   Embedded images (buffer views, data URIs) keep their raw encoded
///   bytes; external file URIs are rejected — see
///   [`GlbError::ExternalReferences`].
/// - Nodes from the default scene (or the first scene), flattened
///   depth-first into [`NodeData`](crate::NodeData) with column-major
///   transforms. Multiple nodes referencing the same mesh index is
///   instancing — the sharing is preserved.
///
/// Topology: non-triangle primitive modes are mapped rather than rejected
/// — `POINTS` → [`TopologyKind::PointList`], `LINES`/`LINE_STRIP`/
/// `LINE_LOOP` → [`TopologyKind::LineList`] (strips and loops are
/// converted to pair lists), `TRIANGLE_STRIP` → [`TopologyKind::TriangleStrip`].
///
/// # Errors
///
/// Returns [`GlbError::ExternalReferences`] if the file references external
/// files (core parsers cannot touch the filesystem),
/// [`GlbError::EmptyFile`] if no meshes are found, or any of the other
/// [`GlbError`] variants on malformed data.
pub fn parse_asset(bytes: &[u8]) -> Result<AssetData, GlbError> {
    // Cheap JSON-level pass first: reject external references before
    // `import_slice` tries to resolve them (which yields opaque IO errors).
    let root = gltf::Gltf::from_slice(bytes)?;
    check_external_references(&root.document)?;

    let (document, buffers, _images) = gltf::import_slice(bytes)?;

    let textures = extract_textures(&document, &buffers)?;
    let materials = extract_materials(&document);

    let mut meshes = Vec::new();
    for mesh in document.meshes() {
        for primitive in mesh.primitives() {
            let mut mesh_data = extract_primitive(&primitive, &buffers, false)?;
            mesh_data.material_index = primitive.material().index();
            meshes.push(mesh_data);
        }
    }

    if meshes.is_empty() {
        return Err(GlbError::EmptyFile);
    }

    let mut nodes = Vec::new();
    let mut root_nodes = Vec::new();
    let scene = document
        .default_scene()
        .or_else(|| document.scenes().next());
    if let Some(scene) = scene {
        for node in scene.nodes() {
            let index = flatten_node(node, &mut nodes);
            root_nodes.push(index);
        }
    }

    Ok(AssetData {
        meshes,
        materials,
        textures,
        nodes,
        root_nodes,
        name: None,
    })
}

/// Reject glTF files that reference external files.
///
/// Core parsers are pure — they get bytes in and return data out. A `.gltf`
/// file whose buffers or images live in sibling files cannot be resolved
/// without touching the filesystem, so it is rejected with a clear error
/// instead of silently producing empty data.
fn check_external_references(document: &gltf::Document) -> Result<(), GlbError> {
    for buffer in document.buffers() {
        if let gltf::buffer::Source::Uri(uri) = buffer.source() {
            if !uri.starts_with("data:") {
                return Err(GlbError::ExternalReferences {
                    what: format!("buffer '{uri}'"),
                });
            }
        }
    }
    for image in document.images() {
        if let gltf::image::Source::Uri { uri, .. } = image.source() {
            if !uri.starts_with("data:") {
                return Err(GlbError::ExternalReferences {
                    what: format!("image '{uri}'"),
                });
            }
        }
    }
    Ok(())
}

/// Extract one [`TextureData`](crate::TextureData) per glTF image, in image
/// index order.
fn extract_textures(
    document: &gltf::Document,
    buffers: &[gltf::buffer::Data],
) -> Result<Vec<TextureData>, GlbError> {
    let mut out = Vec::new();

    for image in document.images() {
        let name = image.name().map(String::from);

        let data = match image.source() {
            gltf::image::Source::View { view, mime_type } => {
                let mime = TextureMime::parse(mime_type)
                    .ok_or_else(|| GlbError::UnsupportedImageMime(mime_type.to_string()))?;
                let buffer = &buffers[view.buffer().index()];
                let start = view.offset();
                let end = start
                    .checked_add(view.length())
                    .filter(|&end| end <= buffer.0.len())
                    .ok_or(GlbError::InvalidBufferView)?;
                let bytes = buffer.0[start..end].to_vec();
                TextureData {
                    source: TextureSource::Embedded { bytes, mime },
                    name,
                }
            }
            gltf::image::Source::Uri { uri, mime_type } => {
                let payload =
                    uri.strip_prefix("data:")
                        .ok_or_else(|| GlbError::ExternalReferences {
                            what: format!("image '{uri}'"),
                        })?;
                // data:[<mediatype>][;base64],<payload>
                let (meta, encoded) = payload
                    .split_once(',')
                    .ok_or_else(|| GlbError::DataUri(uri.to_string()))?;
                let mime_str = meta.split(';').next().unwrap_or("");
                let mime = TextureMime::parse(mime_str)
                    .or_else(|| mime_type.and_then(TextureMime::parse))
                    .ok_or_else(|| GlbError::UnsupportedImageMime(mime_str.to_string()))?;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(encoded.trim())
                    .map_err(|e| GlbError::DataUri(e.to_string()))?;
                TextureData {
                    source: TextureSource::Embedded { bytes, mime },
                    name,
                }
            }
        };

        out.push(data);
    }

    Ok(out)
}

/// Extract one [`MaterialData`](crate::MaterialData) per glTF material.
///
/// Texture references are image indices (matching the order of
/// [`extract_textures`]).
fn extract_materials(document: &gltf::Document) -> Vec<MaterialData> {
    document
        .materials()
        .map(|material| {
            let pbr = material.pbr_metallic_roughness();
            // Texture slots carry the image index (matching the order of
            // `extract_textures`, one entry per image), the UV set, and the
            // resolved sampler from the referenced texture object.
            let tex = |info: Option<gltf::texture::Info>| info.map(texture_slot);

            let base_color_texture = tex(pbr.base_color_texture());
            let normal_texture = material
                .normal_texture()
                .map(|t| texture_slot_of(t.texture(), t.tex_coord()));
            let metallic_roughness_texture = tex(pbr.metallic_roughness_texture());
            let emissive_texture = tex(material.emissive_texture());
            let has_textures = base_color_texture.is_some()
                || normal_texture.is_some()
                || metallic_roughness_texture.is_some()
                || emissive_texture.is_some();

            MaterialData {
                name: material.name().map(String::from),
                base_color: pbr.base_color_factor(),
                metallic: pbr.metallic_factor(),
                roughness: pbr.roughness_factor(),
                emissive: material.emissive_factor(),
                alpha_mode: match material.alpha_mode() {
                    gltf::material::AlphaMode::Opaque => AlphaMode::Opaque,
                    gltf::material::AlphaMode::Mask => AlphaMode::Mask,
                    gltf::material::AlphaMode::Blend => AlphaMode::Blend,
                },
                alpha_cutoff: material.alpha_cutoff().unwrap_or(0.5),
                double_sided: material.double_sided(),
                has_textures,
                base_color_texture,
                normal_texture,
                metallic_roughness_texture,
                emissive_texture,
            }
        })
        .collect()
}

/// Build a [`TextureSlot`] from a glTF texture-info struct (PBR base color,
/// metallic-roughness, emissive).
fn texture_slot(info: gltf::texture::Info<'_>) -> TextureSlot {
    texture_slot_of(info.texture(), info.tex_coord())
}

/// Build a [`TextureSlot`] from a glTF texture reference and UV set.
///
/// The sampler is resolved here: a texture object with no explicit sampler
/// per the glTF spec means the implicit default (repeat wrapping, auto
/// filtering), which [`SamplerData::default_sampler`] represents.
fn texture_slot_of(texture: gltf::texture::Texture<'_>, tex_coord: u32) -> TextureSlot {
    let sampler = texture.sampler();
    TextureSlot {
        image: texture.source().index(),
        tex_coord,
        sampler: SamplerData {
            mag_filter: sampler.mag_filter().map(|f| f.as_gl_enum()),
            min_filter: sampler.min_filter().map(|f| f.as_gl_enum()),
            wrap_s: sampler.wrap_s().as_gl_enum(),
            wrap_t: sampler.wrap_t().as_gl_enum(),
        },
    }
}

/// Recursively flatten a node and its descendants into `nodes`, returning
/// the node's index.
fn flatten_node(node: gltf::Node<'_>, nodes: &mut Vec<NodeData>) -> usize {
    let index = nodes.len();

    // `Transform::matrix()` produces a column-major [[f32; 4]; 4].
    let matrix = node.transform().matrix();
    let mut transform = [0.0f32; 16];
    for (row, values) in matrix.iter().enumerate() {
        transform[row * 4..][..4].copy_from_slice(values);
    }

    nodes.push(NodeData {
        name: node.name().map(String::from),
        transform,
        mesh: node.mesh().map(|mesh| mesh.index()),
        material: None, // glTF binds materials per-primitive, not per-node
        children: Vec::new(),
    });

    let children = node
        .children()
        .map(|child| flatten_node(child, nodes))
        .collect();
    nodes[index].children = children;

    index
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
///
/// When `strict` is `true` (the geometry-only [`parse`] path), non-triangle
/// modes are rejected. The full [`parse_asset`] path passes `false` and maps
/// every representable mode to its [`TopologyKind`].
fn extract_primitive(
    primitive: &gltf::Primitive<'_>,
    buffers: &[gltf::buffer::Data],
    strict: bool,
) -> Result<MeshData, GlbError> {
    let mode = primitive.mode();

    if strict && mode != gltf::mesh::Mode::Triangles {
        return Err(GlbError::UnsupportedTopology(format!("{mode:?}")));
    }

    // ── Topology (G1) — map instead of reject ─────────────────────────

    let topology = match mode {
        gltf::mesh::Mode::Triangles => TopologyKind::TriangleList,
        gltf::mesh::Mode::TriangleStrip => TopologyKind::TriangleStrip,
        gltf::mesh::Mode::Lines | gltf::mesh::Mode::LineStrip | gltf::mesh::Mode::LineLoop => {
            TopologyKind::LineList
        }
        gltf::mesh::Mode::Points => TopologyKind::PointList,
        other => {
            return Err(GlbError::UnsupportedTopology(format!("{other:?}")));
        }
    };

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

    let raw_indices = primitive
        .indices()
        .map(|accessor| read_index_accessor(&accessor, buffers));

    let indices = finalize_indices(topology, mode, raw_indices, positions.len());

    // ── Compute normals if missing (triangles only) ───────────────────

    let normals = match normals {
        Some(n) if n.len() == positions.len() => Some(n),
        _ if matches!(
            topology,
            TopologyKind::TriangleList | TopologyKind::TriangleStrip
        ) && !positions.is_empty()
            && indices.len() >= 3 =>
        {
            Some(compute_smooth_normals(&positions, &indices))
        }
        _ => None,
    };

    Ok(MeshData {
        topology,
        positions,
        normals,
        uvs,
        vertex_colors: None,
        indices,
        material_index: None,
    })
}

/// Produce the final index list for a primitive's topology.
///
/// Non-indexed primitives get generated indices. Line strips and loops are
/// converted to explicit pair lists; everything else passes indices through
/// as-is.
fn finalize_indices(
    topology: TopologyKind,
    mode: gltf::mesh::Mode,
    raw: Option<Vec<u32>>,
    vertex_count: usize,
) -> Vec<u32> {
    let mut indices = raw.unwrap_or_else(|| (0..vertex_count as u32).collect());

    if matches!(topology, TopologyKind::LineList) && !matches!(mode, gltf::mesh::Mode::Lines) {
        // Convert LINE_STRIP / LINE_LOOP to an explicit pair list.
        let loop_ = matches!(mode, gltf::mesh::Mode::LineLoop);
        let n = indices.len();
        let mut pairs = Vec::with_capacity(if loop_ { n * 2 } else { n.saturating_mul(2) });
        for i in 0..n.saturating_sub(1) {
            pairs.push(indices[i]);
            pairs.push(indices[i + 1]);
        }
        if loop_ && n > 1 {
            pairs.push(indices[n - 1]);
            pairs.push(indices[0]);
        }
        indices = pairs;
    }

    indices
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
        material_index: None,
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

    // ── parse_asset: materials ────────────────────────────────────────

    #[test]
    fn parses_materials() {
        let asset = parse_asset(&fixture("cube_textured.glb")).unwrap();

        assert_eq!(asset.materials.len(), 2);
        assert_eq!(asset.materials[0].name.as_deref(), Some("RedTextured"));
        assert_eq!(asset.materials[0].base_color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(asset.materials[0].metallic, 0.0);
        assert_eq!(asset.materials[0].roughness, 0.4);
        assert_eq!(asset.materials[0].emissive, [0.0, 1.0, 0.0]);

        assert_eq!(asset.materials[1].name.as_deref(), Some("BlueMasked"));
        assert_eq!(asset.materials[1].metallic, 0.5);
        assert_eq!(asset.materials[1].alpha_mode, AlphaMode::Mask);
        assert_eq!(asset.materials[1].alpha_cutoff, 0.4);
        assert!(asset.materials[1].double_sided);
        assert_eq!(asset.materials[0].alpha_mode, AlphaMode::Opaque);
    }

    #[test]
    fn parses_textures_indexed() {
        let asset = parse_asset(&fixture("cube_textured.glb")).unwrap();

        assert_eq!(asset.textures.len(), 1);
        assert_eq!(asset.textures[0].name.as_deref(), Some("red"));
        match &asset.textures[0].source {
            TextureSource::Embedded { bytes, mime } => {
                assert_eq!(*mime, TextureMime::Png);
                // PNG magic
                assert_eq!(&bytes[..4], b"\x89PNG");
            }
            other => panic!("expected embedded texture, got {other:?}"),
        }

        // Material 0 references the texture via image index 0.
        assert_eq!(
            asset.materials[0].base_color_texture.map(|slot| slot.image),
            Some(0)
        );
        assert!(asset.materials[0].has_textures);
        assert!(!asset.materials[1].has_textures);
        assert!(asset.capabilities().has_textures);
    }

    #[test]
    fn sampler_settings_captured() {
        // cube_textured.glb declares one sampler:
        // magFilter LINEAR, minFilter LINEAR_MIPMAP_LINEAR, wrap REPEAT×2.
        let asset = parse_asset(&fixture("cube_textured.glb")).unwrap();
        let slot = asset.materials[0]
            .base_color_texture
            .expect("material 0 has a base color texture");

        assert_eq!(slot.image, 0);
        assert_eq!(slot.tex_coord, 0);
        assert_eq!(
            slot.sampler,
            SamplerData {
                mag_filter: Some(crate::GL_LINEAR),
                min_filter: Some(crate::GL_LINEAR_MIPMAP_LINEAR),
                wrap_s: crate::GL_WRAP_REPEAT,
                wrap_t: crate::GL_WRAP_REPEAT,
            }
        );
    }

    #[test]
    fn default_sampler_when_texture_has_none() {
        // cube_default_sampler.glb has a texture object with no `sampler`
        // key — per the glTF spec that's the implicit default sampler:
        // repeat wrapping, auto filtering (filters undefined).
        let asset = parse_asset(&fixture("cube_default_sampler.glb")).unwrap();
        let slot = asset.materials[0]
            .base_color_texture
            .expect("material 0 has a base color texture");

        assert_eq!(slot.sampler, SamplerData::default_sampler());
        assert_eq!(slot.sampler.wrap_s, crate::GL_WRAP_REPEAT);
        assert_eq!(slot.sampler.mag_filter, None);
        assert_eq!(slot.sampler.min_filter, None);
    }

    #[test]
    fn material_index_on_primitives() {
        let asset = parse_asset(&fixture("cube_textured.glb")).unwrap();

        // Two primitives — one per material.
        assert_eq!(asset.meshes.len(), 2);
        assert_eq!(asset.meshes[0].material_index, Some(0));
        assert_eq!(asset.meshes[1].material_index, Some(1));
        assert!(asset.capabilities().has_materials);
    }

    // ── parse_asset: nodes / hierarchy ────────────────────────────

    #[test]
    fn node_hierarchy_and_transforms() {
        let asset = parse_asset(&fixture("cube_hierarchy.glb")).unwrap();

        assert_eq!(asset.nodes.len(), 2);
        assert_eq!(asset.root_nodes, vec![0]);
        assert_eq!(asset.nodes[0].name.as_deref(), Some("parent"));
        assert_eq!(asset.nodes[0].children, vec![1]);
        assert_eq!(asset.nodes[1].name.as_deref(), Some("child"));
        assert!(asset.nodes[1].children.is_empty());

        // Translation lives at elements 12–14 (column-major).
        assert_eq!(asset.nodes[0].translation(), [1.0, 2.0, 3.0]);
        assert_eq!(asset.nodes[1].translation(), [0.0, 0.0, 0.0]);
        assert_eq!(asset.nodes[1].transform, NodeData::IDENTITY);
        assert!(asset.capabilities().has_nodes);
    }

    #[test]
    fn instancing_shares_mesh_index() {
        let asset = parse_asset(&fixture("cube_hierarchy.glb")).unwrap();

        // Both nodes reference the same mesh — instancing.
        assert_eq!(asset.nodes[0].mesh, Some(0));
        assert_eq!(asset.nodes[1].mesh, Some(0));
        assert_eq!(asset.meshes.len(), 1);
    }

    // ── parse_asset: topology (G1) ──────────────────────────────────

    #[test]
    fn lines_become_linelist() {
        let asset = parse_asset(&fixture("cube_lines.glb")).unwrap();

        assert_eq!(asset.meshes.len(), 2);
        assert_eq!(asset.meshes[0].topology, TopologyKind::TriangleList);
        assert_eq!(asset.meshes[1].topology, TopologyKind::LineList);
        assert_eq!(asset.meshes[1].indices, vec![0, 1, 2, 3]);
        assert_eq!(asset.meshes[1].primitive_count(), 2);
        assert!(asset.meshes[1].validate().is_ok());
        // Line meshes get no computed normals.
        assert!(asset.meshes[1].normals.is_none());
    }

    #[test]
    fn external_gltf_uri_is_rejected() {
        let result = parse_asset(&fixture("gltf_external.gltf"));
        assert!(
            matches!(result, Err(GlbError::ExternalReferences { .. })),
            "expected ExternalReferences, got {result:?}"
        );
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
