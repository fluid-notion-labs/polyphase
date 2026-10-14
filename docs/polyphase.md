# polyphase — 3D Asset Loader for Bevy

Format-agnostic 3D asset loading. Single assets, sequences, throttled batch loading.

**Status:** Design. No code yet.

## What It Is

A Bevy ecosystem crate that abstracts mesh loading across formats (STL, OBJ, GLB, future mmap format). Orthogonal to `svmesh` (the custom zero-copy format from [mmap_loader_architecture.md](../../../docs/research/mmap_loader_architecture.md)) — svmesh is one format polyphase would support.

**Not** a Bevy fork or patch. A companion crate that works with `Assets<Mesh>` and `AssetServer`.

## Prior Research

- [mesh_loading_ecosystem.md](../../../docs/research/mesh_loading_ecosystem.md) — `MeshSource` trait design, capability querying, format landscape
- [mmap_loader_architecture.md](../../../docs/research/mmap_loader_architecture.md) — data flow analysis, copy audit, svmesh format
- [mesh-loading-bypass.md](../../../docs/research/mesh-loading-bypass.md) — direct loader architecture, bypassing Bevy's asset pipeline

## Crate Structure

Single crate, Bevy behind a feature flag. Core is useful standalone (CLI tools, pipeline).

```text
crates/polyphase/
  Cargo.toml
  src/
    lib.rs              # Public API, feature gates
    mesh_data.rs        # MeshData — intermediate repr (positions, normals, indices)
    format.rs           # MeshFormat enum, extension detection
    capabilities.rs     # MeshCapabilities, TopologyKind, ParseCost
    source.rs           # MeshSource trait
    parsers/
      mod.rs
      stl.rs            # STL bytes → MeshData (extract from seaview)
      obj.rs            # OBJ bytes → MeshData (extract from seaview)
      glb.rs            # GLB bytes → MeshData (extend from bin/ seed code, replaces bevy_gltf delegation)
      ply.rs            # PLY bytes → MeshData (hand-rolled parser, ASCII + binary LE/BE)
    bevy/               # #[cfg(feature = "bevy")] only
      mod.rs
      plugin.rs         # LoadfuPlugin — registers loaders
      asset_loaders.rs  # AssetLoader impls wrapping parsers
      into_mesh.rs      # Into<bevy::Mesh> for MeshData
```

### Cargo.toml sketch

```toml
[package]
name = "polyphase"
version = "0.1.0"
edition = "2021"

[dependencies]
stl_io = "0.7"
tobj = "4.0"
gltf = { version = "1.4", features = ["import"] }
bytemuck = { version = "1", features = ["extern_crate_alloc"] }
thiserror = "1"

bevy = { version = "0.18", default-features = false, optional = true, features = [
    "bevy_asset", "bevy_render", "bevy_mesh", "bevy_app",
] }

[features]
default = []
bevy = ["dep:bevy"]
```

### Consumer usage

```toml
# Seaview app — full Bevy integration
polyphase = { path = "../polyphase", features = ["bevy"] }

# CLI tool — just parsing
polyphase = { path = "../polyphase" }
```

## Extraction From Seaview

### What moves to polyphase

| Current location | Becomes |
|---|---|
| `lib/asset_loaders/stl_loader.rs` | `parsers/stl.rs` (pure) + `bevy/asset_loaders.rs` (AssetLoader wrapper) |
| `lib/asset_loaders/obj_loader.rs` | `parsers/obj.rs` (pure) + `bevy/asset_loaders.rs` (AssetLoader wrapper) |
| `MeshFileFormat` (in `loader/assets.rs`) | `format.rs` — `MeshFormat` |
| `load_direct_mesh_handle`, `load_gltf_handle` | `bevy/` helpers |
| `bin/debug_loader.rs` (`parse_glb_stats`) + `bin/timeline_debug.rs` (`parse_glb_frame_size`) | `parsers/glb.rs` — extend stats-only `gltf::import` code to full mesh extraction into `MeshData` |

### What stays in seaview

| Module | Why |
|---|---|
| `sequence/loader/` (queue, tracker, frame_window, assets) | Seaview-specific orchestration (timeline, memory budget, sections) |
| `sequence/discovery.rs`, `playback.rs`, `watch.rs` | Domain logic |
| `memory_budget.rs` | Seaview-specific |
| `FrameStore`, `LoaderConfig`, `AssetIndex` | Seaview resources that *use* polyphase |

### Seed code for GLB parser

