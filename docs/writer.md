# Writer Support — Design Plan

**Status:** Planned (not started)
**Companion:** `docs/polyphase.md` (reader design), `docs/multi-asset.md` (multi-asset plan, mostly implemented)

## Goal

Make polyphase a complete asset I/O crate: **load and save faces/vertices across
formats**. Writers are the inverse of parsers — same purity rules, same
architecture, mirrored direction:

- **Core layer:** pure functions, `&AssetData -> Vec<u8>` (or `-> impl Write`).
  No I/O, no filesystem, no Bevy.
- **Bevy layer:** unaffected — writers don't need Bevy.
- **Consumers:** manifold's `write_glb*` family becomes thin wrappers;
  seaview can export too.

## Why now

- `AssetData` is already the in-memory form of a GLB — meshes, materials,
  textures, samplers, node hierarchy. The writer is its inverse.
- The read side is done and battle-tested (multi-asset + meshopt decode).
- Manifold's hand-rolled writer (`src/io/glb.rs`, ~1,300 lines) is now the
  *only* remaining place with hand-rolled glTF knowledge, and its
  `GlbTextureMaterial` struct is shaped for a single use case (preserve
  first material + base-color texture through a rewrite). `AssetData`
  expresses the same information more completely.
- The "re-enable meshopt compression" question resolves naturally: it gets
  implemented in polyphase's writer, once, and manifold's compression question
  disappears.

## API sketch

```rust
// core — src/writers/{mod,glb,obj,ply,stl}.rs
pub fn write_glb(asset: &AssetData) -> Result<Vec<u8>, WriteError>;
pub fn write_glb_pretty(asset: &AssetData) -> Result<Vec<u8>, WriteError>; // debug-friendly JSON
#[cfg(feature = "meshopt")]
pub fn write_glb_compressed(asset: &AssetData, options: &CompressOptions) -> Result<Vec<u8>, WriteError>;

pub fn write_obj(asset: &AssetData) -> Result<Vec<u8>, WriteError>; // + MTL companion
pub fn write_mtl(asset: &AssetData) -> Result<Vec<u8>, WriteError>;
pub fn write_ply(asset: &AssetData, ascii: bool) -> Result<Vec<u8>, WriteError>;
pub fn write_stl(asset: &AssetData, ascii: bool) -> Result<Vec<u8>, WriteError>;
```

- Dispatch mirrors the reader: `MeshFormat` already knows the formats; a
  `save(asset, format) -> Vec<u8>` convenience can live next to `load()`.
- Options structs per writer (like the parsers): e.g. `GlbOptions {
  include_normals, quantize_positions, ... }`.
- Everything returns bytes; the caller does I/O. (Core stays testable.)

## GLB writer architecture

Port from manifold's `src/io/glb.rs` (proven code), restructured:

1. **Buffer builder** — accumulate binary chunk with 4-byte alignment;
   accessors registered as (offset, count, type). Manifold has this logic;
   extract it cleanly.
2. **Per-mesh accessors** — positions (required, min/max computed),
   normals, UVs, indices; topology-aware (line/point meshes get correct
   modes instead of being rejected).
3. **Materials & textures** — write every `MaterialData` with PBR fields;
   embedded textures as buffer-view images (mimeType from
   `TextureSource::Embedded`); `TextureSlot` gives (image, texCoord,
   sampler) — samplers emitted per unique `SamplerData`.
4. **Nodes** — emit the flattened hierarchy (`root_nodes`/`children`),
   transforms as TRS-decomposed or full matrices (manifold writes matrices;
   keep that), instancing preserved by node→mesh sharing.
5. **Multi-mesh** — every mesh in `AssetData` becomes a glTF mesh with one
   primitive (or merged, per options). Supersedes manifold's
   `write_glb_multi_mesh`.

### Compression (`meshopt` feature)

Port manifold's disabled `write_glb_with_meshopt` (encode
indices/positions/normals per view, EXT_meshopt_compression layout) plus
what the reader parity requires:

- Compression filters: decide whether to add OCTAHEDRAL for normals or keep
  filterless (current reader parity = filterless; start there).
- Round-trip gate: `write_glb_compressed -> parse_asset` must be
  byte-identical geometry to the input (the existing
  `quad_meshopt.glb`/reader tests become the validation harness).

## Round-trip testing strategy

The reader suite makes writer verification trivial — for each format:

```text
fixture -> parse_asset -> write_<fmt> -> parse_asset -> compare AssetData
```

- Geometry exact (lossless formats); PLY/STL ASCII writers need epsilon
  care; OBJ/MTL color-space note (parsers pass colors through; document).
- Materials/textures/samplers/hierarchy round-trip for GLB.
- Cross-format: write OBJ from a GLB-parsed asset, read back, compare
  geometry.
- Existing manifold writer tests (triangle/quad/UV round-trip) port over.

## Migration in manifold (after landing)

1. `write_glb*` become wrappers over polyphase writers (same signatures,
   gradually delegating).
2. `GlbTextureMaterial` retires once callers pass `AssetData` (or a
   `MaterialBundle` extracted from it) through the pipeline.
3. `write_stl` wraps polyphase's STL writer.
4. The open "re-enable meshopt compression in manifold" item is closed by
   this work instead.

## Non-goals

- Animations/skins, cameras, lights (reader doesn't support them either).
- Draco compression.
- Streaming/progressive writing.
- KTX2/Basis texture containers (textures stay PNG/JPEG bytes or URIs).

## Sequencing

1. **Epic: GLB writer (plain)** — buffer builder, meshes, materials,
   textures, samplers, nodes; round-trip tests.
2. **Epic: OBJ/MTL/PLY/STL writers** — small; same round-trip pattern.
3. **GLB compression** — port + filters decision + round-trip gate.
4. **Manifold migration** — wrappers, retire `GlbTextureMaterial`, close
   the compression follow-up.
5. **Rename** — crate/repo rename (see rename plan) once the API is stable,
   so dependents update once.
