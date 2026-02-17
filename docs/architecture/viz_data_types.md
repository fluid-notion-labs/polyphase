# Visualization Data Types — Research

Research on the data types that scientific visualization demands, what formats
carry them, and what loadfu needs to evolve into to be table-stakes for serious
viz work. Issue context: `loadfu-cnj` (VTK), broader than any single format.

## Why This Matters

loadfu currently models one thing: **indexed triangle-list surface meshes**
(`MeshData`). That's sufficient for loading 3D model files (STL, OBJ, GLB, PLY)
but it covers maybe 20% of what a serious visualization tool needs. CFD,
FEA, particle methods, climate science, molecular dynamics — they all produce
data that doesn't fit in `MeshData` as it stands.

The topology field we just added (`TopologyKind` on `MeshData`) is the first
crack in the "everything is triangles" assumption. This document maps out the
full landscape so we can design the right abstractions instead of bolting on
fields ad-hoc.

---

## Data Type Taxonomy

### 1. Surface Meshes (what we have)

Polygonal surfaces — the bread and butter of 3D rendering.

| Property | Current support | Notes |
|----------|----------------|-------|
| Triangle lists | ✅ | All parsers produce this |
| Triangle strips | ✅ (topology field) | VTK, GLB can carry these |
| Quads / n-gons | ✅ (fan-triangulated) | PLY, VTK, OBJ — triangulated on ingest |
| Per-vertex normals | ✅ | Optional, computed if missing |
| Per-vertex UVs | ✅ | Optional |
| Per-vertex colors | ✅ | Optional, RGBA f32 |
| Multi-mesh assets | ✅ | `AssetData.meshes: Vec<MeshData>` |

**Gap:** None for basic surface mesh ingest. This layer is solid.

### 2. Lines and Polylines

Linear features rendered as line primitives. Ubiquitous in scientific viz.

| Use case | Source | Frequency |
|----------|--------|-----------|
| **Streamlines** | CFD velocity field integration | Very common |
| **Pathlines / streaklines** | Transient flow visualization | Common |
| **Contour lines** (isolines) | Scalar field level sets on surfaces | Common |
| **Mesh edges / wireframes** | Debug, mesh quality inspection | Universal |
| **Feature lines** | Ridge/valley detection on surfaces | Specialized |
| **Boundary edges** | Domain boundaries, region interfaces | Common |
| **Measurement / annotation lines** | User-placed rulers, cross-sections | Common |
| **Particle trajectories** | Lagrangian tracking over time | Common |

**Format support:** VTK (LINES section in POLYDATA, PolyLine cells in
UNSTRUCTURED_GRID), Ensight, Tecplot, CGNS (discrete line elements).

**What loadfu needs:**
- `TopologyKind::LineList` — already defined ✅
- `TopologyKind::LineStrip` — **missing**, needed for polylines (VTK polylines
  are N-connected-vertices, not pairs). Could convert to LineList on ingest
  (doubling indices) or add native strip support.
- Per-vertex scalar data on lines (e.g. velocity magnitude along a streamline
  for color mapping) — **not in MeshData today**.

**Design decision:** Convert polylines to LineList on ingest (like we
fan-triangulate polygons) or add `LineStrip`? LineList is simpler and maps
directly to GPU. Polylines with thousands of points would double the index
count but that's rarely a bottleneck for lines.

### 3. Point Clouds / Particles

Unconnected point data — no topology, just positions + attributes.

| Use case | Source | Frequency |
|----------|--------|-----------|
| **SPH particles** | Smoothed Particle Hydrodynamics | Very common in fluid sim |
| **DEM particles** | Discrete Element Method (granular) | Common |
| **LiDAR scans** | Point cloud scanners | Common (PLY, LAS/LAZ) |
| **Molecular dynamics** | Atom positions | Specialized |
| **Particle seeds** | Streamline/pathline starting points | Common |
| **Scatter plots** | 3D data visualization | Common |
| **Lagrangian particles** | Tracer particles in CFD | Common |

