# Session Context — polyphase (2026-10-14)

Imported from the seaview-side planning session. Covers: crate rename, seaview
adoption state, and the plan for the Bevy integration layer (Step 5).

## What polyphase is now

- **Renamed from loadfu → polyphase** (commit `c218002`, pushed to
  `github.com/fluid-notion-labs/polyphase`, main == origin/main, working tree clean).
- Same architecture: format-agnostic 3D asset loading; core parsers are pure
  `&[u8] → Result<AssetData>`, Bevy behind the `bevy` feature flag.
- Recent landed work: multi-asset steps 1–4 (`AssetData` with materials,
  textures, samplers, node hierarchy; `MeshData::material_index`;
  `parse_asset`/`load` top-level dispatch; `MeshFormat::sniff`), meshopt GLB
  decoding behind the `meshopt` feature, vtkio optional dep removed (`965e0f6`).
- Working design docs: `docs/polyphase.md`, `docs/multi-asset.md`,
  `docs/writer.md` (writer support design plan).

## Cross-repo state (all clean, all pushed)

| Repo | State |
|---|---|
| `nursery/polyphase` (this repo) | main `c218002`, clean, pushed |
| `seaview` | main `3773b30`, clean, pushed. loadfu/polyphase dep pinned to `965e0f6` |
| `fluidspace-pipeline/seaview/vendor/loadfu` | **dangling symlink** → `nursery/loadfu` (gone). Fix: retarget to `nursery/polyphase` or delete |

## Seaview integration — what's done (Phase 0 ✅)

- Seaview consumes polyphase via git dep
  (`fluid-notion-labs/loadfu.git`, branch main, `features = ["bevy"]`).
- `cargo update -p loadfu` repinned seaview from `c90bad8` (geometry-only era)
  → `965e0f6` (multi-asset core). Zero breakage: 529 unit tests + doc tests pass.
  Seaview commit: `58f893b build: repin loadfu to 965e0f6, drop unused meshopt dep`.
- Dropped seaview's unused `meshopt = "0.3"` dep.
- Seaview's git dep URL still says `loadfu.git` — GitHub redirects, but should
  be updated to `polyphase.git` at the next touch of seaview's Cargo.toml.
- Beads is installed in seaview (v1.3.1, DB rebuilt from issues.jsonl).

## The plan — Phase 2/3: Bevy layer for AssetData + seaview adoption

Full plan doc lives in seaview's workflow system (`plan.loadfu-adoption`); the
essentials, updated for the rename:

### Phase 2 (work happens in polyphase) — Bevy layer for AssetData

Today `bevy_integration` still only produces bare `Handle<Mesh>` (Stl/Obj/Glb/Ply
loaders); materials/textures/hierarchy exist in core only.

