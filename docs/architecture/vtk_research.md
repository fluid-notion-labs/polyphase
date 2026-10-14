# VTK Format Research

Research notes for VTK support in polyphase. Issue: `loadfu-cnj`.

## VTK Format Landscape

VTK (Visualization Toolkit) defines two families of file formats:

### Legacy VTK (.vtk)

Simple ASCII or binary format. Line-based header, then data sections.

```
# vtk DataFile Version 2.0
Title
ASCII|BINARY
DATASET <type>
...
```

Dataset types:
- **POLYDATA** — surface geometry (vertices, lines, polygons, triangle strips)
- **UNSTRUCTURED_GRID** — arbitrary cells (tets, hexes, triangles, mixed)
- **STRUCTURED_GRID** — curvilinear grids
- **RECTILINEAR_GRID** — axis-aligned variable spacing
- **STRUCTURED_POINTS** (ImageData) — regular grids

### XML VTK Formats

Same data model, XML container with optional base64 encoding and compression.

| Extension | Type | Description |
|-----------|------|-------------|
| `.vtp` | PolyData | Surface meshes — triangles, polygons, lines, points |
| `.vtu` | UnstructuredGrid | Volumetric — tets, hexes, wedges, mixed cells |
| `.vts` | StructuredGrid | Curvilinear grids |
| `.vtr` | RectilinearGrid | Axis-aligned variable spacing |
| `.vti` | ImageData | Regular grids (structured points) |
| `.pvtu`, `.pvtp`, etc. | Parallel | Multi-file references, split across processes |
| `.pvd` | ParaView Data | Time series collection |

## What's Relevant for polyphase

polyphase loads surface meshes into `MeshData` (indexed triangle lists). The relevant VTK types are:

### Tier 1 — Direct surface mesh (implement first)

| Format | VTK Type | Notes |
|--------|----------|-------|
| `.vtk` (POLYDATA) | Legacy PolyData | Polygons → fan-triangulate → MeshData. Most common for simple surface meshes. |
| `.vtp` | XML PolyData | Same content as legacy POLYDATA, XML container. |

### Tier 2 — Surface extraction from unstructured grids

| Format | VTK Type | Notes |
|--------|----------|-------|
| `.vtk` (UNSTRUCTURED_GRID) | Legacy UnstructuredGrid | Filter to surface cell types (Triangle=5, Quad=9, Polygon=7), skip volume cells. |
| `.vtu` | XML UnstructuredGrid | Same extraction, XML container. Common output from CFD tools (OpenFOAM, etc.). |

### Not in scope

| Format | Why |
|--------|-----|
| `.vts`, `.vtr`, `.vti` | Grid-based, not triangle meshes. Different rendering pipeline. |
| `.pvtu`, `.pvtp`, `.pvd` | Multi-file/parallel. Would need file I/O (violates pure parser contract). |
| Volume cell surface extraction | Extracting boundary faces from tet/hex meshes is a complex algorithm (shared-face detection). Belongs in a pipeline tool, not a loader. |

## VTK Cell Types

VTK `UNSTRUCTURED_GRID` uses typed cells. Relevant surface types:

| CellType | ID | Mapping to MeshData |
|----------|----|---------------------|
| `Triangle` | 5 | Direct: 3 indices → 1 triangle |
| `Quad` | 9 | Split: 4 indices → 2 triangles |
| `Polygon` | 7 | Fan-triangulate: N indices → (N-2) triangles |
| `TriangleStrip` | 6 | Unstrip: N indices → (N-2) triangles |

Volume types we'd **skip** (or warn about):

| CellType | ID | Notes |
|----------|----|-------|
| `Tetra` | 10 | 4 faces, but we'd need boundary detection |
| `Hexahedron` | 12 | 6 faces |
| `Wedge` | 13 | 5 faces |
| `Pyramid` | 14 | 5 faces |
| `Vertex` | 1 | Point cloud — different topology |
| `Line` / `PolyLine` | 3, 4 | Line rendering — different topology |

## VTK Attributes → MeshData Fields

VTK carries per-point and per-cell attributes:

