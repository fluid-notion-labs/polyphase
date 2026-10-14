# Consumer & Writer Traits — Scope and Design

**Status:** scope document, not yet implemented.
**Companions:** `docs/polyphase.md` (reader design), `docs/writer.md` (writer plan),
`docs/architecture/viz_data_types.md` (research taxonomy).

## Goal

Define how **consumers** receive mesh data from polyphase, and how **producers**
(handled by the writers) negotiate with formats about what survives a save.

Two artifacts come out of this:

1. **A consumer trait** — the contract polyphase offers to code that wants mesh
   data (renderers, exporters, pipelines, CLI tools). General enough for meshes
   (triangles, quads), textures, UVs, normals, materials, scenes — but *optional
   on almost everything*. The only mandatory part: **vertices + whatever it takes
   to make a face** (an index buffer, or a plain vertex soup).
2. **A write-capability negotiation** — the answer to two questions every user
   of the writers eventually asks:
   - *"I have this — which formats can I save it to, and what will I lose?"*
   - *"I only need this much — which formats are sufficient?"*

The reference point for scope is **meshio** (Python, ~32 format modules), the
most complete open implementation of "read and write meshes across formats".
This document surveys what it actually handles, where polyphase already matches
it, where it deliberately stops, and what the trait looks like at each stage.

---

## 1. What meshio handles (survey, meshio 5.3.5)

### 1.1 The data model

Everything meshio does is expressed through one container:

```python
Mesh(
    points,            # (N, 3) float array
    cells,             # list of CellBlock(type, connectivity)  ← the core
    point_data,        # dict[str, (N, …) array]   named per-vertex channels
    cell_data,         # dict[str, list[(M, …) array]]  named per-cell channels, per block
    field_data,        # dict[str, array]          named global values
    point_sets,        # dict[str, index array]    named vertex subsets
    cell_sets,         # dict[str, list[index array]]  named cell subsets
    gmsh_periodic,     # periodic boundary info
    info,              # format-specific leftovers
)
```

Key ideas worth stealing:

- **Cells are typed blocks, not one buffer.** `CellBlock("triangle", ...)` ,
  `CellBlock("quad", ...)`, `CellBlock("tetra", ...)` — a mesh can mix them,
  and data channels attach per block. This is how meshio represents a mesh
  that is half quads, half triangles.
- **Named channels, not fixed fields.** `point_data["temperature"]` — any
  number of arbitrary per-vertex/per-cell arrays, discovered at read time.
- **Sets are first-class.** Named subsets of points/cells survive a round trip
  (physical groups in Gmsh, element sets in Abaqus/Nastran/Exodus…).

### 1.2 Cell types: 64 of them, dimensions 0–3

| Dim | Types |
|---|---|
| 0 | `vertex` |
| 1 | `line`, `line3`–`line11` (higher-order) |
| 2 | `triangle`, `triangle6`–`triangle66`, `quad`, `quad8`–`quad121`, `polygon` (arbitrary, variable counts) |
| 3 | `tetra`, `tetra10`–`tetra286`, `hexahedron`, `hexahedron20`–`hexahedron729`, `wedge`, `pyramid`, `polyhedron` |

Two families stand out:

- **Higher-order (curved) elements** — the `triangle6`, `hexahedron27` family.
  These exist for FEM/simulation meshes. They are the bulk of the 64 types.
- **Arbitrary polygons/polyhedra** — `polygon` blocks carry variable vertex
  counts (connectivity laid out with leading counts), covering everything from
  quads to n-gons without a type per arity.

### 1.3 Formats: ~32 modules, nearly all read+write

`abaqus, ansys, avsucd, cgns, dolfin, exodus, flac3d, gmsh (2.2/4.0/4.1),
h5m, hmf, mdpa, med, medit, nastran, netgen, neuroglancer, obj, off, permas,
ply, stl, su2, svg, tecplot, tetgen, ugrid, vtk (4.2/5.1), vtu, wkt, xdmf…`

Almost every format has both a reader and a writer. Exceptions are instructive:

- **`svg` is write-only** — you can *export* a mesh as SVG for debugging, but
  not read it back. "Writer with no reader" is a legitimate format class.