Decisions to make, in order:
1. **Asset type**: (a) `GlbSceneLoader` → Bevy `Scene`/`DynamicScene` (leanest for
   seaview's environment objects, parity with bevy_gltf shape) vs (b) register
   `AssetData` as a Bevy asset + app-side spawning system (more control, more
   consumer code). Lean (a).
2. **Conversions needed** (`bevy_integration/`, each <100 lines):
   `MaterialData → StandardMaterial` (alpha_mode/cutoff, double_sided,
   metallic/roughness, emissive); `TextureData` (encoded PNG/JPEG) → `Image` via
   bevy_image; sampler → `ImageSampler` from the GL consts;
   `NodeData.transform: [f32; 16]` → `Mat4::from_cols_array` (column-major — verify).
3. **OBJ MTL resolution** in the Bevy layer via load-context sibling reads
   (core takes in-memory MTL; loaders pass `mtl: None` today).
4. **`.glb` loader-override semantics**: polyphase's GlbLoader replaces
   bevy_gltf for `.glb` in seaview — check all call sites before swapping to a
   scene-producing loader.

Gates: `cargo check -p polyphase`, `cargo check -p polyphase --features bevy`,
nextest suite (194 tests, two feature configs).

### Phase 3 (work happens in seaview) — single pipeline for .glb

- `app/environment_objects.rs` currently splits: GLB/glTF → bevy_gltf scenes;
  STL/OBJ/PLY → polyphase direct-mesh loaders. Once Phase 2 lands, route `.glb`
  through polyphase; keep `.gltf` (multi-file, external URIs) on bevy_gltf.
- Only app-layer changes (`LoadedAsset` enum, `spawn_new_objects`,
  `poll_gltf_assets`); `lib/environment_objects.rs` stays pure.
- Confirm no env asset relies on bevy_gltf-only features (cameras/lights/
  animations — `NodeData` has none).

### Phase 4 (optional)

- Enable the `meshopt` feature in seaview if the FluidX3D-case → manifold
  pipeline emits `EXT_meshopt_compression` GLBs (verify manifold's exporter first).
- Adopt `MeshFormat::sniff` + `polyphase::load` in seaview's debug bins
  (cosmetic; do during a bin-touching change).

## Related issue tracking (seaview beads, now installed)

- `seaview-znf` (open, P1 epic): "loadfu: full GLB feature parity" ≈ Phase 2 —
  update its title to polyphase when touched.
- `seaview-tqr` (in_progress, P1): "loadfu: split .gltf from GlbLoader" ≈ Phase 3.
- `seaview-437` (P0, always open): seaview session context.
- Contextual (cross-machine) work items mirror this plan: epic `0040`,
  phases `9601` (closed), `3aa0`, `5ac2`, `3b71`.

## Next steps (priority order)

1. Decide Phase 2 asset-type question (Scene loader vs AssetData asset) —
   best done with a mock of seaview's environment_objects spawn flow.
2. Fix seaview `vendor/loadfu` dangling symlink (retarget or delete).
3. Update seaview dep URL `loadfu.git` → `polyphase.git` when convenient.
4. Update seaview AGENTS.md / beads issue titles from loadfu → polyphase.

## Open questions

1. Does the manifold pipeline emit meshopt-compressed GLBs? (gates Phase 4)
2. Scene loader vs AssetData asset (Phase 2 decision 1).
3. Bevy 0.18 loader-registration override semantics: confirm a later-registered
   loader for the same extension wins (polyphase plugin registers after
   DefaultPlugins; verify GlbLoader still takes `.glb`).

## Writer support — plan (full design: `docs/writer.md`)

Goal: make polyphase complete asset I/O — load **and** save. Writers mirror
parsers: core layer is pure `&AssetData -> Vec<u8>` (no I/O, no Bevy; the Bevy
layer is unaffected — writers don't need it). Manifold's hand-rolled
`src/io/glb.rs` (~1,300 lines) becomes thin wrappers; seaview can export too.
Implementing meshopt compression here once retires manifold's open
"re-enable meshopt compression" item.

API sketch: `write_glb` (+ `_pretty` for debug JSON, `_compressed` under
`meshopt`), `write_obj`/`write_mtl`, `write_ply(ascii)`, `write_stl(ascii)`;
a `save(asset, format)` dispatch next to `load()`; per-writer options structs
(e.g. `GlbOptions { include_normals, quantize_positions, .. }`). Everything
returns bytes — the caller does I/O.

Sequencing:
1. **Epic: GLB writer (plain)** — port from manifold's proven `src/io/glb.rs`,
   restructured: buffer builder (4-byte alignment, accessor registry);
   per-mesh accessors (positions w/ min-max, normals, UVs, indices;
   topology-aware line/point modes); all `MaterialData` PBR fields + embedded
   textures as buffer-view images + per-unique `SamplerData` samplers;
   flattened node hierarchy (matrices, instancing via node→mesh sharing);
   every mesh becomes a glTF mesh (supersedes `write_glb_multi_mesh`).
2. **Epic: OBJ/MTL/PLY/STL writers** — small, same pattern.
3. **GLB compression** — port manifold's disabled `write_glb_with_meshopt`;
   start filterless (matches reader parity), decide on OCTAHEDRAL for normals
   later; round-trip gate `write_glb_compressed -> parse_asset` geometry-exact
   (existing `quad_meshopt.glb` tests are the harness).
4. **Manifold migration** — `write_glb*` become wrappers over polyphase;
   retire `GlbTextureMaterial` once callers pass `AssetData`; close the
   compression follow-up. (Sequencing note: the original plan's step 5 said
   "rename after API is stable" — the rename already landed, so dependents
   updating once is already done.)

Verification strategy: round-trip everything —
`fixture -> parse_asset -> write_<fmt> -> parse_asset -> compare AssetData`.
Geometry exact on lossless formats; epsilon care for ASCII PLY/STL; OBJ/MTL
color-space pass-through documented; cross-format test (OBJ written from a
GLB-parsed asset); manifold's existing writer tests port over.

Non-goals: animations/skins, cameras, lights, Draco, streaming writes,
KTX2/Basis containers (textures stay PNG/JPEG bytes or URIs).
