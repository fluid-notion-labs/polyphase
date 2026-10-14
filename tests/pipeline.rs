//! Integration tests — full pipeline: parse → validate → capabilities.
//!
//! These tests exercise the public API across module boundaries, ensuring
//! that parsers, validation, and capabilities work together correctly.

use loadfu::parsers::{glb, obj, ply, stl};
use loadfu::{AssetCapabilities, AssetData, MeshData, MeshFormat, ParseCost};
use std::path::Path;

// ── Helpers ───────────────────────────────────────────────────────────

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("tests/fixtures/{name}")).expect("fixture not found")
}

/// Parse → validate → wrap in AssetData → derive capabilities.
fn pipeline_single(mesh: MeshData) -> AssetCapabilities {
    mesh.validate().expect("parsed mesh should be valid");
    let data = AssetData::single(mesh);
    data.capabilities()
}

// ── STL pipeline ──────────────────────────────────────────────────────

#[test]
fn stl_binary_full_pipeline() {
    let mesh = stl::parse(&fixture("triangle.stl")).unwrap();
    let cap = pipeline_single(mesh);

    assert!(cap.has_normals);
    assert!(!cap.has_uvs);
    assert!(!cap.multi_object);
    assert!(!cap.has_materials);
    assert!(cap.estimated_memory.is_some());
    assert!(cap.estimated_memory.unwrap() > 0);
}

#[test]
fn stl_ascii_full_pipeline() {
    let mesh = stl::parse(&fixture("triangle_ascii.stl")).unwrap();
    let cap = pipeline_single(mesh);

    assert!(cap.has_normals);
    assert!(!cap.has_uvs);
}

#[test]
fn stl_recompute_normals_pipeline() {
    let opts = stl::StlOptions {
        recompute_normals: true,
    };
    let mesh = stl::parse_with_options(&fixture("quad.stl"), &opts).unwrap();
    let cap = pipeline_single(mesh);

    assert!(cap.has_normals);
    assert_eq!(
        cap.estimated_memory.unwrap(),
        mesh_memory(4, true, false, 6)
    );
}

// ── OBJ pipeline ──────────────────────────────────────────────────────

#[test]
fn obj_triangle_full_pipeline() {
    let mesh = obj::parse(&fixture("triangle.obj")).unwrap();
    let cap = pipeline_single(mesh);

    assert!(cap.has_normals); // computed normals
    assert!(!cap.has_uvs);
    assert!(!cap.multi_object);
}

#[test]
fn obj_with_normals_and_uvs_pipeline() {
    let mesh = obj::parse(&fixture("triangle_full.obj")).unwrap();
    let cap = pipeline_single(mesh);

    assert!(cap.has_normals);
    assert!(cap.has_uvs);
}

#[test]
fn obj_multi_object_pipeline() {
    let mesh = obj::parse(&fixture("two_objects.obj")).unwrap();
    // OBJ merges into one MeshData, but we can check vertex count
    assert!(mesh.validate().is_ok());
    assert_eq!(mesh.vertex_count(), 6);
    assert_eq!(mesh.triangle_count(), 2);

    // As a single mesh in AssetData, multi_object is false (merged)
    let cap = pipeline_single(mesh);
    assert!(!cap.multi_object);
}

#[test]
fn obj_quad_triangulation_pipeline() {
    let mesh = obj::parse(&fixture("quad.obj")).unwrap();
    let cap = pipeline_single(mesh);

    assert_eq!(mesh_triangles_from_cap(&cap, 4, true, false, 6), 2);
}

// ── GLB pipeline ──────────────────────────────────────────────────────

#[test]
fn glb_cube_full_pipeline() {
    let meshes = glb::parse(&fixture("cube.glb")).unwrap();

    // Each primitive is valid
    for mesh in &meshes {
        mesh.validate().expect("GLB primitive should be valid");
    }

    // Multi-mesh AssetData
    let data = AssetData {
        meshes,
        ..Default::default()
    };
    let cap = data.capabilities();

    assert!(cap.has_normals);
    assert!(cap.estimated_memory.is_some());
    assert!(cap.estimated_memory.unwrap() > 0);
}

#[test]
fn glb_merged_full_pipeline() {
    let mesh = glb::parse_merged(&fixture("cube.glb")).unwrap();
    let cap = pipeline_single(mesh);

    assert!(cap.has_normals);
    assert!(!cap.multi_object); // merged into one
}

// ── PLY pipeline ──────────────────────────────────────────────────────