- **`neuroglancer` is read-only.**
- **`cgns`, `med`, `xdmf` need HDF5** — heavy optional dependencies gated
  behind runtime availability.

### 1.4 How meshio's writers behave when data doesn't fit (important)

This is the most valuable part for our writer-negotiation design, because
meshio *doesn't* negotiate — it just loses data with varying degrees of noise:

- **STL writer:** throws away the entire mesh structure and rebuilds a
  **vertex soup** — `points[get_cells_type("triangle")]` duplicates every
  shared vertex per facet. If normals are missing it computes them; if they
  exist as per-vertex data it uses them as facet normals. Everything else
  (UVs, colors, materials, scene) is **silently dropped**.
- **OFF writer:** *warns* and *skips* all non-triangle cells (`"OFF only
  supports triangle cells. Skipping quad."`), drops all data channels.
- **OBJ writer:** does not write MTL at all — materials are dropped, and it
  encodes format specifics as magic channel names (`point_data["obj:vt"]`,
  `cell_data["obj:group_ids"]`). The multi-file problem (OBJ + MTL + textures)
  is simply dodged: single file only.
- **VTU/VTK writers:** map named data channels into the file's data arrays —
  this is the format class where named channels shine.

**Lesson:** silent loss is the default outcome when a writer just "does its
best". Polyphase can do better with an explicit, queryable plan (§4).

### 1.5 What meshio does *not* handle (our opportunity)

- **No PBR materials, no textures, no samplers.** `point_data["obj:vt"]` is as
  close as it gets. glTF's material model is entirely outside its scope.
- **No scene graph** — no nodes, no transforms, no instancing.
- **No embedded binary texture handling** as a first-class concept.
- So meshio covers the *simulation/analysis mesh* world; polyphase's GLB
  coverage already exceeds it on the *rendering/asset* side. The two overlap
  on geometry + named channels.

---

## 2. Where polyphase stands today

Current model (one mesh, one topology, fixed channels):

```rust
MeshData {
    topology: TopologyKind,          // TriangleList | TriangleStrip | LineList | PointList
    positions: Vec<[f32; 3]>,        // required
    normals:  Option<Vec<[f32; 3]>>, // fixed optional channels
    uvs:      Option<Vec<[f32; 2]>>,
    vertex_colors: Option<Vec<[f32; 4]>>,
    indices: Vec<u32>,               // required (parsers always materialize)
    material_index: Option<usize>,
}
AssetData { meshes, materials, textures, nodes, root_nodes, name }
```

Gap analysis against meshio's scenarios:

| Scenario (meshio) | polyphase today |
|---|---|
| Triangles, indexed | ✅ all formats |
| Triangle strips | ✅ GLB |
| Lines, points | ✅ GLB (reader+writer); OBJ `l` not parsed; PLY edges not parsed |
| **Quads preserved as quads** | ❌ PLY/OBJ readers fan-triangulate; GLB can't hold them; nothing round-trips a quad |
| Arbitrary n-gons | ❌ triangulated on ingest (PLY), not represented |
| Polygons with variable counts (`polygon` blocks) | ❌ |
| Mixed topology in one mesh (half quads, half tris) | ❌ one `TopologyKind` per `MeshData` |
| Higher-order elements (triangle6…) | ❌ deliberately — simulation territory (VTK epic) |
| **Named per-vertex channels** | ❌ fixed fields only (`normals`, `uvs`, `vertex_colors`) |
| **Named per-cell channels** | ❌ |
| Named sets (physical groups) | ❌ |
| Field/global data | ❌ |
| PBR materials, 4 texture slots, samplers | ✅ — **ahead of meshio** |
| Embedded textures (binary buffer) | ✅ — ahead of meshio |
| Scene graph / nodes / instancing | ✅ — ahead of meshio |
| Multiple meshes per file | ✅ `AssetData::meshes` |
| Multi-file output (OBJ+MTL+textures) | ❌ writers return one `Vec<u8>` |

The consumer trait must be designed so **none of the ❌ rows require breaking
it later**. That drives the design decisions below.

---

## 3. The consumer trait

### 3.1 Design principles

1. **Required = vertices + faces. Everything else is optional.** A consumer
   that only renders triangle soups should implement one method and be done.
