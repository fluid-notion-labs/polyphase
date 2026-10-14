//! GLB (binary glTF 2.0) writer — [`AssetData`] → GLB bytes.
//!
//! The inverse of the GLB parser: meshes, PBR materials, embedded/URI
//! textures, samplers, and the flattened node hierarchy are all written.
//! Ported from manifold's `src/io/glb.rs` (proven code) and restructured
//! around [`AssetData`].
//!
//! Purity: returns bytes, the caller does I/O.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::HashMap;

use gltf::binary::Glb;
use gltf_json as json;
use json::validation::{Checked, Checked::Valid, USize64};

use crate::capabilities::{
    AlphaMode, AssetData, MaterialData, SamplerData, TextureSlot, TopologyKind,
};
use super::WriteError;

// ---------------------------------------------------------------------------
// Buffer builder
// ---------------------------------------------------------------------------

/// Accumulates the GLB binary chunk, keeping every section 4-byte aligned
/// and recording `(offset, length)` pairs for buffer views.
struct BufferBuilder {
    data: Vec<u8>,
}

impl BufferBuilder {
    fn new() -> Self {
        Self { data: Vec::new() }
    }

    /// Append bytes, 4-byte aligning first. Returns `(offset, length)`.
    fn push(&mut self, bytes: &[u8]) -> (usize, usize) {
        let offset = align4(self.data.len());
        self.data.resize(offset, 0);
        self.data.extend_from_slice(bytes);
        (offset, bytes.len())
    }

    /// Finish: pad the whole buffer to a multiple of 4 (GLB requirement).
    fn finish(mut self) -> Vec<u8> {
        while self.data.len() % 4 != 0 {
            self.data.push(0);
        }
        self.data
    }
}

/// Align a number up to the next multiple of 4.
fn align4(n: usize) -> usize {
    (n + 3) & !3
}

/// Min/max of a position array (glTF requires them on POSITION accessors).
fn bounds(positions: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for p in positions {
        for i in 0..3 {
            min[i] = f32::min(min[i], p[i]);
            max[i] = f32::max(max[i], p[i]);
        }
    }
    if positions.is_empty() {
        min = [0.0; 3];
        max = [0.0; 3];
    }
    (min, max)
}

/// Map [`TopologyKind`] to the glTF primitive mode.
fn gltf_mode(topology: TopologyKind) -> json::mesh::Mode {
    match topology {
        TopologyKind::TriangleList => json::mesh::Mode::Triangles,
        TopologyKind::TriangleStrip => json::mesh::Mode::TriangleStrip,
        TopologyKind::LineList => json::mesh::Mode::Lines,
        TopologyKind::PointList => json::mesh::Mode::Points,
    }
}

fn is_identity(m: &[f32; 16]) -> bool {
    m.iter()
        .enumerate()
        .all(|(i, v)| *v == if i % 5 == 0 { 1.0 } else { 0.0 })
}

// ---------------------------------------------------------------------------
// glTF enum mapping (raw GL enum values → gltf-json enums)
// ---------------------------------------------------------------------------

fn mag_filter(gl: Option<u32>) -> Option<Checked<json::texture::MagFilter>> {
    gl.map(|v| {
        Valid(match v {
            9728 => json::texture::MagFilter::Nearest,
            9729 => json::texture::MagFilter::Linear,
            _ => json::texture::MagFilter::Linear, // "auto" default
        })
    })
}

fn min_filter(gl: Option<u32>) -> Option<Checked<json::texture::MinFilter>> {
    gl.map(|v| {
        Valid(match v {
            9728 => json::texture::MinFilter::Nearest,
            9729 => json::texture::MinFilter::Linear,
            9984 => json::texture::MinFilter::NearestMipmapNearest,
            9985 => json::texture::MinFilter::LinearMipmapNearest,
            9986 => json::texture::MinFilter::NearestMipmapLinear,
            9987 => json::texture::MinFilter::LinearMipmapLinear,
            _ => json::texture::MinFilter::LinearMipmapLinear, // "auto" default
        })
    })
}