`bin/debug_loader.rs::parse_glb_stats` and `bin/timeline_debug.rs::parse_glb_frame_size` already use `gltf::import` directly — walking meshes, primitives, and accessors. Currently stats-only (vertex/index counts). Extend to read position/normal/index buffer data into `MeshData`. Same structure, more accessor reads. Replaces seaview's two-phase `bevy_gltf` delegation (`load_gltf_handle` → `extract_gltf_meshes`), eliminating the two-phase pattern.

### Current parsers

| Parser | Notes |
|---|---|
| `parsers/stl.rs` | STL (binary + ASCII) via `stl_io`. Triangle soup, face normals. |
| `parsers/obj.rs` | OBJ (Wavefront) via `tobj`. Indexed mesh, optional normals/UVs. |
| `parsers/glb.rs` | glTF 2.0 binary/text via `gltf`. Multi-mesh, PBR materials. |
| `parsers/ply.rs` | PLY (Stanford Triangle Format). Hand-rolled parser — ASCII + binary LE/BE. Positions, normals, UVs, vertex colors, n-gon fan triangulation. |

### Future parsers

| Parser | Notes |
|---|---|
| `parsers/draco_glb.rs` | Draco-compressed GLB. Depends on `draco-rs` or similar C++ bridge. |
| `parsers/svmesh.rs` | mmap + bytemuck zero-copy. See [mmap_loader_architecture.md](../../../docs/research/mmap_loader_architecture.md). |
| `parsers/vtk.rs` | VTK (.vtk legacy, .vtp/.vtu XML). Unstructured grids, field data. Needs research pass. |

### The boundary

Seaview's `drain_load_queue` currently calls `load_direct_mesh_handle` / `load_gltf_handle` directly. After extraction, it calls polyphase's API instead. Everything upstream (queue management, section reconciliation, frame window) stays in seaview.

## Key Types

```rust
/// Format-agnostic mesh data. No Bevy dependency.
pub struct MeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Option<Vec<[f32; 3]>>,
    pub uvs: Option<Vec<[f32; 2]>>,
    pub vertex_colors: Option<Vec<[f32; 4]>>,
    pub indices: Vec<u32>,
}

/// Recognized 3D asset formats.
pub enum MeshFormat {
    Stl,
    Obj,
    Glb,
    Gltf,
    Ply,
    // future: DracoGlb, SvMesh, Vtk
}

/// Full asset output — geometry + materials.
pub struct AssetData {
    pub meshes: Vec<MeshData>,
    pub materials: Vec<MaterialData>,  // from GLB PBR, OBJ MTL, etc.
}

/// What polyphase knows about a loaded asset.
/// Two tiers: pre-parse (from format/header) and post-parse (from data).
pub struct AssetCapabilities {
    // Geometry
    pub topology: TopologyKind,
    pub has_normals: bool,
    pub has_uvs: bool,
    pub has_vertex_colors: bool,
    pub multi_object: bool,

    // Materials
    pub has_materials: bool,
    pub has_textures: bool,

    // Cost heuristics
    pub parse_cost: ParseCost,
    pub estimated_memory: Option<u64>,  // from header, before full parse

    // Format-specific
    pub supports_mmap: bool,
    pub supports_streaming: bool,
}

// Pre-parse: knowable from format alone ("STL never has UVs")
impl MeshFormat {
    pub fn static_capabilities(&self) -> AssetCapabilities { ... }
}

// Post-parse: from actual data ("this GLB has 3 meshes, 2 PBR materials")
impl AssetData {
    pub fn capabilities(&self) -> AssetCapabilities { ... }
}
```

See [mesh_loading_ecosystem.md](../../../docs/research/mesh_loading_ecosystem.md) for full `MeshSource` trait design and capability querying rationale.

### Capability classes

| Capability | v0.1 | Future | Notes |
|---|---|---|---|
| **Geometry** (positions, normals, UVs, indices) | ✅ | | Core. The whole point. |
| **Materials** (PBR properties, colors, textures) | ✅ | | GLB has full PBR, OBJ has MTL, STL has nothing. |
| **Multi-object** (multiple meshes per file) | ✅ | | GLB already does this. |
| **Format detection** | ✅ | | Extension + header sniffing. |
| **Capability querying** (pre-parse + post-parse) | ✅ | | Heuristics from format, details from data. |
| **Animations** | ❌ | 🔮 | glTF supports this. Not needed for visualization yet. Trait has room. |
| **Skeletons / skinning** | ❌ | 🔮 | Same — glTF capability we don't parse yet. |
| **Scene hierarchy** (node transforms, parent/child) | ❌ | 🔮 | glTF scenes have a full node tree. Overkill for flat mesh ingest. |
| **Cameras / lights** | ❌ | ❌ | glTF supports these. We won't. App-level concern. |
| **Morph targets** | ❌ | 🔮 | glTF blend shapes. Relevant for animation but not sim playback. |
| **Point clouds** | ❌ | 🔮 | Different topology. Would need `TopologyKind::PointCloud` support. |
| **Scientific formats** (VTK, VTU, NetCDF, HDF5) | ❌ | 🔮 Research needed | Sim output formats. Unstructured grids, field data, time series. Huge ecosystem — needs its own research pass. VTK tracked as `loadfu-cnj`. |
| **Volumetric / rendering** (OpenVDB, NanoVDB, USD) | ❌ | 🔮 Research needed | Not surface meshes — voxel grids, level sets, volume rendering. Different pipeline entirely (compute → texture3d or marching cubes → mesh). USD also carries scene hierarchy. |

