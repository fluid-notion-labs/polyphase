# OpenVDB Research

Research notes for OpenVDB support in polyphase. Evaluates the `vdb-rs` crate,
the OpenVDB data model, and how volumetric data fits into a surface-mesh
rendering pipeline.

## What OpenVDB Is

OpenVDB is a hierarchical sparse voxel data structure developed by DreamWorks
Animation (now an Academy Software Foundation project). It stores volumetric
data — scalar fields, vector fields, level sets — in a tree structure that
is extremely memory-efficient for sparse data (e.g. a thin fluid surface in
a large domain).

**Key properties:**

- **Sparse:** Only stores voxels near the surface or in regions of interest.
  A 2048³ domain might only use 0.1% of total voxels.
- **Hierarchical:** B+-tree-like structure with configurable branching factors
  (typically 5-4-3: 32³ → 16³ → 8³ leaf nodes).
- **Typed grids:** Each grid has a value type — `float`, `Vec3f`, `bool`,
  `Int32`, etc. Multiple named grids per file.
- **Level sets:** Signed distance fields (SDF) are the primary representation
  for surfaces. The zero-crossing of the SDF defines the surface.

### File format

`.vdb` files contain:

1. **Header** — magic number, version, compression flags
2. **Grid descriptors** — name, type, transform (voxel-to-world matrix),
   metadata (class: level set / fog volume / unknown, creator, etc.)
3. **Tree data** — hierarchical node structure with leaf node value buffers
4. **Optional compression** — BLOSC, ZIP, or none

Typical file sizes: 10 MB – 10 GB per frame for production fluid sims.

## The VDB Data Model vs polyphase's MeshData

VDB and MeshData solve fundamentally different problems:

| Aspect | OpenVDB | polyphase MeshData |
|--------|---------|-----------------|
| **Representation** | Sparse voxel grid (implicit surface) | Explicit triangle mesh |
| **Surface definition** | SDF zero-crossing (continuous) | Triangle soup (discrete) |
| **Resolution** | Voxel grid resolution (e.g. 2048³) | Vertex/face count |
| **Data carried** | Per-voxel scalar/vector fields | Per-vertex positions, normals, UVs, colors |
| **Rendering** | Volume raymarching or isosurface extraction | Rasterisation (triangle pipeline) |
| **Memory pattern** | Sparse tree (random access) | Dense arrays (sequential, GPU-friendly) |

**The fundamental gap:** VDB data cannot be rendered directly by Bevy's
standard mesh pipeline. It must be converted to a triangle mesh first
(isosurface extraction) or rendered with a custom volume shader.

## Isosurface Extraction: VDB → MeshData

The standard pipeline for rendering VDB data as a surface:

```
.vdb file
  → vdb-rs: parse tree, read SDF grid
  → isosurface extraction (marching cubes / dual contouring)
  → MeshData (positions, normals, indices)
  → Bevy Mesh → GPU
```

### Marching cubes

The workhorse algorithm. Walks leaf nodes, generates triangles at SDF
zero-crossings. Well-understood, parallelisable, produces watertight meshes.

**Rust crates:**

| Crate | Downloads | Notes |
|-------|-----------|-------|
| `isosurface` 0.7 | 53K | Pure Rust. Marching cubes, dual contouring, surface nets. Generic over source function. |
| `fast_surface_nets` 0.2 | 7K | Pure Rust. Naive surface nets only, optimised for speed. |
| `marching_cubes` | 1K | Minimal, single-algorithm. |

`isosurface` is the clear choice — it's generic over any `fn(x,y,z) → f32`
source, so it can sample from a VDB grid's interpolated values.

### Performance considerations

Isosurface extraction is **expensive**. For a production fluid sim:

- A 1024³ SDF grid with ~5% active voxels ≈ 5M active voxels
- Marching cubes on the narrow band ≈ 1-5M triangles output
- Extraction time: 100ms–2s per frame (single-threaded)

This is fundamentally different from loading an STL (just read bytes → done).
The extraction step is a **compute operation**, not an I/O operation. This
has architectural implications:

1. **Not suitable for frame-rate loading.** A 500-frame VDB sequence cannot
   be extracted in real time. Pre-extraction to STL/PLY is the pragmatic path.
2. **Batch pre-processing tool.** polyphase could provide a CLI or library API:
   `vdb_to_mesh(grid, isovalue) → MeshData`. Run offline, write STL/PLY/svmesh.
