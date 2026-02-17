//! Format-specific mesh parsers.
//!
//! Each parser is a pure function: `&[u8] → Result<MeshData>`.
//! No IO, no filesystem access, no Bevy types.
//!
//! Parsers will be added in Epic 2:
//! - `stl` — STL (binary and ASCII) via `stl_io`
//! - `obj` — OBJ (Wavefront) via `tobj`
//! - `glb` — glTF 2.0 binary via `gltf`