2. **Adding capability never breaks consumers.** New channels (named
   attributes, sets, field data) arrive as new *defaulted* trait methods or as
   payloads of existing ones — never as new required methods.
3. **Soup is a degenerate index buffer, not a separate representation.**
   Parsers always materialize indices (even STL: soup → deduplicated + indexed
   on read). A soup-consuming consumer just reads `positions[indices[i]]`.
   Going the other way (indexed → soup) is a *writer* concern (§4).
4. **No I/O, no engine types.** The trait is a sink for value types; bytes and
   `bevy::Mesh` stay at the edges.

### 3.2 The required payload: `MeshView`

One struct carries everything a mesh needs, with optionality *inside* it:

```rust
/// One mesh, delivered to a consumer. The required contract is
/// `positions` + `topology` + `indices`; every other field is optional.
pub struct MeshView<'a> {
    /// Vertex positions. Always present, always non-empty.
    pub positions: &'a [[f32; 3]],

    /// How `indices` forms primitives.
    pub topology: TopologyKind,

    /// Primitive → vertex mapping. For soup-style consumers this is
    /// trivially sequential; for indexed consumers it may deduplicate
    /// and reference arbitrary vertices.
    pub indices: &'a [u32],

    /// Original face topology before any triangulation, when the source
    /// format preserved it (quads from OBJ/PLY). `None` = only the
    /// triangulated `indices` above is authoritative. (Phase B — see §3.5)
    pub faces: Option<FaceList<'a>>,

    // Fixed optional channels — `None` means "not in the source".
    pub normals: Option<&'a [[f32; 3]]>,
    pub uvs: Option<&'a [[f32; 2]]>,
    pub vertex_colors: Option<&'a [[f32; 4]]>,

    /// Per-mesh material binding, if the source format binds materials
    /// per primitive/object.
    pub material: Option<usize>,
}
```

`FaceList<'a>` (Phase B) preserves the source's own polygons:

```rust
/// Original polygons as vertex-index lists, e.g. a quad = [0, 1, 2, 3].
/// `indices` remains the triangulated form for consumers that need it;
/// `faces` is the lossless form for consumers that can use it.
pub struct FaceList<'a> {
    /// Flattened vertex indices per face.
    pub vertices: &'a [u32],
    /// Start offset of each face in `vertices` (len = face_count + 1).
    pub offsets: &'a [u32],
}
```

### 3.3 The trait

```rust
/// A consumer of mesh data. Only `mesh` is required; every other method
/// defaults to "ignore". polyphase calls methods in a defined order
/// (see `deliver`): metadata → textures → materials → meshes (with
/// per-mesh channels) → scene.
pub trait MeshConsumer {
    /// Required. The geometry: vertices + faces (+ optional channels
    /// that ride along in the view).
    fn mesh(&mut self, mesh: MeshView<'_>) -> Result<(), SinkError>;

    // ── Optional: asset-level extras. Default: ignore. ──────────────

    /// PBR materials. Indices in `MeshView::material` refer here.
    fn materials(&mut self, _materials: &[MaterialData]) -> Result<(), SinkError> { Ok(()) }

    /// Texture payloads (encoded bytes or URI references).
    fn textures(&mut self, _textures: &[TextureData]) -> Result<(), SinkError> { Ok(()) }

    /// Flattened scene hierarchy and root indices.
    fn scene(&mut self, _nodes: &[NodeData], _roots: &[usize]) -> Result<(), SinkError> { Ok(()) }

    /// Named attribute channels that don't map to a fixed field
    /// (Phase C): e.g. PLY arbitrary properties, glTF `_SCALAR` attrs.
    fn named_attribute(
        &mut self,
        _mesh: usize,          // which mesh it belongs to
        _target: DataTarget,   // PerVertex | PerFace
        _name: &str,
        _data: &AttributeData,
    ) -> Result<(), SinkError> { Ok(()) }

    /// Named subsets (Phase C): meshio-style point_sets/cell_sets.
    fn named_set(&mut self, _target: DataTarget, _name: &str, _indices: &[u32])
        -> Result<(), SinkError> { Ok(()) }

    /// Asset name / format-specific metadata.
    fn metadata(&mut self, _meta: &AssetMeta) -> Result<(), SinkError> { Ok(()) }
}
```

