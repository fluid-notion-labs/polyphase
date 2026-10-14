# Dependency Guidelines

Rules and rationale for polyphase's dependency choices. Read this before adding a crate.

## Rules for Adding Dependencies

1. **Justify the weight.** Every new dep adds compile time, audit surface, and version churn. A crate must save significant code or provide correctness guarantees we can't easily match.
2. **Prefer zero-dep leaf crates.** A crate with no transitive deps (like `mint`) is almost free. A crate pulling in `nalgebra` or `syn` is not.
3. **Feature-gate optional integrations.** If a dep is only needed for interop (e.g. `mint`, `bevy`), put it behind a feature flag. Core `cargo check -p polyphase` must stay lean.
4. **Pin to compatible ranges.** Use `"1"` not `"1.25.0"` — let the resolver pick. Exception: Bevy, which has breaking changes every minor version.
5. **Document the choice.** When adding a dep, update this file with a row in the table and a sentence on why.
6. **Update AGENTS.md if it changes architecture rules.** New dep categories (e.g. async runtime) get a bullet in the Architecture section.

## Current Dependencies

| Crate | Version | Optional? | Why |
|-------|---------|-----------|-----|
| `stl_io` | 0.8 | No | STL parsing (binary + ASCII). Small, focused, no alternatives worth considering. |
| `tobj` | 4.0 | No | OBJ parsing. Lightweight tinyobjloader port. `obj` crate is heavier. |
| `gltf` | 1.4 | No | glTF/GLB parsing. The standard Rust glTF crate. `import` feature for `import_slice`. |
| `bytemuck` | 1 | No | Safe transmute for vertex data. Essential for zero-copy paths and buffer interop. |
| `thiserror` | 2 | No | Error derive macros. Eliminates hand-rolled `Display`/`Error` impls across all error types. |
| `base64` | 0.22 | No | Decoding glTF data-URI images/buffers for the multi-asset `parse_asset` path. Tiny, standard, no alternative worth considering for a single decode call. |
| `meshopt` | 0.6 | **Yes** (`meshopt`) | Decoding `EXT_meshopt_compression` GLB buffer views (the standard glTF compression, produced by gltf-transform, Blender exporters, etc.). FFI to meshoptimizer — hence feature-gated like `vtk`. Used by the GLB parser's `decode_meshopt_views`; without the feature, compressed files produce a clear `MeshoptRequired` error. Also used by `examples/gen_meshopt_fixture.rs` to regenerate the compressed test fixture. |
| `bevy` | 0.18 | **Yes** (`bevy`) | Engine integration. Gated behind `bevy` feature. Only `bevy_asset`, `bevy_render`, `bevy_mesh`. |
| `vtkio` | 0.6.3 | **Yes** (`vtk`) | VTK format parsing (legacy + XML). Vendored at `vendor/vtkio` on branch `loadfu-vendor`. Features: `xml` only (no compression). See VTK evaluation below. |

## Math Types — `[f32; 3]` vs Named Types

**Decision: use `[f32; 3]` / `[f32; 2]` internally.**

### Crates evaluated

| Crate | What it does | Verdict |
|-------|-------------|---------|
| **mint** 0.5 (kvark) | Math interop types (`Point3`, `Vector3`). Zero deps, 381 lines, `repr(C)`. glam/nalgebra/cgmath all implement `From`/`Into` mint. | Good crate. Not needed yet — add behind `mint` feature flag if users request interop. |
| **glam** 0.32 | Fast game math (Vec3, Mat4). Used by Bevy internally. | Comes in via `bevy` feature already. Don't add as core dep — violates "core = no engine types". |
| **nalgebra** | Full linear algebra. | Way too heavy for a loading crate. |

### Rationale

- `[f32; 3]` is the universal bottom type. Every math library converts from raw arrays.
- `bytemuck`-compatible without unsafe — plain arrays are POD by definition.
- Parsers read from byte buffers into `[f32; 3]` naturally — no conversion layer.
- gltf crate uses the same pattern — accessor data comes out as raw floats.
- If mint interop is needed later, add `From<MeshData> for MintMeshData` behind a `mint` feature flag.

## Mesh Data Structures — Roll Our Own vs Existing Crates

**Decision: custom `MeshData` — flat indexed triangle list.**

### Crates evaluated (2026-02)