fn wrapping_mode(gl: u32) -> Checked<json::texture::WrappingMode> {
    Valid(match gl {
        33071 => json::texture::WrappingMode::ClampToEdge,
        33648 => json::texture::WrappingMode::MirroredRepeat,
        _ => json::texture::WrappingMode::Repeat, // 10497 + default
    })
}

fn alpha_mode(mode: &AlphaMode) -> Checked<json::material::AlphaMode> {
    Valid(match mode {
        AlphaMode::Opaque => json::material::AlphaMode::Opaque,
        AlphaMode::Mask => json::material::AlphaMode::Mask,
        AlphaMode::Blend => json::material::AlphaMode::Blend,
    })
}

fn mime_type(mime: crate::capabilities::TextureMime) -> json::image::MimeType {
    json::image::MimeType(
        match mime {
            crate::capabilities::TextureMime::Png => "image/png",
            crate::capabilities::TextureMime::Jpeg => "image/jpeg",
        }
        .to_string(),
    )
}

// ---------------------------------------------------------------------------
// Texture registry (dedupe samplers/textures by value)
// ---------------------------------------------------------------------------

/// Deduplicates samplers (by [`SamplerData`] value) and glTF textures (by
/// image+sampler pair) while pushing into the JSON root.
#[derive(Default)]
struct TextureRegistry {
    samplers: HashMap<SamplerData, json::Index<json::texture::Sampler>>,
    textures: HashMap<(usize, SamplerData), json::Index<json::texture::Texture>>,
}

impl TextureRegistry {
    /// Look up or create the glTF sampler for resolved sampler state.
    fn sampler(
        &mut self,
        root: &mut json::Root,
        s: &SamplerData,
    ) -> json::Index<json::texture::Sampler> {
        *self.samplers.entry(*s).or_insert_with(|| {
            root.push(json::texture::Sampler {
                mag_filter: mag_filter(s.mag_filter),
                min_filter: min_filter(s.min_filter),
                wrap_s: wrapping_mode(s.wrap_s),
                wrap_t: wrapping_mode(s.wrap_t),
                name: None,
                extensions: Default::default(),
                extras: Default::default(),
            })
        })
    }