**Format support:** VTK (VERTICES section, PointCloud cell type), PLY (vertex-
only, no faces), HDF5 (arbitrary layout), LAS/LAZ (LiDAR-specific), XYZ
(trivial text format).

**What loadfu needs:**
- `TopologyKind::PointList` — already defined ✅
- Per-point scalars for color mapping (radius, temperature, velocity magnitude)
- Per-point size/radius for rendering (SPH kernel radius, atom radius)
- MeshData currently requires non-empty `indices` — for point clouds, indices
  are trivial (0..N). Could make indices optional or auto-generate.

**Scale concern:** Particle datasets are often 1M–100M+ points. Current
`Vec<[f32; 3]>` works but memory-mapped / zero-copy paths become important.

### 4. Scalar Fields (Per-Vertex / Per-Cell Data)

Named numeric arrays attached to mesh vertices or cells. This is the **biggest
gap** in loadfu today and the single most important addition for scientific viz.

| Field type | Example quantities | Typical range |
|------------|-------------------|---------------|
| **Float scalar** | Pressure, temperature, density, Mach number, wall shear stress | 1 per vertex/cell |
| **Integer scalar** | Region ID, material ID, cell type, partition number | 1 per vertex/cell |
| **Multi-component scalar** | Species concentrations (N components) | N per vertex/cell |

**How they're used:**
1. **Color mapping** — Map scalar value → colormap → vertex/fragment color. This
   is the #1 visualization technique in all of CFD/FEA. ParaView, VisIt,
   Tecplot — all fundamentally do this.
2. **Thresholding** — Show only regions where scalar > value (e.g. show only
   where temperature > 500K).
3. **Isosurface extraction** — Marching cubes/tetrahedra on a scalar field to
   produce a surface at a constant value.
4. **Annotation** — Display scalar values as text labels at selected points.

**Format support:** Every scientific format carries these — VTK (SCALARS,
FIELD_DATA), HDF5/NetCDF (datasets), CGNS (FlowSolution), Exodus
(nodal/element variables), OpenFOAM (volScalarField), Ensight (per-node/
per-element variables).