#[test]
fn ply_ascii_full_pipeline() {
    let mesh = ply::parse(&fixture("triangle.ply")).unwrap();
    let cap = pipeline_single(mesh);

    assert!(cap.has_normals);
    assert!(!cap.has_uvs);
    assert!(!cap.has_vertex_colors);
    assert!(!cap.multi_object);
    assert!(!cap.has_materials);
    assert!(cap.estimated_memory.is_some());
    assert!(cap.estimated_memory.unwrap() > 0);
}

#[test]
fn ply_binary_le_full_pipeline() {
    let mesh = ply::parse(&fixture("triangle_binary.ply")).unwrap();
    let cap = pipeline_single(mesh);

    assert!(cap.has_normals);
    assert!(!cap.has_uvs);
    assert!(!cap.multi_object);
}

#[test]
fn ply_vertex_colors_pipeline() {
    let mesh = ply::parse(&fixture("triangle_colors.ply")).unwrap();
    let cap = pipeline_single(mesh);

    assert!(cap.has_normals);
    assert!(cap.has_vertex_colors);
    assert!(!cap.has_uvs);
}

#[test]
fn ply_quad_triangulation_pipeline() {
    let mesh = ply::parse(&fixture("quad.ply")).unwrap();
    let cap = pipeline_single(mesh);

    assert_eq!(mesh_triangles_from_cap(&cap, 4, false, false, 6), 2);
}

#[test]
fn ply_recompute_normals_pipeline() {
    let opts = ply::PlyOptions {
        recompute_normals: true,
    };
    let mesh = ply::parse_with_options(&fixture("triangle.ply"), &opts).unwrap();
    let cap = pipeline_single(mesh);

    assert!(cap.has_normals);
}

// ── Format detection → parse pipeline ─────────────────────────────────

#[test]
fn format_detect_and_parse_stl() {
    let path = Path::new("model.stl");
    let format = MeshFormat::from_path(path).expect("should detect STL");
    assert_eq!(format, MeshFormat::Stl);

    let bytes = fixture("triangle.stl");
    let mesh = stl::parse(&bytes).unwrap();
    assert!(mesh.validate().is_ok());
}

#[test]
fn format_detect_and_parse_obj() {
    let path = Path::new("scene.obj");
    let format = MeshFormat::from_path(path).expect("should detect OBJ");
    assert_eq!(format, MeshFormat::Obj);

    let bytes = fixture("triangle.obj");
    let mesh = obj::parse(&bytes).unwrap();
    assert!(mesh.validate().is_ok());
}

#[test]
fn format_detect_and_parse_ply() {
    let path = Path::new("scan.ply");
    let format = MeshFormat::from_path(path).expect("should detect PLY");
    assert_eq!(format, MeshFormat::Ply);

    let bytes = fixture("triangle.ply");
    let mesh = ply::parse(&bytes).unwrap();
    assert!(mesh.validate().is_ok());
}

#[test]
fn format_detect_and_parse_glb() {
    let path = Path::new("frame_001.glb");
    let format = MeshFormat::from_path(path).expect("should detect GLB");
    assert_eq!(format, MeshFormat::Glb);

    let bytes = fixture("cube.glb");
    let meshes = glb::parse(&bytes).unwrap();
    assert!(!meshes.is_empty());
    for m in &meshes {
        assert!(m.validate().is_ok());
    }
}

// ── Static vs post-parse capabilities consistency ─────────────────────

#[test]
fn ply_static_caps_consistent_with_parsed() {
    let static_cap = MeshFormat::Ply.static_capabilities();
    let mesh = ply::parse(&fixture("triangle.ply")).unwrap();
    let parsed_cap = pipeline_single(mesh);

    // Static says PLY *can* have normals; parsed confirms it *does*
    assert!(static_cap.has_normals);
    assert!(parsed_cap.has_normals);

    // Static says PLY can have vertex colors; this file doesn't
    assert!(static_cap.has_vertex_colors);
    assert!(!parsed_cap.has_vertex_colors);

    // Static doesn't know memory; parsed does
    assert!(static_cap.estimated_memory.is_none());
    assert!(parsed_cap.estimated_memory.is_some());

    assert_eq!(static_cap.parse_cost, ParseCost::Low);
}

