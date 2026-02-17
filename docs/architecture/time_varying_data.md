# Time-Varying Data

How scientific visualization handles data that changes over time, and what
loadfu needs to support it.

## Storage Patterns

There are two fundamental patterns for storing time-varying data:

### File-per-timestep

One file per time step. The filesystem is the time index.

```
frame_0001.vtu
frame_0002.vtu
frame_0003.vtu
...
```

This is what seaview does today. The sequence loader discovers numbered files,
builds a frame list, and loads them on demand through a sliding window with
memory budgeting. loadfu parsers see individual files — no time awareness needed.

**Who uses this:** OpenFOAM (directory-per-timestep), VTK sequences, STL/OBJ/GLB
sequences from pipeline export, most CFD post-processing workflows.

**Pros:** Simple, parallelizable, random access, works with any parser.
**Cons:** Thousands of small files, redundant mesh data when topology is static,
no metadata linking steps together.

### Multi-timestep file

One file contains many (or all) time steps. Internal structure indexes by step.

```
simulation.exo    — Exodus II, NetCDF-based, all steps in one file
result.cgns       — CGNS, HDF5-based, zones × time steps
output.h5         — HDF5, application-defined layout
series.pvd        — PVD manifest (XML) pointing to per-step VTK files
```

**Who uses this:** Exodus II (FEA standard), CGNS (CFD standard), HDF5-based
codes (AMReX, Enzo, FLASH), XDMF.

