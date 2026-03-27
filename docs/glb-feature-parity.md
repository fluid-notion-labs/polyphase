# GLB Feature Parity Roadmap

**Epic:** `seaview-znf` — loadfu: full GLB feature parity (except animations)

**Goal:** Make loadfu produce everything GLB supports (except animations) so
seaview can unify all env object loading through loadfu and drop the
dual-loader architecture (loadfu for STL/OBJ/PLY + `bevy_gltf` for GLB).

## Current State

loadfu's `GlbLoader` produces `Handle<Mesh>` — geometry only. The full picture:

| What exists | Where | Status |
|---|---|---|
| GLB parser (`parse`, `parse_merged`) | `src/parsers/glb.rs` | ✅ Geometry extraction works |
| `MeshData` intermediate repr | `src/mesh_data.rs` | ✅ Positions, normals, UVs, vertex colors, indices |
| `AssetData` (multi-mesh + materials) | `src/capabilities.rs` | ✅ Type exists, GLB parser doesn't produce it yet |
| `MaterialData` (PBR fields) | `src/capabilities.rs` | ✅ Type exists (base_color, metallic, roughness, has_textures) |
| `AssetCapabilities` (pre/post-parse) | `src/capabilities.rs` | ✅ Works for geometry, materials fields unused |
| `Into<Mesh> for MeshData` | `src/bevy_integration/into_mesh.rs` | ✅ Geometry only, no material conversion |
| `GlbLoader` (Bevy AssetLoader) | `src/bevy_integration/asset_loaders.rs` | ✅ Produces `Handle<Mesh>`, no materials |
| `LoadfuPlugin` | `src/bevy_integration/plugin.rs` | ✅ Registers loaders |
| Texture data in `MaterialData` | — | ❌ No texture fields beyond `has_textures: bool` |
| `MaterialData` → `StandardMaterial` | — | ❌ Not implemented |
| Scene hierarchy / transforms | — | ❌ Not captured |

### Dual-loader problem in seaview

Seaview's `environment_objects.rs` uses two code paths:

- **loadfu** for STL/OBJ/PLY → `Handle<Mesh>` → `DirectMesh` variant
- **`bevy_gltf`** for GLB → `Handle<Gltf>` → `Gltf` variant with scene spawning

The `LoadedAsset` enum has both `DirectMesh` and `Gltf` variants. The `Gltf`
path handles materials, textures, hierarchy — everything loadfu currently drops.

## Sub-tasks

### 1. GLB parser → `AssetData` (not just `Vec<MeshData>`)

**What:** Make `parsers/glb.rs` return `AssetData` with meshes + materials.

**Changes:**

- Add `parse_asset(bytes: &[u8]) -> Result<AssetData, GlbError>` to `parsers/glb.rs`
- Walk `document.materials()` and extract PBR properties into `MaterialData`
- Track which primitive uses which material (by index)
- Keep `parse()` and `parse_merged()` as convenience wrappers

**Key fields to extract from glTF materials:**

```text
base_color_factor     → MaterialData.base_color
metallic_factor       → MaterialData.metallic
roughness_factor      → MaterialData.roughness
emissive_factor       → MaterialData.emissive (new field)
alpha_mode            → MaterialData.alpha_mode (new field)
alpha_cutoff          → MaterialData.alpha_cutoff (new field)
double_sided          → MaterialData.double_sided (new field)
```

**Design note:** `parse()` and `parse_merged()` remain unchanged (geometry
only). `parse_asset()` is the new full-featured entry point. This avoids
breaking existing consumers.

### 2. Extend `MaterialData` for PBR

**What:** Add missing PBR fields to `MaterialData`.

**New fields:**

```rust
pub struct MaterialData {
    // existing
    pub name: Option<String>,
    pub base_color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    pub has_textures: bool,

    // new
    pub emissive: [f32; 3],
    pub alpha_mode: AlphaMode,
    pub alpha_cutoff: f32,
    pub double_sided: bool,

    // texture references (indices into AssetData.textures)
    pub base_color_texture: Option<usize>,
    pub normal_texture: Option<usize>,
    pub metallic_roughness_texture: Option<usize>,
    pub emissive_texture: Option<usize>,
}

pub enum AlphaMode {
    Opaque,
    Mask,
    Blend,
}
```

**This lives in core** (no Bevy dependency). The Bevy layer maps it to
`StandardMaterial`.

### 3. Texture data in core

**What:** Store raw texture image data in `AssetData`.

**New type:**

```rust
/// Raw image data extracted from an asset.
pub struct TextureData {
    /// Raw image bytes (PNG, JPEG, or raw RGBA).
    pub bytes: Vec<u8>,
    /// Image format hint.
    pub format: TextureFormat,
    /// Label for asset sub-path (e.g. "texture0").
    pub label: Option<String>,
}

pub enum TextureFormat {
    Png,
    Jpeg,
    RawRgba8 { width: u32, height: u32 },
}
```

