//! Generate `tests/fixtures/quad_meshopt.glb` — a quad whose buffers are
//! compressed with `EXT_meshopt_compression`.
//!
//! (Without the feature the example is a stub that exits with an error.)
//!
//! Run: `cargo run --features meshopt --example gen_meshopt_fixture`
//!
//! Encodes positions, normals, UVs, and indices as separate compressed
//! buffer views (the same layout manifold's `write_glb_with_meshopt`
//! produces), so the GLB parser's three decode paths (vec3, vec2, indices)
//! are all exercised.

use std::io::Write;

#[cfg(feature = "meshopt")]
fn main() -> anyhow::Result<()> {
    let quad_positions: Vec<[f32; 3]> = vec![
        [-0.5, -0.5, 0.0],
        [0.5, -0.5, 0.0],
        [0.5, 0.5, 0.0],
        [-0.5, 0.5, 0.0],
    ];
    let quad_normals: Vec<[f32; 3]> = vec![[0.0, 0.0, 1.0]; 4];
    let quad_uvs: Vec<[f32; 2]> = vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    let quad_indices: Vec<u32> = vec![0, 1, 2, 0, 2, 3];

    let encoded_positions: Vec<u8> =
        bytemuck::cast_slice(&meshopt::encode_vertex_buffer(&quad_positions)?).to_vec();
    let encoded_normals: Vec<u8> =
        bytemuck::cast_slice(&meshopt::encode_vertex_buffer(&quad_normals)?).to_vec();
    let encoded_uvs: Vec<u8> =
        bytemuck::cast_slice(&meshopt::encode_vertex_buffer(&quad_uvs)?).to_vec();
    let encoded_indices: Vec<u8> = meshopt::encode_index_buffer(&quad_indices, 4)?.to_vec();

    // Binary chunk: [positions][normals][uvs][indices]
    let mut bin = Vec::new();
    let pos_off = 0;
    bin.extend_from_slice(&encoded_positions);
    let nrm_off = bin.len();
    bin.extend_from_slice(&encoded_normals);
    let uv_off = bin.len();
    bin.extend_from_slice(&encoded_uvs);
    let idx_off = bin.len();
    bin.extend_from_slice(&encoded_indices);
    while bin.len() % 4 != 0 {
        bin.push(0);
    }

    let views = vec![
        compressed_view(pos_off, encoded_positions.len(), 12, 4, "ATTRIBUTES"),
        compressed_view(nrm_off, encoded_normals.len(), 12, 4, "ATTRIBUTES"),
        compressed_view(uv_off, encoded_uvs.len(), 8, 4, "ATTRIBUTES"),
        compressed_view(idx_off, encoded_indices.len(), 4, 6, "INDICES"),
    ];

    let json = serde_json::json!({
        "asset": {"version": "2.0", "generator": "polyphase gen_meshopt_fixture"},
        "extensionsUsed": ["EXT_meshopt_compression"],
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0, "name": "quad"}],
        "meshes": [{
            "name": "quad",
            "primitives": [{
                "attributes": {"POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2},
                "indices": 3,
                "mode": 4
            }]
        }],
        "buffers": [{
            "byteLength": bin.len()
        }],
        // Accessor views point at the DECODED ranges (all start at 0 in the
        // virtual uncompressed buffer; byteStride is the decoded stride).
        "bufferViews": views,
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3", "min": [-0.5, -0.5, 0.0], "max": [0.5, 0.5, 0.0]},
            {"bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC3"},
            {"bufferView": 2, "componentType": 5126, "count": 4, "type": "VEC2"},
            {"bufferView": 3, "componentType": 5125, "count": 6, "type": "SCALAR"}
        ]
    });

    let _ = (pos_off, nrm_off, uv_off, idx_off); // offsets are per-view byteOffset below

    let mut json_bytes = serde_json::to_vec(&json)?;
    while json_bytes.len() % 4 != 0 {
        json_bytes.push(b' ');
    }
    while bin.len() % 4 != 0 {
        bin.push(0);
    }

    let total = 12 + 8 + json_bytes.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.write_all(&0x46546C67u32.to_le_bytes())?; // "glTF"
    out.write_all(&2u32.to_le_bytes())?;
    out.write_all(&(total as u32).to_le_bytes())?;
    out.write_all(&(json_bytes.len() as u32).to_le_bytes())?;
    out.write_all(&0x4E4F534Au32.to_le_bytes())?; // "JSON"
    out.write_all(&json_bytes)?;
    out.write_all(&(bin.len() as u32).to_le_bytes())?;
    out.write_all(&0x004E4942u32.to_le_bytes())?; // "BIN\0"
    out.write_all(&bin)?;

    std::fs::write("tests/fixtures/quad_meshopt.glb", &out)?;
    println!(
        "wrote tests/fixtures/quad_meshopt.glb ({} bytes)",
        out.len()
    );
    Ok(())
}

#[cfg(not(feature = "meshopt"))]
fn main() {
    eprintln!("this example requires --features meshopt");
    std::process::exit(1);
}

/// A compressed buffer view: `byteOffset`/`byteLength` locate the compressed
/// blob in the binary chunk; the extension declares where the DECODED data
/// lives (`buffer` 0, offsets 0..) and its decoded size.
fn compressed_view(
    blob_offset: usize,
    blob_len: usize,
    decoded_stride: usize,
    count: usize,
    mode: &str,
) -> serde_json::Value {
    serde_json::json!({
        "buffer": 0,
        "byteOffset": 0,
        "byteLength": decoded_stride * count,
        "extensions": {
            "EXT_meshopt_compression": {
                "buffer": 0,
                "byteOffset": blob_offset,
                "byteLength": blob_len,
                "byteStride": decoded_stride,
                "count": count,
                "mode": mode
            }
        }
    })
}