**Pros:** Single file to manage, shared mesh across steps (huge space savings),
metadata (time values, variable names) in one place.
**Cons:** Needs selective parsing (can't load the whole file for one step),
harder to parallelize, file can be very large (100s of GB).

## What Varies Over Time

Not everything changes every step. Knowing what's static unlocks major
optimizations.

| What | Changes? | Notes |
|------|----------|-------|
| **Mesh topology** (connectivity) | Rarely | Only with remeshing or AMR. Most sims keep fixed connectivity. |
| **Vertex positions** | Sometimes | Structural deformation (FEA), moving meshes, ALE methods. |
| **Scalar fields** (pressure, T) | Every step | This is the primary thing that changes. |
| **Vector fields** (velocity) | Every step | Same — field values evolve, mesh stays put. |
| **Number of particles** | Every step | SPH, DEM — particles are created/destroyed. |
| **Time value** | Every step | Physical time (e.g. 0.0s, 0.001s, 0.002s). Not always uniform spacing. |

The common case in CFD/FEA: **static mesh, varying fields.** This means:
- Parse geometry once, reuse across all steps.
- Each step only needs new field arrays (pressure, velocity, etc.).
- Memory for geometry is shared; memory per step is just field data.

This is a massive optimization opportunity that file-per-timestep misses
entirely — every GLB frame re-encodes the same vertex positions and indices.

## Manifest / Collection Formats

Some formats exist purely to describe a time series of other files:

| Format | What it is |
|--------|-----------|
| **PVD** (ParaView Data) | XML file listing timestep → VTK filename pairs. ParaView's native collection format. |
| **XDMF** | XML manifest describing mesh topology + field arrays stored in HDF5 files. Time steps are XML elements pointing to HDF5 datasets. |
| **Ensight Case** | Text file listing geometry file + per-variable files + time step values. |
| **OpenFOAM `system/controlDict`** | Not a manifest per se, but the directory structure (`0/`, `0.001/`, `0.002/`) _is_ the time index. |

These are orchestration-layer concerns — loadfu wouldn't parse PVD files
directly (that requires filesystem access, violating pure-parser contract).
But loadfu should expose enough metadata that an orchestrator can build the
time index.

## What loadfu Needs

### Already handled (file-per-timestep)

Seaview's sequence loader + loadfu's pure parsers cover this today. Each frame
is an independent parse. No changes needed.

### Time metadata on AssetData

```rust
pub struct AssetData {
    pub meshes: Vec<MeshData>,
    pub materials: Vec<MaterialData>,
    /// Physical time value for this snapshot, if the format provides it.
    /// VTK files can carry TIME metadata; Exodus stores time per step.
    pub time_value: Option<f64>,
}
```

Small addition, useful for display ("t = 0.0032s") and for interpolation
between non-uniform time steps.

### Multi-timestep file API

For formats like Exodus/CGNS that pack all steps in one file, loadfu needs
a way to query and selectively parse:

```rust
/// Metadata about a multi-timestep file (cheap to extract).
pub struct TimeSeriesInfo {
    pub step_count: usize,
    pub time_values: Vec<f64>,          // physical time per step
    pub field_names: Vec<String>,        // available fields
    pub mesh_is_static: bool,            // topology constant across steps
}

/// Query time series metadata without parsing geometry/fields.
pub fn time_series_info(bytes: &[u8]) -> Result<TimeSeriesInfo, LoadError>;

/// Parse a single time step from a multi-timestep file.
pub fn parse_step(bytes: &[u8], step: usize) -> Result<AssetData, LoadError>;

/// Parse geometry only (for static-mesh optimization).
pub fn parse_geometry(bytes: &[u8]) -> Result<Vec<MeshData>, LoadError>;

/// Parse fields only at a given step (reuse geometry from parse_geometry).
pub fn parse_fields_at_step(bytes: &[u8], step: usize) -> Result<Vec<Vec<FieldArray>>, LoadError>;
```

This keeps parsers pure (`&[u8]` in, data out) while enabling the static-mesh
optimization. The orchestrator calls `parse_geometry` once, then
`parse_fields_at_step` for each frame in the window.

### Static mesh + varying fields pattern

This is the key optimization. The orchestrator (seaview) needs to know:

1. Is the mesh static across steps? → `TimeSeriesInfo::mesh_is_static`
2. Parse geometry once → cache `Vec<MeshData>` (positions, indices, normals)
3. Per step, parse only field arrays → swap field data on existing GPU mesh
4. Bevy integration: update vertex attributes without rebuilding the mesh

This turns O(N × mesh_size) memory into O(mesh_size + N × field_size).
For a 1M vertex mesh with 5 scalar fields over 1000 steps:
- File-per-timestep: 1000 × (12MB positions + 12MB normals + 4MB indices + 20MB fields) = **48 GB**
- Static mesh: 28MB geometry + 1000 × 20MB fields = **20 GB** (2.4× savings)
- And positions/normals/indices don't need re-uploading to GPU.

## Format Priority for Time Series

| Format | Time support | Rust crate | Effort |
|--------|-------------|------------|--------|
| **VTK + PVD** | PVD manifest → per-step VTK files | vtkio (have it) | Low — orchestrator reads PVD XML, feeds files to existing VTK parser |
| **Exodus II** | Native multi-step, static mesh + varying fields | `netcdf` crate (C wrapper) | Medium — need NetCDF dep, Exodus schema knowledge |
| **CGNS** | Native multi-step, zones + fields | `hdf5` crate (C wrapper) | High — complex schema, HDF5 build dependency |
| **XDMF** | XML manifest + HDF5 data | `quick-xml` + `hdf5` | Medium — XML is easy, HDF5 is the hard part |
| **Ensight Gold** | Case file + per-step files | Hand-roll or nom | Medium — text format, well-documented |

## Open Questions

1. **Should `parse_fields_at_step` return `FieldArray`s that reference the
   original geometry, or standalone?** Standalone is simpler (no lifetimes),
   but the caller must know which mesh the fields attach to.

2. **Mmap for multi-timestep files.** A 50GB Exodus file can't be loaded into
   `&[u8]`. Need `parse_step` to accept a file handle or mmap view rather than
   a byte slice. This intersects with loadfu-55k (mmap research).

3. **Non-uniform time spacing.** Simulations often have variable dt (smaller
   steps during transients, larger during steady state). The orchestrator needs
   time values to map playback position to the correct step. Linear
   interpolation between steps is the standard approach.

4. **Topology-changing sequences.** Remeshing, AMR, particle methods — the mesh
   itself changes each step. The static-mesh optimization doesn't apply. Detect
   and fall back to full per-step parsing. `TimeSeriesInfo::mesh_is_static`
   signals this.