**Add to `AssetData`:**

```rust
pub struct AssetData {
    pub meshes: Vec<MeshData>,
    pub materials: Vec<MaterialData>,
    pub textures: Vec<TextureData>,  // NEW
}
```

Materials reference textures by index into this vec. The GLB parser extracts
embedded images from `gltf::import_slice` (the `_images` return value we
currently discard).

### 4. Per-mesh material index

**What:** Track which material a mesh uses.

**Option A — field on `MeshData`:**

```rust
pub struct MeshData {
    // ... existing fields ...
    pub material_index: Option<usize>,  // index into AssetData.materials
}
```

**Option B — separate mapping in `AssetData`:**

```rust
pub struct AssetData {
    pub meshes: Vec<MeshData>,
    pub materials: Vec<MaterialData>,
    pub textures: Vec<TextureData>,
    pub mesh_materials: Vec<Option<usize>>,  // parallel to meshes
}
```

Option A is simpler and keeps the association co-located. Recommended.

### 5. `MaterialData` → Bevy `StandardMaterial`

**What:** Bevy integration layer converts `MaterialData` to `StandardMaterial`.

**Where:** New file `src/bevy_integration/into_material.rs`

**Mapping:**

```text
MaterialData.base_color         → StandardMaterial.base_color
MaterialData.metallic           → StandardMaterial.metallic
MaterialData.roughness          → StandardMaterial.perceptual_roughness
MaterialData.emissive           → StandardMaterial.emissive
MaterialData.alpha_mode         → StandardMaterial.alpha_mode
MaterialData.alpha_cutoff       → StandardMaterial.alpha_cutoff
MaterialData.double_sided       → StandardMaterial.double_sided
MaterialData.base_color_texture → StandardMaterial.base_color_texture (Handle<Image>)
MaterialData.normal_texture     → StandardMaterial.normal_map_texture
MaterialData.*_texture          → corresponding StandardMaterial fields
```

Textures require loading `TextureData.bytes` into Bevy's `Assets<Image>` to get
`Handle<Image>`. This happens in the `AssetLoader` impl, which has access to
`LoadContext`.

### 6. Scene-level asset output

**What:** New `AssetLoader` impl that produces a scene or bundle, not just
`Handle<Mesh>`.

**Design decision:** Custom `LoadfuScene` asset vs `Handle<Scene>`.

**Recommended: labeled sub-assets approach.**

The `GlbLoader` already uses Bevy's `LoadContext`. Instead of producing a
single `Handle<Mesh>`, it can register multiple labeled sub-assets:

```text
model.glb              → LoadfuAsset (metadata: mesh count, material count)
model.glb#Mesh0        → Handle<Mesh>
model.glb#Mesh1        → Handle<Mesh>
model.glb#Material0    → Handle<StandardMaterial>
model.glb#Material1    → Handle<StandardMaterial>
model.glb#Texture0     → Handle<Image>
```

Plus a top-level `LoadfuAsset` that lists all sub-asset labels, transforms,
and hierarchy info. Seaview spawns entities by iterating the asset's manifest.

**Alternative:** Produce `Handle<Scene>` directly (like `bevy_gltf` does).
Simpler for seaview to consume but less control over entity structure.

**Decision criteria:**

- If seaview just needs to spawn the whole thing → `Handle<Scene>`
- If seaview needs per-mesh control (selection, visibility, materials) → labeled sub-assets

For env objects, labeled sub-assets with a manifest is more flexible.

### 7. Node transforms and hierarchy

**What:** Capture glTF node transforms so multi-mesh models render correctly.

**New type (core):**

```rust
pub struct NodeData {
    pub name: Option<String>,
    pub transform: [f32; 16],       // 4×4 column-major
    pub mesh_index: Option<usize>,  // index into AssetData.meshes
    pub children: Vec<usize>,       // indices into AssetData.nodes
}
```

**Add to `AssetData`:**

```rust
pub struct AssetData {
    pub meshes: Vec<MeshData>,
    pub materials: Vec<MaterialData>,
    pub textures: Vec<TextureData>,
    pub nodes: Vec<NodeData>,         // NEW
    pub root_nodes: Vec<usize>,       // NEW — scene roots
}
```

The Bevy layer converts `NodeData.transform` to `Transform` and spawns a
parent-child entity hierarchy.

### 8. Unify env object pipeline in seaview

**What:** Remove `bevy_gltf` code path from `environment_objects.rs`.

**After loadfu has sub-tasks 1–7:**