And the driver that walks an `AssetData` in a defined order:

```rust
/// Deliver an asset to a consumer. Call order is specified and stable:
/// `metadata` → `textures` → `materials` → per-mesh (`mesh`, then its
/// `named_attribute`s) → `scene` → `named_set`s.
pub fn deliver(asset: &AssetData, sink: &mut impl MeshConsumer) -> Result<(), SinkError>;
```

### 3.4 Why this shape (and the alternative we rejected)

**Chosen: push (visitor) with defaulted methods.** The user-visible contract
is literally "optional on everything except vertices + faces", which maps
one-to-one onto one required method + defaulted methods. New capability =
new defaulted method (or new `Option` field inside `MeshView`), and existing
consumers keep compiling. The `deliver` driver keeps call order deterministic
so stateful consumers (e.g. one that builds a scene graph) are reliable.

**Rejected: pull (consumer reads `&AssetData`).** Simpler today, but it couples
every consumer to `AssetData`'s exact shape — every future field (quads, named
channels, sets) becomes a compile-time break across all consumers, and
consumers must each re-implement "which fields did this format actually
provide" logic. It also can't degrade to streaming later.

**Deferred: streaming/push-parse** (feed the consumer during parsing, no
intermediate `AssetData`). Valuable for huge files but a much bigger surgery
on the parsers; the trait above doesn't preclude it — `deliver` just gains a
streaming sibling. Not in scope.

### 3.5 What this requires from the data model (phased)

| Phase | Change | Unlocks |
|---|---|---|
| **A** (now, no data-model change) | `MeshConsumer` trait over today's `MeshData`/`AssetData`; `MeshView` borrows from `MeshData`; `deliver` driver | All current consumers (Bevy `Into<bevy::Mesh>`, CLI, exporters) get one idiomatic entry point |
| **B** | `FaceList` preservation: PLY/OBJ readers keep original polygons alongside triangulated indices; GLB writer triangulates (already does); OBJ/PLY writers emit original faces | Quads survive OBJ⇄PLY round trips; consumers can rebuild ngons |
| **C** | `AttributeData` named channels (per-vertex/per-face) + sets: PLY arbitrary properties, glTF custom attributes (`_SCALAR`…), Gmsh-style groups when we grow VTK | meshio-level generality for data |
| **D** (explicitly out of scope) | Higher-order elements, field data, time series | FEM/simulation — belongs to the VTK epic (`item 87d0`), not the asset-IO trait |

---

## 4. Writing: capability negotiation

> *"I have this — what formats can I use, and what will I lose?
> I only need this — which formats qualify?"*

### 4.1 The core idea: every feature has an `Acceptance` per format

Symmetric with the reader-side `AssetCapabilities`, but about **loss**:

```rust
/// How a writer treats one feature of an asset.
pub enum Acceptance {
    /// Written intact, round-trips.
    Native,
    /// Written with a described, deterministic transformation.
    Downgrade(Downgrade),
    /// The format has nowhere to put it — the data is lost.
    Dropped,
}

pub enum Downgrade {
    /// Shared vertices duplicated per face (indexed → soup).
    Soup,
    /// n-gons fan/split into triangles (original faces not preserved).
    Triangulate,
    /// Per-vertex normals collapsed to per-facet (first vertex, or recomputed).
    FacetNormals,
    /// f32 RGBA → 8-bit RGB(A).
    QuantizeColor8,
    /// Node hierarchy flattened; instancing becomes duplicated geometry.
    FlattenHierarchy,
    /// PBR factors approximated into a Phong-style material (OBJ/MTL).
    PbrToPhong,
    /// Embedded texture bytes written as sidecar files (external refs).
    ExternalTexture,
    /// Geometry merged into one mesh (multi-mesh assets).
    MergeMeshes,
}

pub struct WriteProfile {
    /// One entry per feature the format knows about.
    pub features: BTreeMap<Feature, Acceptance>,
    /// Formats that produce more than one file (OBJ+MTL…).
    pub multi_file: bool,
    pub ascii_variant: bool,
}
```

