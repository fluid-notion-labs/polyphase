//! OBJ parser — pure `&[u8] → Result<MeshData, ObjError>`.
//!
//! Supports OBJ files with optional normals and texture coordinates via the
//! `tobj` crate. Produces indexed [`MeshData`] with positions, normals, and
//! optionally UVs.
//!
//! Two entry points:
//!
//! - [`parse`] / [`parse_with_options`] — geometry only, all objects merged
//!   into one [`MeshData`] (the historical behavior).
//! - [`parse_asset`] — one [`MeshData`] per OBJ object, plus materials and
//!   texture references parsed from an in-memory MTL companion (`ObjOptions::mtl`).
//!   Core never touches the filesystem: MTL bytes must be supplied by the
//!   caller, and texture references stay as [`TextureSource::Uri`] paths.
//!
//! Extracted from seaview's `asset_loaders/obj_loader.rs`, with all Bevy
//! types replaced by [`MeshData`].

use crate::capabilities::{
    AlphaMode, AssetData, MaterialData, TextureData, TextureSlot, TextureSource,
};
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

    /// Contents of a companion MTL file, parsed for materials and texture
    /// references by [`parse_asset`].
    ///
    /// Core is pure — it never resolves the OBJ's `mtllib` path itself. The
    /// caller is responsible for locating and reading the MTL file (or not).
    /// Only used by [`parse_asset`]; geometry-only parsing ignores it.
    pub mtl: Option<Vec<u8>>,
}

// Manual impl: `Default` derive would leave `triangulate: false`, but
// downstream consumers expect triangle lists.
impl Default for ObjOptions {
    fn default() -> Self {
        Self {
            recompute_normals: false,
            triangulate: true,
            mtl: None,
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
    parse_asset(bytes, options).and_then(merge_asset_meshes)
}

/// Parse an OBJ file into a full [`AssetData`] — one [`MeshData`] per OBJ
/// object, plus materials and texture references from the in-memory MTL
/// companion ([`ObjOptions::mtl`]).
///
/// Mappings:
///
/// - `Kd` → `base_color` RGB (passed through as-is; MTL colors are usually
///   sRGB — conversion is the consumer's job).
/// - `d` (dissolve, tobj normalizes `Tr` to it) → `base_color` alpha.
///   Absent → opaque (1.0).
/// - `map_Kd` → [`TextureSource::Uri`] texture; the material's
///   `base_color_texture` is a [`TextureSlot`] pointing at it with the
///   default sampler (MTL has no sampler concept). Core never resolves the
///   URI.
///
/// [`MeshData::material_index`] is bound per object via `usemtl`.
pub fn parse_asset(bytes: &[u8], options: &ObjOptions) -> Result<AssetData, ObjError> {
    let mut cursor = Cursor::new(bytes);

    let load_options = tobj::LoadOptions {
        triangulate: options.triangulate,
        single_index: true, // single index for all attributes (required for MeshData)
        ..Default::default()
    };

    let mtl_bytes = options.mtl.as_deref();
    let (models, materials) = tobj::load_obj_buf(&mut cursor, &load_options, |_mtl_path| {
        // Core purity: we never read the mllib path from disk — only the
        // MTL bytes the caller supplied, regardless of which mtllib was named.
        match mtl_bytes {
            Some(bytes) => tobj::load_mtl_buf(&mut Cursor::new(bytes)),
            None => tobj::MTLLoadResult::Ok((Vec::new(), Default::default())),
        }
    })?;
    // In tobj 4.0, MTL errors surface separately from OBJ errors.
    let materials = materials?;

    if models.is_empty() {
        return Err(ObjError::EmptyFile);
    }

    let mut textures = Vec::new();
    let mut material_data = Vec::with_capacity(materials.len());
    for material in &materials {
        let mut base_color = material
            .diffuse
            .map(|kd| [kd[0], kd[1], kd[2], 1.0])
            .unwrap_or([1.0, 1.0, 1.0, 1.0]);
        if let Some(dissolve) = material.dissolve {
            base_color[3] = dissolve;
        }

        let base_color_texture = material.diffuse_texture.as_ref().map(|path| {
            // De-duplication is not attempted — one texture entry per
            // material reference keeps indices trivial to compute.
            let image = textures.len();
            textures.push(TextureData {
                source: TextureSource::Uri(path.clone()),
                name: Some(material.name.clone()),
            });
            TextureSlot {
                image,
                ..Default::default()
            }
        });

        material_data.push(MaterialData {
            name: Some(material.name.clone()),
            base_color,
            alpha_mode: if base_color[3] < 1.0 {
                AlphaMode::Blend
            } else {
                AlphaMode::Opaque
            },
            has_textures: base_color_texture.is_some(),
            base_color_texture,
            ..Default::default()
        });
    }

    let mut meshes = Vec::new();
    for model in &models {
        if model.mesh.positions.is_empty() {
            continue;
        }
        let mut mesh = build_merged_mesh(std::slice::from_ref(model), options.recompute_normals)?;
        mesh.material_index = model.mesh.material_id;
        meshes.push(mesh);
    }

    if meshes.is_empty() {
        return Err(ObjError::EmptyMesh);
    }

    Ok(AssetData {
        meshes,
        materials: material_data,
        textures,
        ..Default::default()
    })
}

/// Merge an asset's meshes into one, for the geometry-only entry points.
fn merge_asset_meshes(asset: AssetData) -> Result<MeshData, ObjError> {
    let mut meshes = asset.meshes.into_iter();
    let first = meshes.next().ok_or(ObjError::EmptyMesh)?;
    Ok(meshes.fold(first, merge_two_meshes))
}

/// Merge two [`MeshData`], offsetting the second's indices.
fn merge_two_meshes(mut a: MeshData, b: MeshData) -> MeshData {
    let offset = a.positions.len() as u32;
    let has_normals = a.normals.is_some() || b.normals.is_some();
    let has_uvs = a.uvs.is_some() || b.uvs.is_some();
    let has_colors = a.vertex_colors.is_some() || b.vertex_colors.is_some();

    if has_normals {
        let fill = b
            .normals
            .unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; b.positions.len()]);
        a.normals
            .get_or_insert_with(|| vec![[0.0, 1.0, 0.0]; a.positions.len()])
            .extend(fill);
    }
    if has_uvs {
        let fill = b.uvs.unwrap_or_else(|| vec![[0.0, 0.0]; b.positions.len()]);
        a.uvs
            .get_or_insert_with(|| vec![[0.0, 0.0]; a.positions.len()])
            .extend(fill);
    }
    if has_colors {
        let fill = b
            .vertex_colors
            .unwrap_or_else(|| vec![[1.0, 1.0, 1.0, 1.0]; b.positions.len()]);
        a.vertex_colors
            .get_or_insert_with(|| vec![[1.0, 1.0, 1.0, 1.0]; a.positions.len()])
            .extend(fill);
    }

