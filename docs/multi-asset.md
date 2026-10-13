# Multi-Asset Implementation Playbook

**Purpose:** the working guide for making loadfu multi-asset aware — how to
add textures, materials, hierarchy, and new formats to this crate, following
its own conventions. Written for implementers working *in this repo*.

**Companion analysis:** the full validation of the plans (what holds up,
gaps G1–G7, reasoning) lives in the manifold repo at
`manifold/research/architecture/loadfu-multi-asset.md` — read that for
*why*, this for *how*.

**Current state at time of writing** (`c90bad8`, 157 tests):
geometry-only parsers, `MeshData` already carries `uvs` and `vertex_colors`,
GLB parser reads UV set 0 and drops `_images` on the floor.

---

## Part A — What exists today, per capability

| Capability | State | Where |
|---|---|---|
| Positions / normals / indices | ✅ all parsers | `mesh_data.rs` |
| **UVs** | ✅ GLB (set 0 only), OBJ (`vt` via tobj), PLY | `parsers/*.rs`, `MeshData::uvs` |
| **Vertex colors** | ✅ GLB, PLY (0–255 auto-normalized) | `MeshData::vertex_colors` |
| Topology (tri/line/point/strip) | ✅ core-wide, all parsers emit TriangleList | `capabilities.rs::TopologyKind` |
| Multiple meshes per file | ⚠️ GLB: yes (per primitive); OBJ: merged into one | `parsers/glb.rs`, `parsers/obj.rs` |
| **Materials** | ❌ type exists (`MaterialData`), nothing populates it | `capabilities.rs` |
| **Textures** | ❌ no type, `_images` discarded | `parsers/glb.rs` |
| **Node hierarchy / transforms** | ❌ | — |
| Per-mesh material link | ❌ | — |
| VTK | 🔒 `vtk` feature, parser not yet written (`loadfu-cnj`) | `Cargo.toml`, `ref/vtk_samples/` |

So: **UVs are done.** The work is textures → materials → hierarchy →
multi-object OBJ, in that order.

---

## Part B — Implementation steps (in order)

Each step is independently committable and keeps the suite green.
After every step: `cargo check -p loadfu`, `cargo check -p loadfu --features bevy`,
`cargo nextest run -p loadfu --features bevy`.

### Step 1 — Core types (additive, no parser changes yet)

**`capabilities.rs`** — add:

```rust
pub enum AlphaMode { Opaque, Mask, Blend }   // maps to bevy's later

// extend MaterialData:
pub emissive: [f32; 3],          // Default [0; 3]
pub alpha_mode: AlphaMode,       // Default Opaque
pub alpha_cutoff: f32,           // Default 0.5
pub double_sided: bool,          // Default false
pub base_color_texture: Option<usize>,           // index into AssetData::textures
pub normal_texture: Option<usize>,
pub metallic_roughness_texture: Option<usize>,
pub emissive_texture: Option<usize>,
```

New types:

```rust
/// Encoded image bytes — core never decodes (no `image` dependency).
pub struct TextureData {
    pub bytes: Vec<u8>,
    pub mime: TextureMime,          // Png | Jpeg  (GLB gives mimeType directly)
    pub name: Option<String>,
}

/// Flattened node tree — children are indices, meshes shared by index.
pub struct NodeData {
    pub name: Option<String>,
    /// 4×4 column-major (feeds `glam::Mat4::from_cols_array` in the bevy layer).
    pub transform: [f32; 16],       // Default: identity
    pub mesh: Option<usize>,        // index into AssetData::meshes (shared = instancing)
    pub material: Option<usize>,    // per-node override, index into materials
    pub children: Vec<usize>,       // indices into AssetData::nodes
}
```

**`capabilities.rs::AssetData`** — add `textures: Vec<TextureData>`,
`nodes: Vec<NodeData>`, `root_nodes: Vec<usize>`, `name: Option<String>`.

**`mesh_data.rs`** — add `pub material_index: Option<usize>` to `MeshData`
(all constructors default it to `None`; every parser sets it when the
format provides materials).

**Rules:**
- Everything gets a `Default`-compatible value — all 157 tests must stay
  green untouched.
- `capabilities()` on `AssetData` gains nothing new to compute yet except:
  `has_textures` should derive from `!self.textures.is_empty()` once
  populated (keep the `MaterialData::has_textures` bool in sync or drop it —
  prefer deriving; see Step 2).
- Update `docs/architecture/dependencies.md` only if you add a crate.
  **Step 1 adds none.** Textures stay encoded; do **not** add `image` (G3).

**Tests:** unit tests for `AlphaMode` default, `NodeData` identity transform,
`AssetData` with textures/nodes round-trips `Debug`/`Clone`.

### Step 2 — GLB `parse_asset` (the big one)

**`parsers/glb.rs`** — add the full entry point; keep `parse`/`parse_merged`
as thin wrappers over it:

```rust
pub fn parse_asset(bytes: &[u8]) -> Result<AssetData, GlbError>
```

Implementation notes:

- `gltf::import_slice` already returns `(document, buffers, images)` —
  the images we currently discard are your textures. Match each
  `document.materials()` texture reference through `material.pbr_metallic_roughness()`
  / `normal_texture()` / `emissive_texture()` → `texture.source().index()`
  → `images[idx]`. Embedded images have `.source() == BufferView`; URI images
  (plain `.gltf`) are the G2 case — see below.
- Materials: `pbr.base_color_factor()`, `.metallic_factor()`,
  `.roughness_factor()`, `material.emissive()` / `.alpha_mode()` /
  `.alpha_cutoff()` / `.double_sided()`.
- **Per-primitive material link is exact:** `primitive.material().index()`
  → `MeshData::material_index`. One `MeshData` per primitive already, so no
  merging ambiguity.
- **Topology (G1):** stop rejecting non-triangle modes. Map
  `Points → TopologyKind::PointList`, `Lines/LINE_STRIP → LineList`
  (strip→list conversion, or reject strips with a clear error for now),
  `Triangles/TRIANGLE_STRIP → TriangleList/TriangleStrip`. Keep
  `UnsupportedTopology` only for modes we genuinely can't represent.
- **Nodes (G4):** walk `document.default_scene().nodes()` depth-first into
  the flat `nodes` vec; `node.transform()` decomposes to a 4×4 column-major
  matrix (`Transform::to_rows`/`to_cols` from gltf — mind the convention,
  add a test with a known translation). Multiple nodes may point at the same
  `mesh.index()` — that's instancing, keep the sharing.
- **G2 — external references:** if a `.gltf` buffer/image has a URI that
  isn't a data-URI, return a new
  `GlbError::ExternalReferences { what: String }` with a message that says
  "use GLB, or the bevy loader which can resolve sibling files". Do not
  silently produce empty textures.

**Fixtures to add** (`tests/fixtures/`):
- `cube_textured.glb` — two materials, one embedded PNG texture
- `cube_hierarchy.glb` — two nodes, one parent-of-other, shared mesh (instancing)
- `cube_lines.glb` — a LINES primitive alongside triangles (G1)

**Tests:** `parses_materials`, `parses_textures_indexed`, `material_index_on_primitives`,
`node_hierarchy_and_transforms`, `instancing_shares_mesh_index`,
`lines_become_linelist`, `external_gltf_uri_is_rejected`.
Each parses a fixture, asserts on `AssetData` fields, and calls
`.validate()` / `capabilities()`.

### Step 3 — OBJ `parse_asset` + MTL

**`parsers/obj.rs`** — add `parse_asset(bytes, &ObjOptions) -> Result<AssetData, ObjError>`:

- **Stop merging objects:** tobj's `Model`s become one `MeshData` each
  (the existing per-model extraction code is already there — the merge is a
  post-step today; make it optional instead of default).
- **MTL in memory:** `tobj::parse_mtl_buf` on a companion `&[u8]` — add
  `mtl: Option<&[u8]>` to `ObjOptions` (or a second parameter). Never touch
  the filesystem (core purity rule).
- Map: `Kd` → `base_color` (tobj gives linear-ish floats; note OBJ colors
  are usually sRGB — pick one convention, document it, test it),
  `d`/`Tr` → alpha in `base_color[3]`, `map_Kd` → texture reference.
- **MTL textures are file paths, not bytes** — that's the one interaction
  between the GLB and OBJ plans. Resolution: `TextureData` needs a second
  variant. Recommended shape:

  ```rust
  pub enum TextureSource {
      /// Embedded encoded bytes (GLB).
      Embedded { bytes: Vec<u8>, mime: TextureMime },
      /// File reference — resolved by the caller (MTL map_Kd, glTF URIs).
      Uri(String),
  }
  pub struct TextureData { pub source: TextureSource, pub name: Option<String> }
  ```

  Core stays pure: it never opens the URI. The bevy layer resolves `Uri`
  via `LoadContext`; a core-only consumer that wants URIs resolved passes
  paths to its own IO. **Decide this before Step 2** so GLB and OBJ emit
  the same shape.

**Fixture:** `two_objects.mtl` + a `two_objects_textured.obj` referencing it
(by content, not by path — tests pass the MTL bytes alongside).

**Tests:** `two_objects_two_meshes`, `mtl_materials_indexed`,
`mtl_texture_is_uri`, `mtl_alpha_to_base_color`.

### Step 4 — Detection, dispatch, errors

- **`format.rs`:** add `MeshFormat::sniff(bytes) -> Option<MeshFormat>` —
  GLB magic (`0x67 0x6C 0x54 0x46` "glTF" at offset 0), PLY header
  (`b"ply"`), STL (binary heuristics / ASCII `b"solid"`), OBJ/anything-text
  heuristic (starts with `v `/`f `/`# `/`o `). Extension stays the fast
  path; sniff is the fallback. VTK joins when its parser lands.