`Feature` is a closed enum of everything an asset can carry: `TriangleIndices,
Quads, NGons, Lines, Points, Normals, Uvs, VertexColors, Materials, Textures,
EmbeddedTextures, Hierarchy, Instancing, MultipleMeshes, AssetName, …`

### 4.2 The two queries

```rust
/// "I have this — what happens if I save it as `format`?"
/// Only lists what THIS asset actually carries (not the whole feature table).
pub fn plan_write(asset: &AssetData, format: MeshFormat) -> WritePlan;

/// "I only need this much — which formats are sufficient?"
/// `Requirement` = Feature + minimum Acceptance (Native or Downgrade-ok).
pub fn formats_for(requirements: &[Requirement]) -> Vec<MeshFormat>;
```

And a policy knob on the save path:

```rust
pub enum WritePolicy {
    /// Proceed, apply downgrades, drop what's dropped.
    BestEffort,
    /// Error if the plan contains any `Dropped`.
    NoDataLoss,
    /// Error if the plan contains any `Dropped` *or* `Downgrade`.
    LosslessOnly,
}
```

This is the deliberate improvement over meshio (§1.4): instead of silently
souping your mesh, `plan_write` tells you first — *"GLB: quads → Triangulate;
OBJ+MTL: textures → ExternalTexture (3 sidecar files)"* — and the policy
decides whether that's acceptable.

### 4.3 The matrix: "I have this → which formats, what do I lose"

Per-format writer acceptance for the four formats in Epics 1–2:

| Feature | **STL** | **OBJ + MTL** | **PLY** | **GLB** |
|---|---|---|---|---|
| Triangles, indexed | Downgrade: **Soup** | Native | Native | Native |
| Quads preserved | Dropped (tri-only) | **Native** | **Native** | Downgrade: Triangulate |
| n-gons (>4 verts) | Dropped | Native | Native | Downgrade: Triangulate |
| Lines | Dropped | Native (`l`) | (edge elem — not parsed today) | Native |
| Points (point cloud) | Dropped | Dropped | **Native** (bare vertices) | Native |
| Normals | Downgrade: **FacetNormals** | Native | Native | Native |
| UVs | Dropped | Native | Non-standard ext → treat Dropped | Native |
| Vertex colors | Dropped | Non-standard ext → Dropped | Native, Downgrade: **QuantizeColor8** | Native |
| PBR materials | Dropped | Downgrade: **PbrToPhong** | Dropped | **Native** |
| Textures | Dropped | Native (external files) | Comment-only ref → Dropped | **Native** (embedded or URI) |
| Hierarchy / transforms | Dropped | Downgrade: FlattenHierarchy (groups) | Dropped | **Native** |
| Instancing | Dropped | Duplicated | Dropped | **Native** |
| Multiple meshes | Downgrade: **MergeMeshes** (one soup) | Native (objects/groups) | Native (elements) | Native |
| Binary option | Binary or ASCII | Text only | Both | Binary only (GLB); `.gltf` = JSON |

Plus format-level notes:

- **STL has no indices at all** — it is soup by definition. Every indexed mesh
  saved to STL pays the duplication cost (3× vertices per triangle, no sharing).
- **PLY colors are `uchar`** — saving f32 colors quantizes to 8 bits per
  channel (deterministic rounding, but a real fidelity loss for HDR-ish data).
- **OBJ+MTL is multi-file by design**: mesh + MTL + one image per texture.
  This collides with our `&AssetData → Vec<u8>` purity rule — see §4.5.
- **GLB is the only format that is Native across the board** — it is the
  lossless interchange format for polyphase, which is why the round-trip
  harness uses it as the hub format.

### 4.4 Worked examples

*"I have a textured PBR cube (embedded PNG, hierarchy, instancing)."*

| → format | verdict |
|---|---|
| GLB | lossless. Done. |
| OBJ+MTL | metallic/roughness → Phong approximation; embedded PNG → sidecar file; hierarchy flattened; instancing duplicated. Usable, visibly degraded. |
| PLY | materials, textures, hierarchy: **all gone**. Geometry + UVs only. |
| STL | everything but triangle positions gone, soup duplication, normals per-facet. |