3. **Single-frame env objects.** Loading one VDB as an environment object
   (terrain SDF, static obstacle) is feasible — extract once at load time.

## Rust Crate: vdb-rs

### Overview

| Property | Value |
|----------|-------|
| Crate | [`vdb-rs`](https://crates.io/crates/vdb-rs) |
| Version | 0.6.0 (Jan 2024) |
| Author | Traverse Research |
| License | MIT |
| Lines of code | ~915 Rust |
| Downloads | 41K total, ~1.2K recent |
| Repository | https://github.com/Traverse-Research/vdb-rs |

### What it does

- **Read-only** parsing of `.vdb` files (no writing)
- Parses the VDB tree structure (header, grid descriptors, node hierarchy)
- Supports float, Vec3f, bool, Int32 grid types
- Provides voxel-level access to leaf node data
- Includes a `bevy` example for rendering (voxel display, not isosurface)

### What it doesn't do

- No VDB writing
- No multi-pass I/O (`PointDataGrid` — used for particle data)
- No older OpenVDB versions (some test files fail)
- No delay loading / streaming
- No built-in isosurface extraction
- No interpolation (trilinear sampling between voxels)
- No DDA ray tracing through the tree

### API shape

```rust
use vdb_rs::VdbReader;

let mut reader = VdbReader::new(file)?;
// Iterate named grids
for grid_descriptor in reader.grid_descriptors() {
    println!("{}: {:?}", grid_descriptor.name, grid_descriptor.grid_type);
}
// Read a specific grid (typed)
let grid: Grid<f32> = reader.read_grid::<f32>("surface")?;
// Access voxel data through the tree
for leaf in grid.tree.iter_leaf_nodes() {
    // leaf.origin, leaf.values (dense array of voxel values)
}
```

### Dependencies

| Dep | Purpose | Weight |
|-----|---------|--------|
| `half` | f16 support (VDB stores half-float) | Light |
| `lz4_flex` | LZ4 decompression | Light |
| `flate2` | zlib decompression | Medium |
| `log` | Logging | Trivial |
| `bitflags` | Bit flag enums | Trivial |
| `num-traits` | Numeric traits | Light |

No heavy dependencies. The crate is lightweight for what it does.

### Maturity assessment

**Early but usable.** The 915 lines of code cover the core read path. The
Traverse Research team (known for `ash`, `gpu-allocator`, and other serious
Rust graphics crates) maintains it. Development is slow but deliberate.

**Risk:** The crate is read-only and doesn't handle all VDB files. Production
fluid sim output from Houdini or EmberGen will likely work (float/Vec3f grids),
but exotic grid types or very old VDB versions may not.

## Relevance to Seaview

### Current pipeline

FluidX3D exports surface meshes directly as **binary STL** files. The marching
cubes isosurface extraction happens inside FluidX3D (on GPU via OpenCL), and
the output is already a triangle mesh. Seaview loads these STL frames through
polyphase.

```
FluidX3D (GPU)
  → marching cubes (OpenCL kernel)
  → STL export (surface_export.hpp)
  → seaview/polyphase: STL → MeshData → Bevy Mesh
```

**VDB is not in this pipeline today.** FluidX3D doesn't export VDB files.

### Where VDB would matter

1. **Houdini / EmberGen integration.** If the fluid sim pipeline moves to
   Houdini or other tools that output VDB natively, polyphase would need to
   extract surfaces from VDB level sets.

2. **Volume rendering.** Displaying fog volumes, fire, smoke — these don't
   have a surface. They need volumetric rendering (custom Bevy shader), not
   triangle meshes. This is a completely different rendering path that Bevy
   doesn't support out of the box.

3. **Terrain SDF.** If terrain is stored as a signed distance field (e.g.
   from terrain-forge → SDF conversion), a single VDB file could represent
   the terrain as an isosurface. Extract once at load time.

4. **Pre-processing tool.** A `loadfu-cli vdb2stl input.vdb --isovalue 0.0`
   command could convert VDB sequences to STL sequences for viewing in seaview.

### Priority assessment

| Use case | Priority | Rationale |
|----------|----------|-----------|
| FluidX3D pipeline | N/A | Doesn't use VDB |
| Houdini VDB sequences | P3 | Not current workflow, but plausible future |
| Volume rendering (fog/fire) | P4 | Needs custom Bevy shader, large scope |
| Terrain SDF | P3 | Niche, one-shot extraction is feasible |
| CLI batch conversion | P3 | Useful utility, low integration effort |

**Verdict: not needed now.** VDB support is a "nice to have" for future
pipeline flexibility, not a blocker for any current workflow. When it becomes
relevant, the path is:

1. Add `vdb-rs` behind a `vdb` feature flag
2. Implement `parsers/vdb.rs` — read SDF grid → marching cubes → MeshData
3. Use `isosurface` crate for extraction
4. Gate the whole thing behind `vdb` + optional `isosurface` features

## Comparison: VDB vs VTK for Seaview

| Dimension | VTK (.vtk/.vtp/.vtu) | OpenVDB (.vdb) |
|-----------|----------------------|----------------|
| **What it stores** | Explicit geometry + fields | Implicit geometry (SDF) + volumes |
| **Pipeline fit** | Direct: parse → MeshData | Indirect: parse → extract → MeshData |
| **CFD relevance** | Universal interchange (OpenFOAM, ParaView, etc.) | Fluid sim volumes (Houdini, EmberGen) |
| **Rust crate** | `vtkio` 0.6.3 (mature, 7.5K LOC) | `vdb-rs` 0.6.0 (early, 915 LOC) |
| **polyphase priority** | P1 (in progress) | P3–P4 |
| **Extraction cost** | Cheap (read + triangulate polygons) | Expensive (marching cubes on SDF) |
| **Time series** | File-per-step (seaview's model) | File-per-step (same model) |

## Implementation Sketch (Future)

If/when VDB support is implemented:

### Cargo.toml

```toml
[dependencies]
vdb-rs = { version = "0.6", optional = true }
isosurface = { version = "0.7", optional = true }

[features]
vdb = ["dep:vdb-rs", "dep:isosurface"]
```

### MeshFormat additions

```rust
pub enum MeshFormat {
    // ... existing ...
    /// OpenVDB level set — requires isosurface extraction to produce MeshData.
    Vdb,
}
```

### Parser structure

```rust
// src/parsers/vdb.rs

/// Parse an OpenVDB file containing a level set grid, extract the isosurface.
///
/// Looks for the first float grid with class "Level Set". If none found,
/// uses the first float grid. Extracts the zero-crossing isosurface via
/// marching cubes.
pub fn parse(bytes: &[u8]) -> Result<MeshData, VdbError> {
    let reader = VdbReader::new(Cursor::new(bytes))?;
    let grid = find_level_set_grid(&reader)?;
    let mesh = extract_isosurface(&grid, /*isovalue=*/ 0.0)?;
    Ok(mesh)
}

/// Extract isosurface with configurable isovalue.
pub fn parse_with_options(bytes: &[u8], opts: &VdbOptions) -> Result<MeshData, VdbError> {
    // ...
}

pub struct VdbOptions {
    /// The isovalue at which to extract the surface. Default: 0.0.
    pub isovalue: f32,
    /// Grid name to extract (None = first level set grid).
    pub grid_name: Option<String>,
    /// Whether to compute smooth normals from the SDF gradient.
    pub gradient_normals: bool,
}
```

### Static capabilities

```rust
AssetCapabilities {
    topology: TopologyKind::TriangleList,
    has_normals: true,            // Computed from SDF gradient
    has_uvs: false,               // No UV concept in VDB
    has_vertex_colors: false,     // Could map field values → colors
    multi_object: false,          // One surface per grid
    has_materials: false,
    has_textures: false,
    parse_cost: ParseCost::High,  // Isosurface extraction is expensive
    estimated_memory: None,
    supports_mmap: false,
    supports_streaming: false,
}
```

## References

- [OpenVDB](https://www.openvdb.org/) — Official project site
- [OpenVDB File Format](https://www.openvdb.org/documentation/doxygen/io.html) — File format specification
- [vdb-rs](https://github.com/Traverse-Research/vdb-rs) — Rust VDB reader
- [isosurface](https://crates.io/crates/isosurface) — Rust marching cubes / dual contouring
- [Ken Museth, "VDB: High-Resolution Sparse Volumes with Dynamic Topology"](https://www.museth.org/Ken/Publications_files/Museth_TOG13.pdf) — Original paper
- [Academy Software Foundation OpenVDB](https://github.com/AcademySoftwareFoundation/openvdb) — Reference C++ implementation