#[test]
fn stl_static_caps_consistent_with_parsed() {
    let static_cap = MeshFormat::Stl.static_capabilities();
    let mesh = stl::parse(&fixture("triangle.stl")).unwrap();
    let parsed_cap = pipeline_single(mesh);

    // Static says STL *can* have normals; parsed confirms it *does*
    assert!(static_cap.has_normals);
    assert!(parsed_cap.has_normals);

    // Static says STL cannot have UVs; parsed confirms
    assert!(!static_cap.has_uvs);
    assert!(!parsed_cap.has_uvs);

    // Static doesn't know memory; parsed does
    assert!(static_cap.estimated_memory.is_none());
    assert!(parsed_cap.estimated_memory.is_some());

    // Parse cost is set statically, not from data
    assert_eq!(static_cap.parse_cost, ParseCost::Low);
}

#[test]
fn obj_static_caps_consistent_with_parsed() {
    let static_cap = MeshFormat::Obj.static_capabilities();

    // OBJ with full attributes
    let mesh = obj::parse(&fixture("triangle_full.obj")).unwrap();
    let parsed_cap = pipeline_single(mesh);

    assert!(static_cap.has_normals);
    assert!(parsed_cap.has_normals);
    assert!(static_cap.has_uvs);
    assert!(parsed_cap.has_uvs);
    assert_eq!(static_cap.parse_cost, ParseCost::Medium);
}

#[test]
fn glb_static_caps_consistent_with_parsed() {
    let static_cap = MeshFormat::Glb.static_capabilities();
    let mesh = glb::parse_merged(&fixture("cube.glb")).unwrap();
    let parsed_cap = pipeline_single(mesh);

    assert!(static_cap.has_normals);
    assert!(parsed_cap.has_normals);
    assert_eq!(static_cap.parse_cost, ParseCost::Low);
}

// ── Cross-format consistency ──────────────────────────────────────────

#[test]
fn all_parsers_produce_valid_meshdata() {
    let stl_mesh = stl::parse(&fixture("triangle.stl")).unwrap();
    let obj_mesh = obj::parse(&fixture("triangle.obj")).unwrap();
    let glb_meshes = glb::parse(&fixture("cube.glb")).unwrap();
    let ply_mesh = ply::parse(&fixture("triangle.ply")).unwrap();

    assert!(stl_mesh.validate().is_ok());
    assert!(obj_mesh.validate().is_ok());
    for m in &glb_meshes {
        assert!(m.validate().is_ok());
    }
    assert!(ply_mesh.validate().is_ok());
}

#[test]
fn triangle_vertex_count_consistent_across_formats() {
    let stl_mesh = stl::parse(&fixture("triangle.stl")).unwrap();
    let obj_mesh = obj::parse(&fixture("triangle.obj")).unwrap();
    let ply_mesh = ply::parse(&fixture("triangle.ply")).unwrap();

    // All are a single triangle — should have same geometry shape
    assert_eq!(stl_mesh.vertex_count(), 3);
    assert_eq!(obj_mesh.vertex_count(), 3);
    assert_eq!(ply_mesh.vertex_count(), 3);
    assert_eq!(stl_mesh.triangle_count(), 1);
    assert_eq!(obj_mesh.triangle_count(), 1);
    assert_eq!(ply_mesh.triangle_count(), 1);
}

// ── Memory estimation sanity ──────────────────────────────────────────

#[test]
fn memory_grows_with_mesh_size() {
    let small = stl::parse(&fixture("triangle.stl")).unwrap();
    let large = stl::parse(&fixture("quad.stl")).unwrap();

    let small_mem = small.estimated_memory_bytes();
    let large_mem = large.estimated_memory_bytes();

    assert!(
        large_mem > small_mem,
        "quad ({large_mem}) should use more memory than triangle ({small_mem})"
    );
}

#[test]
fn glb_cube_memory_is_reasonable() {
    let mesh = glb::parse_merged(&fixture("cube.glb")).unwrap();
    let mem = mesh.estimated_memory_bytes();

    // A cube: 24 verts * 12 (pos) + 24 * 12 (normals) + 36 * 4 (indices) = 720
    // Should be in the hundreds of bytes, not kilobytes
    assert!(mem > 100, "cube memory too small: {mem}");
    assert!(mem < 4096, "cube memory too large: {mem}");
}

// ── Top-level dispatch: parse_asset / load ───────────────────────────