| Crate | Downloads | What it does | Verdict |
|-------|-----------|-------------|---------|
| **meshx** 0.7 | 194K | Mesh exchange/conversion. Deps: nalgebra, dync, flatk, ahash, ordered-float. Science/sim oriented. | ❌ Too heavy. nalgebra dep tree alone disqualifies it for a loading crate. |
| **meshopt** 0.6 | 311K | FFI to meshoptimizer (vertex cache opt, simplification, overdraw). Processing, not data model. | ❌ Pipeline tool — useful for seaview-pipeline, not for polyphase's loading job. |
| **plexus** 0.0.11 | 22K | Half-edge mesh topology. Last updated 2019. | ❌ Dead. Wrong abstraction — we need flat triangle lists, not topology traversal. |
| **genmesh** 0.6 | 545K | Mesh generation, gfx-rs ecosystem. Last updated 2018. | ❌ Dead. |

### Rationale

- No existing crate provides a lean, engine-agnostic indexed triangle list without pulling in heavy deps.
- Our `MeshData` is ~90 lines: `positions: Vec<[f32; 3]>`, `normals`, `uvs`, `indices: Vec<u32>`. Validation included.
- This is exactly the layout GPU backends want (Bevy `Mesh`, wgpu vertex buffers, etc.).
- Adding a dep for this would bring more complexity than it removes.

## Macro / Derive Crates

**Decision: `thiserror` only. No strum, derive_more, etc.**

| Crate | Evaluated | Verdict |
|-------|-----------|---------|
| **thiserror** 2 | ✅ In use | Earns its keep — eliminates `Display`/`Error` boilerplate across 5+ error enums. |
| **strum** | Considered | Would save ~10 lines on `MeshFormat` (4 variants). Not worth a proc-macro dep at this scale. Reconsider if format count reaches 10+. |
| **derive_more** | Considered | Overlaps with thiserror. No other boilerplate justifies it. |
| **cfg-if** | Noted | Useful when Bevy layer lands (Epic 3). Already a transitive dep. Add explicitly if `#[cfg]` blocks get complex. |

## VTK Parsing — `vtkio` vs Hand-rolling

> **Status:** disabled. The `vtkio` dependency and `vtk` feature were removed
> from `Cargo.toml` so checkouts don't need a vendored `../vtkio` to build.
> The evaluation below stands — re-add `vtkio` as an optional dep with a
> `vtk` feature when the VTK parser (`loadfu-cnj`) starts.

**Decision: use `vtkio`, vendored, gated behind `vtk` feature flag.**

### Crates evaluated

| Crate | What it does | Verdict |
|-------|-------------|---------|
| **vtkio** 0.6.3 (elrnv) | Full VTK I/O — legacy ASCII/binary + XML (.vtp/.vtu/.vts/.vtr/.vti). Pure Rust, 7.5K lines. Deps: nom, num-traits, num-derive, byteorder, base64, bytemuck (shared), quick-xml+serde (xml feature), compression libs (optional). 352K downloads. | ✅ **Use it.** VTK is far too complex to hand-roll. Vendored at `vendor/vtkio` on `loadfu-vendor` branch (pinned to 0.6.3). |
| Hand-rolled | — | ❌ VTK has legacy binary (big-endian), XML with base64+compression, 30+ cell types, multi-section headers. Estimated 3,000-5,000 lines. Not viable (unlike PLY which was ~500 lines). |

### Why vendored?

- 0.6.3 is the last stable release (March 2021). The 0.7.0-rc1 on master uses a custom `quick-xml` git fork — not suitable for a published crate.
- Vendoring lets us pin to a known-good state and patch if needed.
- The `loadfu-vendor` branch is based on commit `d0cfe18` (the 0.6.3 release).

### Feature configuration

- `default-features = false` — strips compression (lz4, flate2, xz2) which we don't need initially.
- `features = ["xml"]` — enables .vtp/.vtu support via `quick-xml` + `serde`.
- `serde` comes in via xml feature (shared with bevy feature's serde — no duplicate).

### Transitive deps added by `vtk` feature

| Dep | Via | Notes |
|-----|-----|-------|
| `nom` 3 | vtkio (legacy parser) | Parser combinator framework. Older version (v3). |
| `num-traits` + `num-derive` | vtkio | Numeric trait derives for CellType enum. Light. |
| `byteorder` | vtkio | Endian-aware reading. Light. |
| `base64` | vtkio | XML binary data encoding. Light. |
| `quick-xml` 0.22 | vtkio (xml feature) | XML parser. |
| `serde` | vtkio (xml feature) | Already present when `bevy` feature is active. |

### Dependency weight

Without `vtk` feature: zero impact — `vtkio` is fully optional.
With `vtk` feature: adds ~8 transitive crates. Acceptable for the complexity saved.

See `docs/architecture/vtk_research.md` for full format research and implementation plan.