- **`lib.rs`:** add the one-call API:

  ```rust
  pub fn parse_asset(bytes: &[u8], format: MeshFormat) -> Result<AssetData, LoadError>;
  pub fn load(bytes: &[u8], hint: Option<&Path>) -> Result<AssetData, LoadError>;
  ```

  Free functions, no `Parser` trait (G7 — five parsers with heterogeneous
  options don't earn one; this matches the closed `seaview-bdz.4` spike).
- **`error.rs`:** `LoadError` gets variants wrapping each parser error +
  `UnsupportedFormat { detected: Option<MeshFormat> }`.

**Tests:** sniffing table-driven over the existing fixtures (rename content,
not the files), `load()` end-to-end per format, round-trip
`parse → validate → capabilities` through the new entry point.

### Step 5 — Bevy layer (only when seaview needs it)

- `bevy_integration/asset_loaders.rs`: new `AssetLoader` for `LoadfuAsset`
  (manifest = the `AssetData` + label list), emitting labeled sub-assets:
  `#Mesh{n}` → `Handle<Mesh>`, `#Material{n}` → `Handle<StandardMaterial>`
  (via new `into_material.rs`), `#Texture{n}` → `Handle<Image>`
  (decode here, not in core). One handle per **mesh index**, shared across
  instanced nodes.
- `LoaderSettings { render_asset_versions / usages }` — sequence loading
  wants MAIN_WORLD-only, env objects want RENDER_WORLD; make it a setting
  (option b from the epic notes).
- Only after this: seaview deletes its `bevy_gltf` path (`znf.8`).

### Step 6 — Afterwards / parallel queue

- `loadfu-jql` FieldArray — per-vertex named data; design it against the
  extended `AssetData` so per-asset metadata has a home.
- `loadfu-cnj` VTK parser — becomes just another `parse_asset` behind `vtk`.
- `loadfu-gj7` LineStrip — fold into the G1 topology mapping when doing Step 2.

---

## Part C — Extension recipes (the general "how to add X")

### How to add a field to `MeshData` / `AssetData`

1. Add the field with a `Default`-friendly value (suite must stay green).
2. Set it in every parser that can know it; leave `None`/default elsewhere.
3. Update `AssetData::capabilities()` if it's a capability.
4. Unit tests in the same file (below a `// ---` divider), integration test
   in `tests/pipeline.rs` if it crosses modules.
5. Fixture if a new input shape is needed.

### How to add a new format parser

1. New `src/parsers/<fmt>.rs`: pure `&[u8] → Result<…>`, no IO, no bevy.
   Options struct with `Default`. Over ~300 lines → split submodules.
2. `<Fmt>Error` in `error.rs` (thiserror, wrap the crate's error type).
3. `MeshFormat::` entry in `format.rs`: `from_extension`, `sniff`,
   `static_capabilities` (conservative pre-parse defaults), `label`.
4. `parse` (geometry) + `parse_asset` (full) — or just `parse_asset` for
   formats that are inherently multi-asset.
5. Fixtures (minimal, hand-crafted) + tests: happy path per sub-format,
   error paths (every error variant), `validate()` round-trip.
6. New crate dependency? Update `docs/architecture/dependencies.md`:
   what/why/what-was-rejected. Prefer no-dependency when the format is
   simple (see PLY); prefer a crate when it's gnarly (see gltf, tobj).
7. Wire into `lib.rs::parse_asset` dispatch + `load()`.

### How to add a capability

`AssetCapabilities` static side = `MeshFormat::static_capabilities`
(conservative, format-only). Dynamic side = `AssetData::capabilities`
(precise, post-parse). A new capability needs: field + `Default`, static
entry per format, dynamic derivation, and at least one test per side.

### How to add a texture-consuming feature

Core: `TextureData` stays **encoded bytes or URI** — never decode in core.
Bevy layer: decode with bevy's image support in the loader, emit
`Handle<Image>`. If a non-bevy consumer needs pixels, that's a new optional
feature (e.g. `image`) — discuss in `dependencies.md` first.

---

## Part D — Consumer rewiring (after Steps 1–4)

- **manifold** (pipeline, core-only): `src/io/mod.rs::load_mesh()` becomes
  `loadfu::load()` + a `MeshData → manifold::mesh::Mesh` converter in its
  existing `convert.rs` style; its own GLB texture/material extraction
  (`read_glb_texture_material`) retires in favor of `AssetData`.
  Vendored copy lives at `manifold/vendor/loadfu` — **re-vendor after
  landing here** (snapshot, no `.git`), then rewire.
- **seaview** (bevy feature): after Step 5, single loader path, no
  `bevy_gltf` — see `glb-feature-parity.md` §8–9.

## Landing checklist (per step)

- [ ] `cargo check -p loadfu` clean
- [ ] `cargo check -p loadfu --features bevy` clean
- [ ] `cargo nextest run -p loadfu --features bevy` green
- [ ] New fixtures added for new input shapes
- [ ] `docs/architecture/dependencies.md` updated iff a crate was added
- [ ] `bd update loadfu-n7x --notes …` at session end, `jj describe` + push