**What loadfu needs:**
A `FieldData` or `AttributeData` concept — named arrays associated with
vertices or cells. See [Proposed Data Model](#proposed-data-model) below.

### 5. Vector Fields

3-component (or 2-component in 2D) arrays representing directional quantities.

| Field type | Example quantities | Notes |
|------------|-------------------|-------|
| **Velocity** | Flow velocity (u, v, w) | The most common vector field in CFD |
| **Displacement** | Structural deformation (dx, dy, dz) | FEA |
| **Force / traction** | Surface forces | FEA boundary conditions |
| **Vorticity** | Curl of velocity | Derived field |
| **Gradient** | ∇p, ∇T — gradient of scalar fields | Derived |
| **Surface normal** | Already in MeshData as `normals` | Special case |

**How they're used:**
1. **Glyphs** — Arrows/cones at sample points showing direction + magnitude.
2. **Streamlines** — Integrate velocity field to produce line geometry (see §2).
3. **LIC** (Line Integral Convolution) — Texture-based flow visualization.
4. **Magnitude color mapping** — `|v| = sqrt(vx² + vy² + vz²)` → colormap.
5. **Component extraction** — Visualize just vx, vy, or vz as a scalar.
6. **Deformed shape** — Add displacement vectors to original positions.

**What loadfu needs:**
Same `FieldData` mechanism as scalars, but tagged as vector (3-component).
The distinction matters for: glyph rendering, streamline integration, and
correct interpolation. A vector field is not three independent scalars.

### 6. Tensor Fields

Higher-order data — matrices at each point. Less common but critical for
structural analysis.

| Field type | Components | Example |
|------------|-----------|---------|
| **Symmetric 3×3** | 6 unique (xx, yy, zz, xy, xz, yz) | Stress, strain tensors |
| **Full 3×3** | 9 | Velocity gradient tensor |
| **2×2 symmetric** | 3 | 2D stress |

**How they're used:**
1. **Eigenvalue/eigenvector extraction** — Principal stresses, principal
   directions.
2. **Tensor glyphs** — Ellipsoids, superquadrics showing principal directions
   and magnitudes.
3. **Von Mises stress** — Derived scalar from stress tensor for color mapping.
4. **Scalar invariants** — Trace, determinant, J2 invariant.

**Priority for loadfu:** Low. Most users derive scalars from tensors in the
simulation or post-processing pipeline. loadfu can carry the raw components
as a generic N-component field and let the viz layer extract what it needs.

### 7. Volume / Grid Data

Structured and unstructured volumetric data — the full 3D domain, not just
surfaces.

| Grid type | Description | Formats |
|-----------|-------------|---------|
| **Regular (uniform)** | Constant spacing, axis-aligned | VTI, HDF5, NetCDF, NIfTI, raw binary |
| **Rectilinear** | Variable spacing per axis, axis-aligned | VTR, HDF5, NetCDF |
| **Curvilinear (structured)** | Explicit point positions on an i×j×k grid | VTS, Plot3D, CGNS |
| **Unstructured** | Arbitrary connectivity (tets, hexes, wedges, mixed) | VTU, CGNS, Exodus, OpenFOAM |
| **AMR** (Adaptive Mesh Refinement) | Hierarchical multi-resolution grids | HDF5 (Enzo, FLASH, AMReX), SAMRAI |
| **Octree / KD-tree** | Spatial partitioning of points/cells | Custom formats |

**How they're used:**
1. **Volume rendering** — Ray march through 3D scalar field (medical imaging,
   CFD density).
2. **Isosurface extraction** — Marching cubes/tets to produce surface mesh from
   scalar level set.
3. **Slice planes** — Cut through the volume to show 2D cross-section with
   scalar coloring.
4. **Clip / threshold** — Show only cells meeting a criterion.
5. **Probe** — Sample field value at arbitrary points.
6. **Boundary extraction** — Extract the outer surface of a volume mesh for
   surface rendering.

**What loadfu needs:**
This is the hardest category. Volume data is a fundamentally different
representation from surface meshes. Options:

- **Option A: Boundary extraction only.** Parse VTU/CGNS, extract outer
  surface → MeshData + field data. This is what the VTK parser research
  (loadfu-cnj) scoped for Tier 2. Pragmatic but limited.
- **Option B: VolumeData type.** A new top-level type alongside MeshData for
  grid-based data. Carries the grid structure + field arrays. Consumer (Bevy
  or pipeline) decides how to render (volume ray-cast, isosurface, slice).
- **Option C: Convert-on-load.** Like Option A but with more extraction
  options — isosurface, slice, threshold — as parser-time operations.

**Recommendation:** Option A for v1 (what we already planned). Option B for
v2 when volume rendering or slice planes are needed. Don't try to be ParaView.

### 8. Time-Varying Data

Sequences of any of the above data types over time steps.

| Pattern | Description | Formats |
|---------|-------------|---------|
| **File-per-timestep** | `frame_001.vtu`, `frame_002.vtu`, ... | VTK, OpenFOAM, most CFD |
| **Multi-timestep file** | Single file with N time steps | Exodus, CGNS, HDF5, PVD (VTK collection) |
| **Adaptive topology** | Mesh connectivity changes between steps (remeshing) | OpenFOAM, AMR |
| **Static mesh + varying fields** | Same mesh, different field values per step | Common optimization |

**What loadfu needs:**
Seaview already handles file-per-timestep sequences at the orchestration layer
(sequence loader, frame window, etc.). loadfu's role is parsing individual
frames. But loadfu should be **aware** of time metadata:

- A parser returning `AssetData` could include `time_value: Option<f64>` (the
  physical time of this snapshot).
- Multi-timestep formats (Exodus, CGNS) need a way to request a specific time
  step: `parse_at_time(bytes, t)` or `parse_step(bytes, step_index)`.
- `PVD` files (VTK ParaView Data) are XML manifests pointing to per-timestep
  files — these need file I/O, which violates the pure-parser contract. Handle
  at the orchestration layer, not in loadfu core.

### 9. Multi-Block / Composite Data

Large simulations decompose domains into blocks (for parallel computation).

| Pattern | Description | Formats |
|---------|-------------|---------|
| **Multi-block** | Collection of blocks, each a separate grid | VTM (VTK), CGNS (Zone), Exodus (Block) |
| **AMR hierarchy** | Nested refinement levels | HDF5 (AMReX, Enzo) |
| **Domain decomposition** | Per-processor chunks | OpenFOAM (processor*/) |

**What loadfu needs:**
`AssetData` already holds `Vec<MeshData>` — each block becomes one entry.
For very large datasets, need lazy/streaming access (don't parse all blocks
if only rendering a subset). This connects to the mmap research (loadfu-55k).

---

## Field Data: The Critical Gap

The single most impactful addition to loadfu's data model is **named per-vertex
field arrays**. Without this, loadfu can show geometry but can't show _data on
geometry_ — which is the entire point of scientific visualization.

### What it looks like in practice

A CFD user loads a VTU file of flow around an airfoil. The file contains:
- Surface mesh (triangles) — loadfu handles this today ✅
- `pressure` — float scalar per vertex ❌
- `velocity` — float[3] vector per vertex ❌
- `temperature` — float scalar per vertex ❌
- `wallShearStress` — float[3] vector per vertex ❌
- `regionId` — int scalar per cell ❌

They want to:
1. Color the surface by pressure (scalar → colormap)
2. Show velocity arrows at sample points (vector → glyphs)
3. Switch between pressure/temperature/Mach coloring interactively
4. Threshold by regionId to isolate specific parts

All of this requires loadfu to carry the field data through to the consumer.

### What other tools do

| Tool | Field data model |
|------|-----------------|
| **VTK (C++)** | `vtkDataArray` — typed arrays attached to `vtkPointData` or `vtkCellData`. Named, typed (float/double/int), N-component. |
| **ParaView** | Same as VTK — exposes arrays in the UI for coloring, filtering. |
| **Three.js** | `BufferAttribute` — named typed arrays on `BufferGeometry`. Same concept, different API. |
| **Bevy** | `Mesh::insert_attribute(id, values)` — custom `MeshVertexAttribute` with arbitrary format. Already supports this! |
| **USD** | `primvar` — named, typed, interpolation-qualified arrays on geometry prims. |
| **glTF** | `extras` and custom accessors — limited, not designed for scientific data. |

---

## Proposed Data Model

### Option A: Fields on MeshData (recommended)

Add a field storage mechanism directly to `MeshData`:

```rust
/// A named data array associated with mesh vertices or cells.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldArray {
    /// Human-readable name (e.g. "pressure", "velocity").
    pub name: String,
    /// Where this data is attached.
    pub association: FieldAssociation,
    /// Number of components per element (1 = scalar, 3 = vector, etc.).
    pub num_components: usize,
    /// Flat data buffer: length = num_elements × num_components.
    pub data: Vec<f32>,
}

/// Where field data is defined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldAssociation {
    /// One value per vertex (interpolated across primitives).
    Vertex,
    /// One value per primitive (constant across the primitive).
    Cell,
}
```

`MeshData` gains:

```rust
pub struct MeshData {
    pub topology: TopologyKind,
    pub positions: Vec<[f32; 3]>,
    pub normals: Option<Vec<[f32; 3]>>,
    pub uvs: Option<Vec<[f32; 2]>>,
    pub vertex_colors: Option<Vec<[f32; 4]>>,
    pub indices: Vec<u32>,
    /// Named field arrays (scalars, vectors, tensors).
    /// Empty for formats that don't carry field data (STL, OBJ, GLB).
    pub fields: Vec<FieldArray>,
}
```

**Pros:**
- Simple, flat, no new top-level types.
- Existing parsers produce `fields: Vec::new()` — zero breakage.
- VTK parser populates fields from SCALARS/VECTORS/FIELD_DATA sections.
- Bevy consumer can map fields to custom `MeshVertexAttribute`s.
- CLI tools can inspect/query field names and ranges.
- `f32` storage matches GPU upload format (avoid f64→f32 conversion at render
  time).

**Cons:**
- `f32` loses precision for double-precision simulation data. But GPU rendering
  is f32 anyway — if someone needs f64 analysis, they use VTK/Python, not a
  real-time renderer.
- Cell data needs expansion to per-vertex for GPU rendering (flat shading or
  explicit duplication). This is a consumer concern, not a storage concern.
- `Vec<f32>` for everything (including integers). Could use an enum for typed
  storage, but f32 covers 99% of visualization use cases. Integer fields (region
  ID, material ID) fit in f32 for values up to 16M (2²⁴).

### Option B: Typed field storage

```rust
pub enum FieldData {
    Float32(Vec<f32>),
    Float64(Vec<f64>),
    Int32(Vec<i32>),
    UInt8(Vec<u8>),
}
```

More correct but adds complexity. The consumer needs to match on variants.
Bevy's mesh attributes are typed anyway, so this might be necessary for
correct round-tripping. **Defer to v2** — start with `Vec<f32>` and add typed
storage when a real use case demands it.

### Option C: Separate FieldDataSet type

```rust
pub struct SceneData {
    pub meshes: Vec<MeshData>,        // geometry
    pub fields: Vec<FieldDataSet>,    // attached data
    pub materials: Vec<MaterialData>, // materials
}
```

Separating fields from meshes. Over-engineered for our needs — fields are
always associated with specific geometry. Keep them together.

**Recommendation: Option A**, with `Vec<f32>` storage. Upgrade to typed
storage later if needed.

---

## Format Coverage Matrix

What data types each format can carry, and what loadfu should extract:

| Format | Surface mesh | Lines | Points | Scalar fields | Vector fields | Tensor fields | Time series | Volume grid |
|--------|:-----------:|:-----:|:------:|:-------------:|:-------------:|:-------------:|:-----------:|:-----------:|
| **STL** | ✅ | — | — | — | — | — | — | — |
| **OBJ** | ✅ | — | — | — | — | — | — | — |
| **GLB/glTF** | ✅ | ✅¹ | ✅¹ | — | — | — | ✅² | — |
| **PLY** | ✅ | — | ✅³ | ✅⁴ | — | — | — | — |
| **VTK legacy** | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | — | ✅ |
| **VTP** | ✅ | ✅ | ✅ | ✅ | ✅ | — | — | — |
| **VTU** | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | — | ✅ |
| **VTI/VTR/VTS** | — | — | — | ✅ | ✅ | ✅ | — | ✅ |
| **Exodus II** | ✅ | ✅ | — | ✅ | ✅ | ✅ | ✅ | ✅ |
| **CGNS** | ✅ | ✅ | — | ✅ | ✅ | ✅ | ✅ | ✅ |
| **OpenFOAM** | ✅ | — | — | ✅ | ✅ | ✅ | ✅ | ✅ |
| **Ensight Gold** | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **HDF5** | ✅⁵ | ✅⁵ | ✅⁵ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **NetCDF** | — | — | — | ✅ | ✅ | — | ✅ | ✅ |
| **XDMF** | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **USD** | ✅ | ✅ | ✅ | ✅⁶ | ✅⁶ | — | ✅ | ✅⁷ |

¹ glTF supports line/point primitive modes, rarely used.
² glTF has animation tracks, not simulation time series.
³ PLY vertex-only files (no face element).
⁴ PLY custom properties can carry arbitrary scalars.
⁵ HDF5 is a container — schema depends on the application (XDMF, AMReX, etc.).
⁶ USD primvars.
⁷ USD OpenVDB volumes.

---

## Implementation Roadmap

### Phase 1: Lines + Points (current)
**Status: topology field landed.**

- [x] `TopologyKind` on `MeshData` (TriangleList, TriangleStrip, LineList,
  PointList)
- [x] Topology-aware validation
- [x] Topology-aware `Into<bevy::Mesh>`
- [ ] VTK parser extracts LINES as separate `MeshData` with `LineList` topology
- [ ] VTK parser extracts VERTICES as `PointList` topology
- [ ] Consider `LineStrip` topology or convert polylines → LineList on ingest

### Phase 2: Field Data
**The critical gap. Enables scientific coloring and filtering.**

- [ ] Add `FieldArray` and `FieldAssociation` types to core
- [ ] Add `fields: Vec<FieldArray>` to `MeshData`
- [ ] VTK parser extracts SCALARS, VECTORS, NORMALS, TEXTURE_COORDINATES,
  COLOR_SCALARS, FIELD_DATA into `FieldArray`s
- [ ] PLY parser extracts arbitrary vertex properties as fields
- [ ] Bevy integration: map `FieldArray` → custom `MeshVertexAttribute`
- [ ] CLI tool: list field names, ranges, component counts

### Phase 3: Volume Data (future)
**When volume rendering or isosurface extraction is needed.**

- [ ] `GridData` type for structured grids (regular, rectilinear, curvilinear)
- [ ] `VolumeData` type or extension of `AssetData`
- [ ] VTI/VTR/VTS parser support
- [ ] Isosurface extraction (marching cubes) as a loadfu utility
- [ ] Slice plane extraction as a loadfu utility

### Phase 4: Multi-Timestep Formats (future)
**When single-file time series formats are needed.**

- [ ] `parse_step(bytes, index)` API for multi-timestep files
- [ ] `step_count(bytes)` / `time_values(bytes)` metadata queries
- [ ] Exodus II parser (NetCDF-based, multi-timestep)
- [ ] CGNS parser (HDF5-based, multi-zone + multi-timestep)

### Phase 5: Large-Scale / Streaming (future)
**When dataset size exceeds memory.**

- [ ] Memory-mapped field arrays (connects to loadfu-55k mmap research)
- [ ] Lazy block loading for multi-block datasets
- [ ] Progressive resolution (LOD) for large meshes
- [ ] Out-of-core rendering pipeline

---

## Format Priority for loadfu

Based on frequency of use in scientific visualization and seaview's target
audience (CFD, fluid simulation):

| Priority | Format | Why | Complexity |
|----------|--------|-----|------------|
| **P1** | VTK/VTP/VTU | Universal interchange, vtkio crate exists | Medium (in progress) |
| **P2** | PLY (field extraction) | Already parsed, just need to expose custom properties as fields | Low |
| **P3** | Exodus II | Standard FEA output, multi-timestep, NetCDF-based | Medium-High |
| **P3** | CGNS | Standard CFD output (OpenFOAM can export), HDF5-based | High |
| **P4** | Ensight Gold | Common CFD post-processing format, text-based | Medium |
| **P4** | XDMF | Lightweight XML manifest + HDF5 data, popular in academia | Medium |
| **P5** | OpenFOAM native | Direct OpenFOAM case reading (complex directory structure) | Very High |
| **P5** | HDF5 (generic) | Container format, needs schema knowledge per application | High |
| **P5** | USD | Emerging standard for 3D interchange, OpenVDB volumes | Very High |

---

## Key Crates in the Rust Ecosystem

| Crate | What | Maturity | Notes |
|-------|------|----------|-------|
| `vtkio` | VTK legacy + XML parsing | Stable (0.6.3) | Already evaluated, recommended |
| `hdf5` | HDF5 file access | Mature | Wraps C library, complex build. Needed for CGNS, XDMF, Exodus. |
| `netcdf` | NetCDF file access | Mature | Wraps C library. Needed for Exodus II. |
| `nom` | Parser combinators | Very mature | Used by vtkio, useful for Ensight/custom formats. |
| `quick-xml` | XML parsing | Mature | Needed for XDMF. Already pulled in by vtkio's xml feature. |
| `usd-rs` | USD bindings | Experimental | Wraps C++ pxr libraries. Very heavy dependency. |
| `ndarray` | N-dimensional arrays | Mature | Useful for structured grid data. Optional dep. |

---

## Design Principles

1. **Fields are data, not rendering.** loadfu carries field arrays as raw
   data. Color mapping, glyph generation, isosurface extraction — these are
   consumer concerns (Bevy plugin, CLI tool, pipeline). loadfu doesn't pick
   colormaps or generate arrows.

2. **f32 is the rendering type.** Simulation codes output f64, but GPU
   rendering is f32. Convert on ingest. If someone needs f64 analysis, they
   use Python/VTK, not a real-time renderer.

3. **Names are strings, semantics are conventions.** A field named "velocity"
   is just a 3-component f32 array. loadfu doesn't interpret it as a velocity —
   the consumer decides what to do with it. Semantic hints (scalar vs vector vs
   tensor) come from component count and optional metadata, not type system
   enforcement.

4. **Cell data is second class.** GPU rendering is per-vertex. Cell data needs
   expansion (duplicate vertices per cell) to render with flat shading. loadfu
   can carry cell data but the Bevy integration layer would need to expand it.
   Start by supporting vertex data only; add cell data when a real use case
   demands it.

5. **One MeshData per topology.** A VTK file with both polygons and lines
   produces two `MeshData` entries in `AssetData`. Don't mix topologies in one
   MeshData — it complicates validation, rendering, and field association.

6. **Lazy is better than eager for large data.** Don't parse all fields if the
   consumer only needs geometry. Don't parse all timesteps if the consumer only
   needs frame 42. Design APIs that allow selective loading. (This is a v2/v3
   concern but shapes the API now.)

---

## Open Questions

1. **Should `vertex_colors` stay as a dedicated field or become a FieldArray
   named "color"?** Keeping it dedicated is simpler for the common case (PLY,
   GLB) but creates an inconsistency when VTK COLOR_SCALARS arrive as fields.
   Probably keep both — `vertex_colors` for direct rendering, fields for named
   data that happens to be colors.

2. **Integer field storage.** Region IDs, material IDs, cell types — these are
   integers. Storing in `Vec<f32>` works for values up to 2²⁴ ≈ 16M, which
   covers most cases. But some Exodus/CGNS files have 64-bit integer IDs.
   Address when it's a real problem.

3. **Field metadata.** Units (Pa, m/s, K), physical quantity type, valid
   range, colormap hints. VTK doesn't carry much of this; CGNS and Exodus
   carry some. Start without metadata, add an `Option<FieldMetadata>` later.

4. **Per-face vs per-vertex normals.** Currently normals are always per-vertex.
   Some formats (STL) have per-face normals natively. We expand to per-vertex
   on ingest. This is fine for rendering but loses the original per-face
   information. For quality-of-life, could keep original face normals as a
   field.

5. **Coordinate systems.** Some formats specify coordinate handedness or up-
   axis (glTF is right-handed Y-up, VTK is right-handed Z-up). Should loadfu
   normalize to a consistent convention on ingest? Or carry the convention as
   metadata and let the consumer transform? Bevy is right-handed Y-up.

---

## References

- [VTK File Formats](https://docs.vtk.org/en/latest/design_documents/VTKFileFormats.html) — Official VTK format specification
- [VTK User's Guide: Data Model](https://vtk.org/Wiki/VTK/Tutorials/DataModel) — VTK data object hierarchy
- [Exodus II Model Description](https://sandialabs.github.io/seacas-docs/exodusII/Exodus.html) — Sandia's FEA format
- [CGNS Standard](https://cgns.github.io/) — CFD General Notation System
- [XDMF Model and Format](https://xdmf.org/index.php/XDMF_Model_and_Format) — eXtensible Data Model and Format
- [OpenFOAM File Structure](https://www.openfoam.com/documentation/user-guide/) — Case directory layout
- [USD Specification](https://openusd.org/release/spec.html) — Universal Scene Description
- [Ensight Gold Format](https://support.paraview.org/hc/en-us/articles/18944996512788-EnSight-Gold-formats) — CEI/Ansys post-processing format