    a.positions.extend(b.positions);
    a.indices.extend(b.indices.into_iter().map(|i| i + offset));

    a
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
        material_index: None,
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

    // ── parse_asset: multi-object ─────────────────────────────────

    #[test]
    fn two_objects_two_meshes() {
        let asset = parse_asset(&fixture("two_objects.obj"), &ObjOptions::default()).unwrap();

        // One MeshData per OBJ object — no merging.
        assert_eq!(asset.meshes.len(), 2);
        assert_eq!(asset.meshes[0].vertex_count(), 3);
        assert_eq!(asset.meshes[1].vertex_count(), 3);
        assert!(asset.meshes[0].validate().is_ok());
        assert!(asset.meshes[1].validate().is_ok());
        // Both meshes have their own local indices.
        assert_eq!(asset.meshes[1].indices, vec![0, 1, 2]);
        // No MTL supplied → no materials.
        assert!(asset.materials.is_empty());
        assert!(asset.meshes.iter().all(|m| m.material_index.is_none()));
    }

    // ── parse_asset: MTL ──────────────────────────────────────────

    #[test]
    fn mtl_materials_indexed() {
        let options = ObjOptions {
            mtl: Some(fixture("two_objects.mtl")),
            ..Default::default()
        };
        let asset = parse_asset(&fixture("two_objects_textured.obj"), &options).unwrap();

        assert_eq!(asset.materials.len(), 2);
        assert_eq!(asset.materials[0].name.as_deref(), Some("red"));
        assert_eq!(asset.materials[0].base_color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(asset.materials[1].name.as_deref(), Some("blue_transparent"));
        assert_eq!(asset.materials[1].base_color[0..3], [0.0, 0.0, 1.0]);

        // material_id links objects to materials.
        assert_eq!(asset.meshes[0].material_index, Some(0));
        assert_eq!(asset.meshes[1].material_index, Some(1));
        assert!(asset.capabilities().has_materials);
    }

    #[test]
    fn mtl_texture_is_uri() {
        let options = ObjOptions {
            mtl: Some(fixture("two_objects.mtl")),
            ..Default::default()
        };
        let asset = parse_asset(&fixture("two_objects_textured.obj"), &options).unwrap();

        // map_Kd becomes a Uri texture — core never resolves it.
        assert_eq!(asset.textures.len(), 1);
        assert_eq!(
            asset.textures[0].source,
            TextureSource::Uri("textures/brick.png".to_string())
        );
        assert_eq!(asset.materials[0].base_color_texture, None);
        assert_eq!(
            asset.materials[1].base_color_texture,
            Some(TextureSlot::default())
        );
        assert!(asset.materials[1].has_textures);
        assert!(!asset.materials[0].has_textures);
    }

    #[test]
    fn mtl_alpha_to_base_color() {
        let options = ObjOptions {
            mtl: Some(fixture("two_objects.mtl")),
            ..Default::default()
        };
        let asset = parse_asset(&fixture("two_objects_textured.obj"), &options).unwrap();

        // `d 1.0` → opaque, `d 0.5` → alpha 0.5.
        assert_eq!(asset.materials[0].base_color[3], 1.0);
        assert_eq!(asset.materials[1].base_color[3], 0.5);
        assert_eq!(asset.materials[0].alpha_mode, AlphaMode::Opaque);
        assert_eq!(asset.materials[1].alpha_mode, AlphaMode::Blend);
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