| VTK Attribute | MeshData Field | Notes |
|---------------|----------------|-------|
| `NORMALS` | `normals: Option<Vec<[f32; 3]>>` | Direct mapping. Per-point normals. |
| `COLOR_SCALARS` | `vertex_colors: Option<Vec<[f32; 4]>>` | Normalize component count (3→RGBA with A=1). |
| `TEXTURE_COORDINATES` | `uvs: Option<Vec<[f32; 2]>>` | VTK allows 1D/3D tex coords; we take first 2 components. |
| `SCALARS` (named) | ❌ Not in MeshData | Pressure, temperature, etc. — visualization concern. |
| `VECTORS` | ❌ Not in MeshData | Velocity, displacement — visualization concern. |
| `TENSORS` | ❌ Not in MeshData | Stress tensors, etc. |
| Cell attributes | ❌ Skipped | Per-cell data doesn't map to per-vertex MeshData. |

**Future consideration:** A `FieldData` extension to `MeshData` or `AssetData` could carry named scalar/vector arrays for visualization coloring. Not needed for v1.

## Rust Crate: vtkio

### Overview

| Property | Value |
|----------|-------|
| Crate | [`vtkio`](https://crates.io/crates/vtkio) |
| Stable version | 0.6.3 (March 2021) |
| Latest version | 0.7.0-rc1 (July 2024) |
| Author | Egor Larionov |
| License | MIT OR Apache-2.0 |
| Lines of code | ~7,500 Rust |
| Downloads | 352K total, 133K recent |
| Repository | https://github.com/elrnv/vtkio |

### What it does

- Parses legacy VTK (.vtk) — ASCII and binary
- Parses XML VTK (.vtp, .vtu, .vts, .vtr, .vti) — with optional compression
- Unified `Vtk` data model covering both format families
- `IOBuffer` type for type-erased numeric arrays
- `PolyDataPiece`, `UnstructuredGridPiece` with topology and attributes
- Writer support (not needed for polyphase)

### Dependencies

**Default features (`xml` + `compression`):**

| Dep | Purpose | Weight concern |
|-----|---------|----------------|
| `nom` 8 | Parser combinators for legacy format | Medium — it's a parsing framework |
| `num-traits` + `num-derive` | Numeric trait derives | Light |
| `byteorder` | Endian-aware reading | Light |
| `base64` | XML binary data encoding | Light |
| `bytemuck` | Safe transmute | Already in polyphase |
| `quick-xml` | XML parsing | Medium (optional, `xml` feature) |
| `serde` | Deserialization for XML | Already in polyphase behind `bevy` |
| `flate2` | zlib compression | Medium (optional, `compression` feature) |
| `lz4_flex` | LZ4 compression | Medium (optional, `compression` feature) |
| `xz2` | LZMA compression | Heavy — C dependency (optional) |
| `log` | Logging | Light |
| `trim-in-place` | String trimming | Trivial |

**Minimal features (legacy only, no XML, no compression):**
- `nom`, `num-traits`, `num-derive`, `byteorder`, `base64`, `bytemuck`, `log`, `trim-in-place`
- This is a reasonable dep footprint for the complexity of VTK parsing.

**Recommended features for polyphase:**
- `xml` — needed for .vtp/.vtu support. Adds `quick-xml` + `serde`.
- Skip `compression` initially — most files polyphase encounters won't be compressed. Add later if needed.

### API sketch

```rust
use vtkio::Vtk;

// Parse from bytes (legacy or XML auto-detected)
let vtk = Vtk::parse_legacy_buf(&bytes)?;  // legacy .vtk
let vtk = Vtk::parse_xml(&bytes)?;          // XML .vtp/.vtu

// The main data structure
match vtk.data {
    DataSet::PolyData { pieces, .. } => {
        // pieces[0] contains points (IOBuffer) and polys (VertexNumbers)
    }
    DataSet::UnstructuredGrid { pieces, .. } => {
        // pieces[0] contains points, cells (with CellType per cell)
    }
    _ => { /* skip non-mesh types */ }
}
```

### Verdict

**Use vtkio.** VTK is far too complex to hand-roll — the legacy format alone has intricate binary handling with big-endian data, and XML formats need proper XML parsing with base64-encoded binary blobs and optional compression. Unlike PLY (which we hand-rolled in ~500 lines), a VTK parser would be thousands of lines and still miss edge cases.

**Gate behind a `vtk` feature flag** to keep the default build lean.

## Hand-rolling: Not Viable

For comparison with the PLY decision (where we hand-rolled):

| Aspect | PLY | VTK |
|--------|-----|-----|
| Format variants | 3 (ASCII, binary LE, binary BE) | Legacy ASCII, legacy binary (big-endian!), XML ASCII, XML base64, XML base64+compressed |
| Header complexity | ~20 lines, line-based | Legacy: multi-section, keyword-driven. XML: full XML DOM. |
| Data types | 8 scalar types | 8+ scalar types, `IOBuffer` type erasure, `vtkIdType` |
| Topology | Flat vertex+face lists | Cells with per-cell types, offsets, strips, multiple topology arrays |
| Estimated implementation | ~500 lines (done) | 3,000-5,000 lines minimum |
| Existing crate quality | `ply-rs` adequate but unnecessary | `vtkio` essential — battle-tested, handles edge cases |

## Implementation Plan

### MeshFormat additions

```rust
pub enum MeshFormat {
    // ... existing ...
    /// VTK legacy format — auto-detects POLYDATA vs UNSTRUCTURED_GRID.
    Vtk,
    /// VTK XML PolyData format.
    Vtp,
    /// VTK XML UnstructuredGrid format.
    Vtu,
}
```

Three separate variants because the file extensions are distinct and the parsing paths differ. They share `is_vtk()` classification similar to `is_gltf()`.

### Cargo.toml

```toml
[dependencies]
vtkio = { version = "0.6", default-features = false, features = ["xml"], optional = true }

[features]
vtk = ["dep:vtkio"]
```

Skip `compression` initially. Users with compressed VTU files can add `vtkio/compression` themselves or we add it later.

Use 0.6.3 (stable) rather than 0.7.0-rc1, unless 0.7 stabilizes by implementation time.

### Parser structure

```
src/parsers/
  vtk.rs          # Unified VTK parser — delegates to vtkio, extracts MeshData
```

Single file (not three) since the extraction logic is shared — vtkio gives us a unified `Vtk` struct regardless of input format. The `parse()` function accepts bytes and auto-detects. The `parse_legacy()` and `parse_xml()` variants are explicit.

### Extraction logic (pseudocode)

```rust
pub fn parse(bytes: &[u8]) -> Result<MeshData, VtkError> {
    let vtk = detect_and_parse(bytes)?;
    match vtk.data {
        DataSet::PolyData { pieces, .. } => extract_polydata(&pieces),
        DataSet::UnstructuredGrid { pieces, .. } => extract_unstructured_grid(&pieces),
        other => Err(VtkError::UnsupportedDataset(dataset_name(&other))),
    }
}

fn extract_polydata(pieces: &[Piece<PolyDataPiece>]) -> Result<MeshData, VtkError> {
    // 1. Get points from IOBuffer → Vec<[f32; 3]>
    // 2. Get polys VertexNumbers → fan-triangulate → Vec<u32>
    // 3. Extract normals from point attributes (if present)
    // 4. Extract tex coords from point attributes (if present)
    // 5. Extract color scalars from point attributes (if present)
    // 6. Build MeshData
}

fn extract_unstructured_grid(pieces: &[Piece<UnstructuredGridPiece>]) -> Result<MeshData, VtkError> {
    // 1. Get points from IOBuffer → Vec<[f32; 3]>
    // 2. Iterate cells + cell_types:
    //    - Triangle (5) → emit 3 indices
    //    - Quad (9) → emit 6 indices (2 triangles)
    //    - Polygon (7) → fan-triangulate
    //    - Skip other types (log warning)
    // 3. Extract attributes (same as polydata)
    // 4. Build MeshData
}
```

### Sub-tasks

1. **Add `vtk` feature flag to Cargo.toml** — `vtkio` dependency, gated
2. **Add `Vtk`, `Vtp`, `Vtu` to `MeshFormat`** — extensions, static_capabilities, `is_vtk()`, `is_direct_mesh()` → true for all three
3. **Add `VtkError`** — wraps `vtkio::Error`, plus `UnsupportedDataset`, `EmptyMesh`, `NoSurfaceCells`
4. **Create `parsers/vtk.rs`** — parse bytes → vtkio `Vtk` → extract PolyData/UnstructuredGrid → MeshData
5. **Test fixtures** — copy from `ref/vtk_samples/`: `tri.vtk`, `square.vtk`, `polyEx0.vtp`, `pygmsh_ascii.vtu`
6. **Bevy integration** — `VtkLoader` (handles .vtk, .vtp, .vtu), register in LoadfuPlugin behind `vtk` + `bevy` features
7. **Integration tests** — pipeline tests for VTK formats

### Static capabilities

```rust
// Shared across Vtk/Vtp/Vtu
AssetCapabilities {
    topology: TopologyKind::TriangleList,
    has_normals: true,           // VTK can carry normals
    has_uvs: true,               // VTK can carry texture coordinates
    has_vertex_colors: true,     // VTK can carry color scalars
    multi_object: false,         // We merge pieces into one MeshData
    has_materials: false,        // No material system in VTK
    has_textures: false,         // No texture references
    parse_cost: ParseCost::Medium, // XML parsing + type conversion
    estimated_memory: None,
    supports_mmap: false,
    supports_streaming: false,
}
```

### Verification gates

```bash
# Core without VTK
cargo check -p polyphase
# Core with VTK
cargo check -p polyphase --features vtk
# Bevy without VTK
cargo check -p polyphase --features bevy
# Full
cargo check -p polyphase --features "bevy,vtk"
# Tests
cargo nextest run -p polyphase --features vtk
```

## Test Data

Reference VTK files are in `ref/vtk_samples/` (sourced from [vtkio](https://github.com/elrnv/vtkio) test assets, MIT/Apache-2.0 licensed):

| File | Type | Description |
|------|------|-------------|
| `tri.vtk` | Legacy POLYDATA | Single triangle, ASCII |
| `tri_attrib.vtk` | Legacy POLYDATA | Triangle with color scalars + texture coords |
| `tri_attrib_binary.vtk` | Legacy POLYDATA | Binary variant of tri_attrib |
| `square.vtk` | Legacy POLYDATA | Quad polygon (tests triangulation) |
| `cube.vtk` | Legacy UNSTRUCTURED_GRID | Hexahedron (volume cell — tests skip/warning) |
| `tet.vtk` | Legacy UNSTRUCTURED_GRID | Tetrahedron (volume cell) |
| `tet_test.vtk` | Legacy UNSTRUCTURED_GRID | Tet with attributes |
| `unstructured_grid_complex.vtk` | Legacy UNSTRUCTURED_GRID | Mixed cell types |
| `polyEx0.vtp` | XML PolyData | Cube as 6 quad polys with point/cell data |
| `pygmsh_ascii.vtu` | XML UnstructuredGrid | Mixed lines + triangles from meshio |

## Open Questions

1. **Feature flag naming:** `vtk` or `vtkio`? Prefer `vtk` — it's the format name, not the crate name. User-facing.

2. **Compression support:** Skip initially (`default-features = false, features = ["xml"]`). Add `vtk-compression` feature later if needed. Most simulation output files from modern tools use uncompressed or zlib.

3. **Multi-piece handling:** VTK files can have multiple pieces. Merge into one MeshData (like GLB merge mode) or error? Recommend merge — consistent with other parsers.

4. **Volume cells:** When an UNSTRUCTURED_GRID has only volume cells (all tets/hexes, no triangles), return `VtkError::NoSurfaceCells`. Don't silently return empty mesh. Log which cell types were skipped.

5. **IOBuffer type conversion:** vtkio stores points in `IOBuffer` which can be f32, f64, or integer types. We need to convert to `Vec<[f32; 3]>` — straightforward with `bytemuck` or vtkio's cast utilities.

6. **vtkio version:** Use 0.6.3 (stable). Monitor 0.7.0 for release — it's been RC since July 2024. The XML feature in 0.7.0-rc1 uses a custom `quick-xml` fork (`git` dependency), which is not ideal for a published crate.