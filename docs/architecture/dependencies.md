# Dependency Guidelines

Rules and rationale for loadfu's dependency choices. Read this before adding a crate.

## Rules for Adding Dependencies

1. **Justify the weight.** Every new dep adds compile time, audit surface, and version churn. A crate must save significant code or provide correctness guarantees we can't easily match.
2. **Prefer zero-dep leaf crates.** A crate with no transitive deps (like `mint`) is almost free. A crate pulling in `nalgebra` or `syn` is not.
3. **Feature-gate optional integrations.** If a dep is only needed for interop (e.g. `mint`, `bevy`), put it behind a feature flag. Core `cargo check -p loadfu` must stay lean.
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
| `bevy` | 0.18 | **Yes** (`bevy`) | Engine integration. Gated behind `bevy` feature. Only `bevy_asset`, `bevy_render`, `bevy_mesh`. |

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
| **meshopt** 0.6 | 311K | FFI to meshoptimizer (vertex cache opt, simplification, overdraw). Processing, not data model. | ❌ Pipeline tool — useful for seaview-pipeline, not for loadfu's loading job. |
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