#[test]
fn parse_asset_dispatches_all_formats() {
    let cases = [
        ("triangle.stl", MeshFormat::Stl),
        ("triangle.obj", MeshFormat::Obj),
        ("triangle.ply", MeshFormat::Ply),
        ("cube.glb", MeshFormat::Glb),
    ];

    for (name, format) in cases {
        let asset =
            loadfu::parse_asset(&fixture(name), format).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(!asset.meshes.is_empty(), "{name}: no meshes");
        for mesh in &asset.meshes {
            mesh.validate().unwrap_or_else(|e| panic!("{name}: {e}"));
        }
        assert!(asset.capabilities().estimated_memory.is_some());
    }
}

#[test]
fn load_with_extension_hint() {
    let data = loadfu::load(&fixture("cube.glb"), Some(Path::new("scene.glb"))).unwrap();
    assert_eq!(data.meshes.len(), 1);
    assert!(data.meshes[0].validate().is_ok());
}

#[test]
fn load_sniffs_when_no_hint() {
    for name in ["triangle.stl", "triangle.obj", "triangle.ply", "cube.glb"] {
        let asset =
            loadfu::load(&fixture(name), None).unwrap_or_else(|e| panic!("sniff {name}: {e}"));
        assert!(!asset.meshes.is_empty(), "sniff {name}: no meshes");
    }
}

#[test]
fn load_unrecognized_bytes_error() {
    let err = loadfu::load(b"{}", Some(Path::new("thing.gltf"))).unwrap_err();
    assert!(
        matches!(err, loadfu::LoadError::Glb(_)),
        "glTF text with no data: {err}"
    );

    let err = loadfu::load(b"not a mesh at all", None).unwrap_err();
    assert!(
        matches!(err, loadfu::LoadError::UnsupportedFormat { detected: None }),
        "got {err}"
    );
}

#[test]
fn glb_parse_asset_full_pipeline() {
    let asset = loadfu::parse_asset(&fixture("cube_textured.glb"), MeshFormat::Glb).unwrap();

    // Round-trip: validate every mesh, then derive capabilities.
    for mesh in &asset.meshes {
        mesh.validate().unwrap();
    }
    let cap = asset.capabilities();
    assert!(cap.has_materials);
    assert!(cap.has_textures);
    assert!(cap.multi_object); // two primitives

    // Materials and meshes are linked.
    assert_eq!(asset.materials.len(), 2);
    assert_eq!(asset.meshes[0].material_index, Some(0));
    assert_eq!(asset.meshes[1].material_index, Some(1));
}

#[test]
fn obj_parse_asset_full_pipeline() {
    use loadfu::parsers::obj::ObjOptions;

    let options = ObjOptions {
        mtl: Some(fixture("two_objects.mtl")),
        ..Default::default()
    };
    let asset =
        loadfu::parsers::obj::parse_asset(&fixture("two_objects_textured.obj"), &options).unwrap();

    for mesh in &asset.meshes {
        mesh.validate().unwrap();
    }
    let cap = asset.capabilities();
    assert!(cap.multi_object);
    assert!(cap.has_materials);
    assert!(cap.has_textures);

    assert_eq!(asset.meshes[0].material_index, Some(0));
    assert_eq!(asset.meshes[1].material_index, Some(1));
    assert!(matches!(
        asset.textures[0].source,
        loadfu::TextureSource::Uri(_)
    ));
}

// ── Helpers (internal) ────────────────────────────────────────────────

/// Calculate expected memory for a mesh with known attributes.
fn mesh_memory(verts: usize, has_normals: bool, has_uvs: bool, indices: usize) -> u64 {
    mesh_memory_full(verts, has_normals, has_uvs, false, indices)
}

fn mesh_memory_full(
    verts: usize,
    has_normals: bool,
    has_uvs: bool,
    has_colors: bool,
    indices: usize,
) -> u64 {
    let mut total = verts * 12; // positions: [f32; 3]
    if has_normals {
        total += verts * 12; // normals: [f32; 3]
    }
    if has_uvs {
        total += verts * 8; // uvs: [f32; 2]
    }
    if has_colors {
        total += verts * 16; // vertex_colors: [f32; 4]
    }
    total += indices * 4; // indices: u32
    total as u64
}

/// Helper to verify triangle count from capabilities memory estimate.
/// Reverse-engineers triangle count from estimated memory and known attributes.
fn mesh_triangles_from_cap(
    cap: &AssetCapabilities,
    verts: usize,
    has_normals: bool,
    has_uvs: bool,
    expected_indices: usize,
) -> usize {
    let expected_mem = mesh_memory(verts, has_normals, has_uvs, expected_indices);
    assert_eq!(cap.estimated_memory, Some(expected_mem as u64));
    expected_indices / 3
}