The trait boundary (`AssetData`) has room for future classes — fields get added, parsers opt in. Don't over-engineer for them now, but don't design them out either.

**Note:** `seaview-pipeline` already exists upstream of polyphase. It ingests raw sim output (.stl triangle soup), runs analysis, simplifies geometry, and outputs indexed .glb files. polyphase doesn't need to absorb scientific/volumetric formats — it loads what the pipeline produces. The boundary is: pipeline does the heavy conversion/analysis, polyphase does the fast ingest.

```text
seaview-pipeline:  .stl triangle soup → analysis → simplification → indexed .glb
polyphase:            .glb / .stl / .obj / ... → MeshData → Bevy
```

Full integration awareness TBD — will bring in pipeline context at a later point.

## Upstream Path

1. Workspace member in seaview (`crates/polyphase/`)
2. Publish to crates.io when API stabilizes
3. Propose to Bevy org — name fits their style (`bevy_mesh_io` or similar rename at that point)

Feature flag keeps the split clean: core parsers have no engine dependency, Bevy layer is a thin wrapper. Upstream would only take the `bevy` feature contents and fold core into `bevy_mesh` or similar.

## Agentic Refactoring Skill

When an agent is tasked with extracting polyphase from seaview, use this plan. Reference the research docs for design rationale — don't re-derive.

### Context files to read first

```
docs/research/polyphase.md                          # This doc — architecture
docs/research/mesh_loading_ecosystem.md          # MeshSource trait, capabilities
docs/research/mesh-loading-bypass.md             # Direct loader design
crates/seaview/src/lib/asset_loaders/mod.rs      # Current plugin registration
crates/seaview/src/lib/asset_loaders/stl_loader.rs
crates/seaview/src/lib/asset_loaders/obj_loader.rs
crates/seaview/src/lib/sequence/loader/assets.rs # MeshFileFormat lives here
crates/seaview/src/lib/sequence/loader/mod.rs    # load_direct_mesh_handle, load_gltf_handle
crates/seaview/src/lib/sequence/loader/load_queue.rs  # drain_load_queue (consumer)
```

### Steps

1. **Create `crates/polyphase/`** — Cargo.toml, src/lib.rs, module skeleton
2. **Extract parsers** — Move STL/OBJ parsing logic (the pure `build_mesh_*` functions) into `parsers/stl.rs`, `parsers/obj.rs`. These take `&[u8]` → `Result<MeshData>`. No Bevy.
3. **Extract `MeshFormat`** — Move from `loader/assets.rs` into `format.rs`. Rename `MeshFileFormat` → `MeshFormat`.
4. **Create `MeshData`** — The intermediate representation. Add `Into<bevy::Mesh>` behind `#[cfg(feature = "bevy")]`.
5. **Wrap in AssetLoaders** — Under `bevy/asset_loaders.rs`, create `StlLoader` and `ObjLoader` that delegate to parsers. These replace seaview's versions.
6. **Create `LoadfuPlugin`** — Registers the asset loaders. Replaces seaview's `AssetLoadersPlugin`.
7. **Wire seaview** — Add `polyphase = { path = "../polyphase", features = ["bevy"] }` to seaview's Cargo.toml. Replace `use crate::lib::asset_loaders::*` with `use polyphase::*`. Update `drain_load_queue` to use polyphase's format enum.
8. **Delete `lib/asset_loaders/`** from seaview.
9. **Test** — `cargo test -p polyphase`, `cargo test -p seaview`, `cargo build --release`.

### Verification

```bash
# Core compiles without bevy
cargo check -p polyphase
# Bevy feature compiles
cargo check -p polyphase --features bevy
# Seaview still builds
cargo build -p seaview
# Tests pass
cargo test -p polyphase
cargo test -p seaview
```