*"I only need the triangle shell for a 3D-print slicer."* → STL is *sufficient*
(soup and facet normals are fine for slicing) and the smallest common
denominator. `formats_for(&[Requirement::Triangles])` lists all four.

*"I need vertex colors to survive, fidelity matters."* → GLB only
(PLY's 8-bit quantization fails the `Native` bar).
`formats_for(&[Requirement::VertexColors(Native)])` → `[Glb]`.

*"I need quads intact for a subdiv/edge-flow pipeline."* →
`formats_for(&[Requirement::Quads(Native)])` → `[Obj, Ply]`.

### 4.5 The multi-file problem (OBJ+MTL+textures)

Writers are pure: `&AssetData → Vec<u8>`. OBJ+MTL+textures cannot honestly be
one `Vec<u8>`. Options, in order of preference:

1. **Composite output** (recommended): writers that need companions return a
   structured bundle:
   ```rust
   pub enum Output {
       Single(Vec<u8>),                    // STL, PLY, GLB
       Archive { primary: Vec<u8>, companions: Vec<Companion> }, // .obj + .mtl + .png…
   }
   // Companion { name: String, data: Vec<u8> }  — caller writes files / a zip
   ```
   Purity preserved (no filesystem in core); the *caller* decides layout —
   flat files, zip, in-memory pipeline, whatever.
2. URI-only OBJ export (no embedded texture copying) — `TextureSource::Uri`
   entries pass through as filenames; `Embedded` textures error or get skipped
   per policy. Cheap but lossy for GLB→OBJ.
3. A `zip` feature — convenient for interchange, adds a dependency; can layer
   on top of (1) later without changes.

This should be decided before Epic 2 (OBJ writer), because it shapes the
writer signature for every text format with companions.

### 4.6 Reader symmetry

`AssetCapabilities` (read side) and `WriteProfile` (write side) should share
the `Feature` vocabulary so UI/tooling can show one table: *this format, as
far as polyphase is concerned, reads X and writes Y*. `static_capabilities()`
gains a write-side sibling: `MeshFormat::write_profile()`.

---

## 5. Roadmap

1. **Phase A** — `MeshConsumer` + `deliver` + `WriteProfile`/`plan_write` for
   STL/OBJ/PLY/GLB; `Acceptance` tables as data, unit-tested per writer as
   Epics 2–3 land. Small, no data-model changes. (This doc → items under the
   Writers epic `7f8e`.)
2. **Epic 2** (existing item `928b`) — OBJ/MTL/PLY/STL writers, including the
   `Output` composite decision (§4.5).
3. **Epic 3** (existing item `4dae`) — meshopt writer; `plan_write` gains
   `MeshoptCompression` as a GLB-only optional feature.
4. **Phase B** — `FaceList` quad/n-gon preservation across PLY/OBJ (readers +
   writers), with round-trip tests for quads.
5. **Phase C** — named attributes + sets; PLY arbitrary properties;
   glTF custom attributes. Consumers opt in via `named_attribute`/`named_set`.
6. **Out of scope** — higher-order elements, field data, time series:
   VTK epic (`87d0`) will need a *different* trait surface anyway
   (cell blocks, per-block data); revisit `MeshConsumer` there rather than
   warping this one.

## 6. Open questions

- **`MeshView::faces` in Phase A or B?** Shipping the field as always-`None`
  in Phase A avoids a later trait change; shipping it with the Phase B parser
  work avoids dead API. Lean: include the field in Phase A, populate in B.
- **`Downgrade::Soup` for STL: is duplication acceptable as a *default*, or
  should `write_stl` error on assets above a size threshold?** (Soup-ifying a
  10M-triangle mesh triples memory.) Probably a `StlOptions` knob.
- **Does `AttributeData` need typed variants (F32 / U8 / strings) from day
  one?** PLY properties are typed; glTF custom attributes are numeric. Lean:
  start `Vec<f32>` + a `u8`-quantized variant, extend when VTK lands.
- **Error granularity:** `SinkError` vs. letting consumers return their own
  error type via an associated type. Lean: concrete `SinkError` with a
  `Consumer(String)` variant; associated types add friction for little gain.