1. `LoadedAsset` enum drops the `Gltf` variant — everything is loadfu
2. `load_gltf_handle` → replaced by loadfu's `GlbLoader` (scene-level)
3. One code path: loadfu → spawn entities with meshes + materials
4. `RenderAssetUsages` handled at loader level via `GlbLoaderSettings`
5. Simplify `extract_gltf_meshes` and related plumbing

### 9. Capability-driven UI

**What:** Wire `AssetCapabilities` to seaview's UI layer.

- Hide UV/material controls when format doesn't support them
- Show material properties panel when materials are present
- Per-format info in environment panel

This is a seaview-side change, not a loadfu change. Depends on sub-tasks 1–2
being done so capabilities are accurate.

## Ordering

```text
Phase 1 — Core types (no Bevy changes needed)
  [2] Extend MaterialData for PBR
  [3] TextureData type
  [4] material_index on MeshData
  [7] NodeData type

Phase 2 — GLB parser
  [1] parse_asset() → AssetData with materials, textures, nodes

Phase 3 — Bevy integration
  [5] MaterialData → StandardMaterial
  [6] Scene-level asset output (labeled sub-assets)

Phase 4 — Seaview integration
  [8] Unify env object pipeline
  [9] Capability-driven UI
```

Phase 1 is pure-core, no Bevy, easy to test. Phase 2 exercises it against real
GLB files. Phase 3 bridges to Bevy. Phase 4 is seaview-side cleanup.

## Open Design Questions

### Asset type: `Handle<Scene>` vs labeled sub-assets?

See sub-task 6. Labeled sub-assets with a manifest gives more control.
`Handle<Scene>` is simpler. Decide based on what seaview's env object system
actually needs.

### `RenderAssetUsages` strategy

From the epic notes:

- Sequence meshes use `MAIN_WORLD` only (for memory windowing — promote/demote)
- Env objects need `RENDER_WORLD` (persistent, always visible)
- **Recommended: loader settings.** `GlbLoaderSettings` already exists — add a
  `render_asset_usages: RenderAssetUsages` field. Default to
  `MAIN_WORLD | RENDER_WORLD`. Sequence loader overrides to `MAIN_WORLD` only.

### Texture pipeline: embedded vs file references?

GLB embeds images as binary blobs. The `gltf` crate's `import_slice` decodes
them into raw pixel data. Two options:

- **Embedded (recommended):** Store raw bytes in `TextureData`, register as
  Bevy `Image` assets via `LoadContext::add_labeled_asset`. No temp files,
  no extra I/O. This is what `bevy_gltf` does.
- **File references:** Write textures to disk, load via `AssetServer`. More
  complex, only useful if textures are shared across multiple assets.

Go with embedded — it matches `bevy_gltf` behavior and avoids filesystem
complexity.

## Out of Scope

- Animations / skeletal meshes
- Cameras and lights from GLB scenes
- glTF extensions beyond `KHR_texture_transform`
- Draco / meshopt decompression (separate issue)
- External `.bin` buffer files (GLB embeds everything; `.gltf` + `.bin` is a
  future concern)

## Test Strategy

### Phase 1 — Core types

Unit tests for new `MaterialData` fields, `TextureData`, `NodeData`,
`AlphaMode` defaults and constructors. Pure Rust, no fixtures needed.

### Phase 2 — GLB parser

- **Existing fixtures:** `tests/fixtures/` should have GLB files with materials.
  If not, add a small one (e.g. a colored cube with PBR material).
- Test `parse_asset()` returns correct material count, texture count, node hierarchy.
- Test material property extraction against known values.
- Test `parse()` and `parse_merged()` still work unchanged (regression).

### Phase 3 — Bevy integration

- Test `MaterialData` → `StandardMaterial` conversion (field mapping).
- Test labeled sub-asset registration (may need Bevy test harness or
  integration test).

### Phase 4 — Seaview integration

- Load a GLB env object through the unified pipeline — confirm it renders with
  correct materials.
- Verify the `Gltf` variant is removed from `LoadedAsset`.
- Confirm `RenderAssetUsages` is correct for both env objects and sequences.

## References

- `docs/loadfu.md` — original architecture doc
- `docs/architecture/` — loadfu architecture docs
- `src/parsers/glb.rs` — current GLB parser (geometry only)
- `src/capabilities.rs` — `AssetData`, `MaterialData`, `AssetCapabilities`
- `src/bevy_integration/into_mesh.rs` — current geometry-only conversion
- `src/bevy_integration/asset_loaders.rs` — current `GlbLoader`
- Seaview `docs/research/mesh_loading_ecosystem.md` — MeshSource trait, capabilities
- Seaview `docs/research/mesh_formats_comparison.md` — format feature matrix