    /// Look up or create the glTF texture for a [`TextureSlot`].
    ///
    /// `slot.image` must be a valid index into the images pushed so far.
    fn texture(
        &mut self,
        root: &mut json::Root,
        images: &[json::Index<json::Image>],
        slot: &TextureSlot,
    ) -> json::Index<json::texture::Texture> {
        let key = (slot.image, slot.sampler);
        if let Some(&idx) = self.textures.get(&key) {
            return idx;
        }
        let sampler = self.sampler(root, &slot.sampler);
        let idx = root.push(json::texture::Texture {
            name: None,
            sampler: Some(sampler),
            source: images[slot.image],
            extensions: Default::default(),
            extras: Default::default(),
        });
        self.textures.insert(key, idx);
        idx
    }
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// Write an [`AssetData`] as an uncompressed binary GLB.
///
/// Errors if the asset has no meshes or any mesh fails validation.
pub fn write_glb(asset: &AssetData) -> Result<Vec<u8>, WriteError> {
    write_glb_inner(asset, false)
}

/// Debug-friendly variant: the JSON chunk is pretty-printed (larger output,
/// identical semantics).
pub fn write_glb_pretty(asset: &AssetData) -> Result<Vec<u8>, WriteError> {
    write_glb_inner(asset, true)
}

fn write_glb_inner(asset: &AssetData, pretty: bool) -> Result<Vec<u8>, WriteError> {
    const FORMAT: &str = "glb";

    if asset.meshes.is_empty() {
        return Err(WriteError::Unsupported {
            format: FORMAT,
            reason: "asset has no meshes".to_string(),
        });
    }
    for (i, mesh) in asset.meshes.iter().enumerate() {
        mesh.validate().map_err(|e| WriteError::Unsupported {
            format: FORMAT,
            reason: format!("mesh {i} failed validation: {e}"),
        })?;
    }
    // All referenced texture slots must point at a real image.
    for (mi, material) in asset.materials.iter().enumerate() {
        for slot in material.texture_slots() {
            if slot.image >= asset.textures.len() {
                return Err(WriteError::Unsupported {
                    format: FORMAT,
                    reason: format!(
                        "material {mi} references image {0} but the asset has only {1} texture(s)",
                        slot.image,
                        asset.textures.len()
                    ),
                });
            }
        }
    }

    let mut root = json::Root::default();
    let mut buffer = BufferBuilder::new();

    // --- Buffer views & accessors per mesh ---------------------------------
    struct MeshRefs {
        indices: json::Index<json::Accessor>,
        positions: json::Index<json::Accessor>,
        normals: Option<json::Index<json::Accessor>>,
        uvs: Option<json::Index<json::Accessor>>,
        colors: Option<json::Index<json::Accessor>>,
    }

    let mesh_refs: Vec<MeshRefs> = asset
        .meshes
        .iter()
        .map(|mesh| {
            let indices = push_accessor(
                &mut root,
                &mut buffer,
                bytemuck::cast_slice(&mesh.indices),
                json::buffer::Target::ElementArrayBuffer,
                mesh.indices.len(),
                json::accessor::ComponentType::U32,
                json::accessor::Type::Scalar,
            );

            // POSITION accessors must carry min/max.
            let (min, max) = bounds(&mesh.positions);
            let positions = {
                let mut acc = push_accessor_plain(
                    &mut root,
                    &mut buffer,
                    bytemuck::cast_slice(&mesh.positions),
                    json::buffer::Target::ArrayBuffer,
                    mesh.positions.len(),
                    json::accessor::ComponentType::F32,
                    json::accessor::Type::Vec3,
                );
                acc.min = Some(json::Value::from(Vec::from(min)));
                acc.max = Some(json::Value::from(Vec::from(max)));
                root.push(acc)
            };

            let normals = mesh.normals.as_ref().map(|n| {
                push_accessor(
                    &mut root,
                    &mut buffer,
                    bytemuck::cast_slice(n),
                    json::buffer::Target::ArrayBuffer,
                    n.len(),
                    json::accessor::ComponentType::F32,
                    json::accessor::Type::Vec3,
                )
            });

            let uvs = mesh.uvs.as_ref().map(|u| {
                push_accessor(
                    &mut root,
                    &mut buffer,
                    bytemuck::cast_slice(u),
                    json::buffer::Target::ArrayBuffer,
                    u.len(),
                    json::accessor::ComponentType::F32,
                    json::accessor::Type::Vec2,
                )
            });

            let colors = mesh.vertex_colors.as_ref().map(|c| {
                push_accessor(
                    &mut root,
                    &mut buffer,
                    bytemuck::cast_slice(c),
                    json::buffer::Target::ArrayBuffer,
                    c.len(),
                    json::accessor::ComponentType::F32,
                    json::accessor::Type::Vec4,
                )
            });

            MeshRefs {
                indices,
                positions,
                normals,
                uvs,
                colors,
            }
        })
        .collect();

    // --- Images ---------------------------------------------------------------
    // Embedded images go into the binary buffer; Uri images become `image.uri`.
    let images: Vec<json::Index<json::Image>> = asset
        .textures
        .iter()
        .map(|tex| {
            let image = match &tex.source {
                crate::capabilities::TextureSource::Embedded { bytes, mime } => json::Image {
                    buffer_view: Some(push_view(&mut root, &mut buffer, bytes, None)),
                    mime_type: Some(mime_type(*mime)),
                    uri: None,
                    name: tex.name.clone(),
                    extensions: Default::default(),
                    extras: Default::default(),
                },
                crate::capabilities::TextureSource::Uri(uri) => json::Image {
                    buffer_view: None,
                    mime_type: None,
                    uri: Some(uri.clone()),
                    name: tex.name.clone(),
                    extensions: Default::default(),
                    extras: Default::default(),
                },
            };
            root.push(image)
        })
        .collect();

    // --- Materials ----------------------------------------------------------
    let mut registry = TextureRegistry::default();
    let materials: Vec<json::Index<json::Material>> = asset
        .materials
        .iter()
        .map(|m| {
            let mat = material_json(&mut root, &mut registry, &images, m);
            root.push(mat)
        })
        .collect();

    // --- Meshes -------------------------------------------------------------
    let mesh_indices: Vec<json::Index<json::Mesh>> = asset
        .meshes
        .iter()
        .zip(&mesh_refs)
        .map(|(mesh, refs)| {
            let mut attributes = BTreeMap::new();
            attributes.insert(Valid(json::mesh::Semantic::Positions), refs.positions);
            if let Some(n) = refs.normals {
                attributes.insert(Valid(json::mesh::Semantic::Normals), n);
            }
            if let Some(u) = refs.uvs {
                attributes.insert(Valid(json::mesh::Semantic::TexCoords(0)), u);
            }
            if let Some(c) = refs.colors {
                attributes.insert(Valid(json::mesh::Semantic::Colors(0)), c);
            }

            let material = mesh.material_index.and_then(|mi| materials.get(mi).copied());

            let primitive = json::mesh::Primitive {
                attributes,
                indices: Some(refs.indices),
                material,
                mode: Valid(gltf_mode(mesh.topology)),
                targets: None,
                extensions: Default::default(),
                extras: Default::default(),
            };

            root.push(json::Mesh {
                name: None, // MeshData carries no name
                primitives: vec![primitive],
                weights: None,
                extensions: Default::default(),
                extras: Default::default(),
            })
        })
        .collect();

    // --- Nodes & scene --------------------------------------------------------
    let referenced_meshes: Vec<bool> = {
        let mut used = vec![false; asset.meshes.len()];
        for node in &asset.nodes {
            if let Some(m) = node.mesh {
                used[m] = true;
            }
        }
        used
    };

    let node_indices: Vec<json::Index<json::Node>> = asset
        .nodes
        .iter()
        .map(|node| {
            root.push(json::Node {
                name: node.name.clone(),
                mesh: node.mesh.map(|m| mesh_indices[m]),
                matrix: (!is_identity(&node.transform)).then_some(node.transform),
                children: (!node.children.is_empty()).then(|| {
                    node.children
                        .iter()
                        .map(|ci| json::Index::new(*ci as u32))
                        .collect()
                }),
                ..Default::default()
            })
        })
        .collect();

    // Any mesh not attached to a node gets a standalone node so geometry is
    // never dropped.
    let orphan_nodes: Vec<json::Index<json::Node>> = referenced_meshes
        .iter()
        .zip(&mesh_indices)
        .filter(|(used, _)| !**used)
        .map(|(_, mesh)| root.push(json::Node {
            mesh: Some(*mesh),
            ..Default::default()
        }))
        .collect();

    let scene_nodes: Vec<json::Index<json::Node>> = if asset.nodes.is_empty() {
        // No hierarchy: one standalone node per mesh.
        mesh_indices
            .iter()
            .map(|m| root.push(json::Node { mesh: Some(*m), ..Default::default() }))
            .collect()
    } else {
        asset
            .root_nodes
            .iter()
            .map(|ri| node_indices[*ri])
            .chain(orphan_nodes)
            .collect()
    };

    root.push(json::Scene {
        name: asset.name.clone(),
        nodes: scene_nodes,
        extensions: Default::default(),
        extras: Default::default(),
    });
    root.scene = Some(json::Index::new(0));

    // --- Buffer + GLB container ----------------------------------------------
    let buffer_data = buffer.finish();
    root.push(json::Buffer {
        byte_length: USize64::from(buffer_data.len()),
        uri: None,
        name: None,
        extensions: Default::default(),
        extras: Default::default(),
    });

    root.asset.generator = Some("polyphase".to_string());

    let json_string = if pretty {
        root.to_string_pretty()
    } else {
        root.to_string()
    }
    .map_err(|e| WriteError::GlbSerialization(e.to_string()))?;

    // JSON chunk is padded with spaces (0x20) per the GLB spec.
    let json_length = align4(json_string.len());
    let mut json_padded = json_string.into_bytes();
    json_padded.resize(json_length, b' ');

    let glb = Glb {
        header: gltf::binary::Header {
            magic: *b"glTF",
            version: 2,
            length: (12 + 8 + json_length + 8 + buffer_data.len()) as u32,
        },
        json: Cow::Owned(json_padded),
        bin: Some(Cow::Owned(buffer_data)),
    };

    let mut out = Vec::new();
    glb.to_writer(&mut out)
        .map_err(|e| WriteError::GlbSerialization(e.to_string()))?;
    Ok(out)
}

// ---------------------------------------------------------------------------
// JSON construction helpers
// ---------------------------------------------------------------------------

/// Push a byte section, register its buffer view, and register its accessor.
fn push_accessor(
    root: &mut json::Root,
    buffer: &mut BufferBuilder,
    bytes: &[u8],
    target: json::buffer::Target,
    count: usize,
    component_type: json::accessor::ComponentType,
    type_: json::accessor::Type,
) -> json::Index<json::Accessor> {
    let view = push_view(root, buffer, bytes, Some(target));
    root.push(accessor(view, count, component_type, type_))
}

/// Like [`push_accessor`] but returns the unpushed accessor (so callers can
/// adjust fields such as `min`/`max` before pushing).
fn push_accessor_plain(
    root: &mut json::Root,
    buffer: &mut BufferBuilder,
    bytes: &[u8],
    target: json::buffer::Target,
    count: usize,
    component_type: json::accessor::ComponentType,
    type_: json::accessor::Type,
) -> json::Accessor {
    accessor(
        push_view(root, buffer, bytes, Some(target)),
        count,
        component_type,
        type_,
    )
}

/// Build an accessor with the common defaults.
fn accessor(
    buffer_view: json::Index<json::buffer::View>,
    count: usize,
    component_type: json::accessor::ComponentType,
    type_: json::accessor::Type,
) -> json::Accessor {
    json::Accessor {
        buffer_view: Some(buffer_view),
        byte_offset: Some(USize64(0)),
        count: USize64::from(count),
        component_type: Valid(json::accessor::GenericComponentType(component_type)),
        type_: Valid(type_),
        min: None,
        max: None,
        name: None,
        normalized: false,
        sparse: None,
        extensions: Default::default(),
        extras: Default::default(),
    }
}

/// Push a byte section into the buffer and register its buffer view.
///
/// `target` is `None` for non-vertex data (e.g. image bytes).
fn push_view(
    root: &mut json::Root,
    buffer: &mut BufferBuilder,
    bytes: &[u8],
    target: Option<json::buffer::Target>,
) -> json::Index<json::buffer::View> {
    let (offset, length) = buffer.push(bytes);
    root.push(json::buffer::View {
        buffer: json::Index::new(0), // single buffer, registered at the end
        byte_length: USize64::from(length),
        byte_offset: Some(USize64::from(offset)),
        byte_stride: None,
        target: target.map(Valid),
        name: None,
        extensions: Default::default(),
        extras: Default::default(),
    })
}

/// Serialize one [`MaterialData`] to a glTF material.
fn material_json(
    root: &mut json::Root,
    registry: &mut TextureRegistry,
    images: &[json::Index<json::Image>],
    m: &MaterialData,
) -> json::Material {
    let base_color_texture = m
        .base_color_texture
        .as_ref()
        .map(|s| texture_info(root, registry, images, s));
    let metallic_roughness_texture = m
        .metallic_roughness_texture
        .as_ref()
        .map(|s| texture_info(root, registry, images, s));
    let normal_texture = m.normal_texture.as_ref().map(|slot| {
        json::material::NormalTexture {
            index: registry.texture(root, images, slot),
            scale: 1.0,
            tex_coord: slot.tex_coord,
            extensions: Default::default(),
            extras: Default::default(),
        }
    });
    let emissive_texture = m
        .emissive_texture
        .as_ref()
        .map(|s| texture_info(root, registry, images, s));

    json::Material {
        name: m.name.clone(),
        pbr_metallic_roughness: json::material::PbrMetallicRoughness {
            base_color_factor: json::material::PbrBaseColorFactor(m.base_color),
            base_color_texture,
            metallic_factor: json::material::StrengthFactor(m.metallic),
            roughness_factor: json::material::StrengthFactor(m.roughness),
            metallic_roughness_texture,
            extensions: Default::default(),
            extras: Default::default(),
        },
        alpha_mode: alpha_mode(&m.alpha_mode),
        alpha_cutoff: (m.alpha_mode == AlphaMode::Mask)
            .then(|| json::material::AlphaCutoff(m.alpha_cutoff)),
        double_sided: m.double_sided,
        normal_texture,
        emissive_factor: json::material::EmissiveFactor(m.emissive),
        emissive_texture,
        occlusion_texture: None,
        extensions: Default::default(),
        extras: Default::default(),
    }
}

/// Look up (or create) the glTF texture for a slot and wrap it in an `Info`
/// carrying the slot's UV set.
fn texture_info(
    root: &mut json::Root,
    registry: &mut TextureRegistry,
    images: &[json::Index<json::Image>],
    slot: &TextureSlot,
) -> json::texture::Info {
    json::texture::Info {
        index: registry.texture(root, images, slot),
        tex_coord: slot.tex_coord,
        extensions: Default::default(),
        extras: Default::default(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_builder_aligns_and_pads() {
        let mut b = BufferBuilder::new();
        let (o1, l1) = b.push(&[1u8, 2, 3]); // 3 bytes — next push must align
        assert_eq!((o1, l1), (0, 3));
        let (o2, l2) = b.push(&[4u8, 5, 6, 7, 8]); // 5 bytes, offset must be 4
        assert_eq!((o2, l2), (4, 5));
        let out = b.finish();
        assert_eq!(out.len(), 12); // padded 9 → 12
        assert_eq!(&out[0..3], &[1, 2, 3]);
        assert_eq!(out[3], 0);
        assert_eq!(&out[4..9], &[4, 5, 6, 7, 8]);
    }

    #[test]
    fn gltf_mode_mapping() {
        assert!(matches!(
            gltf_mode(TopologyKind::TriangleList),
            json::mesh::Mode::Triangles
        ));
        assert!(matches!(
            gltf_mode(TopologyKind::PointList),
            json::mesh::Mode::Points
        ));
    }

    #[test]
    fn identity_detection() {
        assert!(is_identity(&crate::capabilities::NodeData::default().transform));
        let mut m = [0.0f32; 16];
        m[0] = 2.0;
        m[15] = 1.0;
        assert!(!is_identity(&m));
    }

    #[test]
    fn sampler_gl_enum_mapping() {
        // Raw GL enums map to gltf-json enums and back to the same GL enum.
        assert_eq!(
            mag_filter(Some(crate::GL_NEAREST)).unwrap().unwrap().as_gl_enum(),
            crate::GL_NEAREST
        );
        assert_eq!(
            min_filter(Some(crate::GL_LINEAR_MIPMAP_LINEAR))
                .unwrap()
                .unwrap()
                .as_gl_enum(),
            crate::GL_LINEAR_MIPMAP_LINEAR
        );
        assert_eq!(
            wrapping_mode(crate::GL_CLAMP_TO_EDGE).unwrap().as_gl_enum(),
            crate::GL_CLAMP_TO_EDGE
        );
        assert_eq!(
            wrapping_mode(crate::GL_MIRRORED_REPEAT).unwrap().as_gl_enum(),
            crate::GL_MIRRORED_REPEAT
        );
        assert_eq!(
            wrapping_mode(crate::GL_WRAP_REPEAT).unwrap().as_gl_enum(),
            crate::GL_WRAP_REPEAT
        );
    }
}
