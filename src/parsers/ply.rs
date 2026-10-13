//! PLY parser — pure `&[u8] → Result<MeshData, PlyError>`.
//!
//! Supports all three PLY sub-formats:
//! - ASCII
//! - Binary little-endian
//! - Binary big-endian
//!
//! Extracts positions, normals, UVs, vertex colors, and face indices.
//! N-gon faces are fan-triangulated to produce a triangle list.
//!
//! Hand-rolled parser — no external PLY crate dependency. The PLY format is
//! simple enough that a focused implementation for our needs (triangle mesh
//! loading) is straightforward and keeps the dep tree lean.

use crate::error::PlyError;
use crate::mesh_data::MeshData;

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

/// Options for PLY parsing.
#[derive(Debug, Clone)]
pub struct PlyOptions {
    /// If `true`, recompute smooth vertex normals from geometry instead of
    /// using normals from the PLY file (if present).
    ///
    /// Defaults to `false` (use PLY normals when available).
    pub recompute_normals: bool,
}

impl Default for PlyOptions {
    fn default() -> Self {
        Self {
            recompute_normals: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse a PLY file from bytes using default options.
///
/// Auto-detects ASCII vs binary format from the header.
///
/// # Errors
///
/// Returns [`PlyError::InvalidHeader`] if the header is malformed,
/// [`PlyError::UnsupportedFormat`] for unsupported property types,
/// [`PlyError::EmptyMesh`] if no geometry is found.
pub fn parse(bytes: &[u8]) -> Result<MeshData, PlyError> {
    parse_with_options(bytes, &PlyOptions::default())
}

/// Parse a PLY file from bytes with explicit options.
///
/// See [`PlyOptions`] for available settings.
pub fn parse_with_options(bytes: &[u8], options: &PlyOptions) -> Result<MeshData, PlyError> {
    let header = parse_header(bytes)?;
    let data_start = header.data_offset;
    let body = &bytes[data_start..];

    let raw = match header.format {
        PlyFormat::Ascii => parse_ascii_body(&header, body)?,
        PlyFormat::BinaryLittleEndian => parse_binary_body(&header, body, Endian::Little)?,
        PlyFormat::BinaryBigEndian => parse_binary_body(&header, body, Endian::Big)?,
    };

    if raw.positions.is_empty() {
        return Err(PlyError::EmptyMesh);
    }

    // Build indices from faces (fan triangulation for n-gons)
    let indices = triangulate_faces(&raw.faces, raw.positions.len())?;

    // If no faces, treat as triangle soup if vertex count is divisible by 3,
    // otherwise error
    let indices = if indices.is_empty() {
        if raw.positions.len() >= 3 && raw.positions.len() % 3 == 0 {
            (0..raw.positions.len() as u32).collect()
        } else {
            return Err(PlyError::EmptyMesh);
        }
    } else {
        indices
    };

    let normals = if options.recompute_normals || raw.normals.is_empty() {
        if raw.normals.is_empty() && !options.recompute_normals {
            None
        } else {
            Some(crate::parsers::stl::compute_smooth_normals(
                &raw.positions,
                &indices,
            ))
        }
    } else {
        Some(raw.normals)
    };

    let uvs = if raw.uvs.is_empty() {
        None
    } else {
        Some(raw.uvs)
    };

    let vertex_colors = if raw.vertex_colors.is_empty() {
        None
    } else {
        Some(raw.vertex_colors)
    };

    Ok(MeshData {
        topology: crate::capabilities::TopologyKind::TriangleList,
        positions: raw.positions,
        normals,
        uvs,
        vertex_colors,
        indices,
        material_index: None,
    })
}

// ---------------------------------------------------------------------------
// Header types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlyFormat {
    Ascii,
    BinaryLittleEndian,
    BinaryBigEndian,
}

/// Scalar types that PLY properties can have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScalarType {
    Char,
    UChar,
    Short,
    UShort,
    Int,
    UInt,
    Float,
    Double,
}

impl ScalarType {
    fn from_str(s: &str) -> Result<Self, PlyError> {
        match s {
            "char" | "int8" => Ok(Self::Char),
            "uchar" | "uint8" => Ok(Self::UChar),
            "short" | "int16" => Ok(Self::Short),
            "ushort" | "uint16" => Ok(Self::UShort),
            "int" | "int32" => Ok(Self::Int),
            "uint" | "uint32" => Ok(Self::UInt),
            "float" | "float32" => Ok(Self::Float),
            "double" | "float64" => Ok(Self::Double),
            other => Err(PlyError::UnsupportedFormat(format!(
                "unknown scalar type: {other}"
            ))),
        }
    }

    fn byte_size(self) -> usize {
        match self {
            Self::Char | Self::UChar => 1,
            Self::Short | Self::UShort => 2,
            Self::Int | Self::UInt | Self::Float => 4,
            Self::Double => 8,
        }
    }
}

#[derive(Debug, Clone)]
enum PropertyDef {
    Scalar {
        name: String,
        scalar_type: ScalarType,
    },
    List {
        name: String,
        count_type: ScalarType,
        value_type: ScalarType,
    },
}

impl PropertyDef {
    fn name(&self) -> &str {
        match self {
            Self::Scalar { name, .. } => name,
            Self::List { name, .. } => name,
        }
    }
}

#[derive(Debug, Clone)]
struct ElementDef {
    name: String,
    count: usize,
    properties: Vec<PropertyDef>,
}

#[derive(Debug)]
struct PlyHeader {
    format: PlyFormat,
    elements: Vec<ElementDef>,
    /// Byte offset where data begins (right after "end_header\n").
    data_offset: usize,
}

// ---------------------------------------------------------------------------
// Vertex property index lookup
// ---------------------------------------------------------------------------

/// Indices into the property list for known vertex attributes.
#[derive(Debug, Default)]
struct VertexLayout {
    x: Option<usize>,
    y: Option<usize>,
    z: Option<usize>,
    nx: Option<usize>,
    ny: Option<usize>,
    nz: Option<usize>,
    u: Option<usize>,
    v: Option<usize>,
    red: Option<usize>,
    green: Option<usize>,
    blue: Option<usize>,
    alpha: Option<usize>,
}

impl VertexLayout {
    fn from_properties(props: &[PropertyDef]) -> Self {
        let mut layout = VertexLayout::default();
        for (i, prop) in props.iter().enumerate() {
            match prop.name() {
                "x" => layout.x = Some(i),
                "y" => layout.y = Some(i),
                "z" => layout.z = Some(i),
                "nx" => layout.nx = Some(i),
                "ny" => layout.ny = Some(i),
                "nz" => layout.nz = Some(i),
                // UV coordinate names vary across exporters
                "s" | "u" | "texture_u" => layout.u = Some(i),
                "t" | "v" | "texture_v" => layout.v = Some(i),
                "red" => layout.red = Some(i),
                "green" => layout.green = Some(i),
                "blue" => layout.blue = Some(i),
                "alpha" => layout.alpha = Some(i),
                _ => {} // skip unknown properties
            }
        }
        layout
    }

    fn has_positions(&self) -> bool {
        self.x.is_some() && self.y.is_some() && self.z.is_some()
    }

    fn has_normals(&self) -> bool {
        self.nx.is_some() && self.ny.is_some() && self.nz.is_some()
    }

    fn has_uvs(&self) -> bool {
        self.u.is_some() && self.v.is_some()
    }

    fn has_colors(&self) -> bool {
        self.red.is_some() && self.green.is_some() && self.blue.is_some()
    }
}

// ---------------------------------------------------------------------------
// Raw parsed data (before building MeshData)
// ---------------------------------------------------------------------------

struct RawPlyData {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    vertex_colors: Vec<[f32; 4]>,
    faces: Vec<Vec<u32>>,
}

// ---------------------------------------------------------------------------
// Header parsing
// ---------------------------------------------------------------------------

fn parse_header(bytes: &[u8]) -> Result<PlyHeader, PlyError> {
    // PLY header is always ASCII text, terminated by "end_header\n"
    // Find end_header to know where the header ends.
    let header_str = find_header_text(bytes)?;

    let mut lines = header_str.lines();

    // First line must be "ply"
    let magic = lines
        .next()
        .ok_or_else(|| PlyError::InvalidHeader("empty file".into()))?
        .trim();
    if magic != "ply" {
        return Err(PlyError::InvalidHeader(format!(
            "expected 'ply' magic, got '{magic}'"
        )));
    }

    let mut format: Option<PlyFormat> = None;
    let mut elements: Vec<ElementDef> = Vec::new();
    let mut current_element: Option<ElementDef> = None;

    for line in lines {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with("comment") || line.starts_with("obj_info") {
            continue;
        }

        if line == "end_header" {
            if let Some(elem) = current_element.take() {
                elements.push(elem);
            }
            break;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        match parts[0] {
            "format" => {
                if parts.len() < 3 {
                    return Err(PlyError::InvalidHeader(format!(
                        "malformed format line: {line}"
                    )));
                }
                format = Some(match parts[1] {
                    "ascii" => PlyFormat::Ascii,
                    "binary_little_endian" => PlyFormat::BinaryLittleEndian,
                    "binary_big_endian" => PlyFormat::BinaryBigEndian,
                    other => {
                        return Err(PlyError::UnsupportedFormat(format!(
                            "unknown PLY format: {other}"
                        )));
                    }
                });
            }
            "element" => {
                if let Some(elem) = current_element.take() {
                    elements.push(elem);
                }
                if parts.len() < 3 {
                    return Err(PlyError::InvalidHeader(format!(
                        "malformed element line: {line}"
                    )));
                }
                let count: usize = parts[2].parse().map_err(|_| {
                    PlyError::InvalidHeader(format!("invalid element count: {}", parts[2]))
                })?;
                current_element = Some(ElementDef {
                    name: parts[1].to_string(),
                    count,
                    properties: Vec::new(),
                });
            }
            "property" => {
                let elem = current_element
                    .as_mut()
                    .ok_or_else(|| PlyError::InvalidHeader("property before element".into()))?;

                if parts.len() >= 3 && parts[1] == "list" {
                    // property list <count_type> <value_type> <name>
                    if parts.len() < 5 {
                        return Err(PlyError::InvalidHeader(format!(
                            "malformed list property: {line}"
                        )));
                    }
                    let count_type = ScalarType::from_str(parts[2])?;
                    let value_type = ScalarType::from_str(parts[3])?;
                    elem.properties.push(PropertyDef::List {
                        name: parts[4].to_string(),
                        count_type,
                        value_type,
                    });
                } else if parts.len() >= 3 {
                    // property <type> <name>
                    let scalar_type = ScalarType::from_str(parts[1])?;
                    elem.properties.push(PropertyDef::Scalar {
                        name: parts[2].to_string(),
                        scalar_type,
                    });
                } else {
                    return Err(PlyError::InvalidHeader(format!(
                        "malformed property line: {line}"
                    )));
                }
            }
            _ => {
                // Unknown header keyword — skip silently for forward compat
            }
        }
    }

    // Push trailing element if end_header was the terminator
    if let Some(elem) = current_element.take() {
        elements.push(elem);
    }

    let format =
        format.ok_or_else(|| PlyError::InvalidHeader("missing format declaration".into()))?;

    // Find byte offset of data start: search for "end_header\n" or "end_header\r\n"
    let data_offset = find_end_header_offset(bytes)?;

    Ok(PlyHeader {
        format,
        elements,
        data_offset,
    })
}

/// Extract the header portion as a UTF-8 string.
fn find_header_text(bytes: &[u8]) -> Result<&str, PlyError> {
    // The header is at most a few KB. Search for "end_header" within a reasonable bound.
    let search_limit = bytes.len().min(1024 * 1024); // 1 MB max header
    let search_bytes = &bytes[..search_limit];

    // Find "end_header"
    let pattern = b"end_header";
    let pos = search_bytes
        .windows(pattern.len())
        .position(|w| w == pattern)
        .ok_or_else(|| PlyError::InvalidHeader("missing end_header".into()))?;

    // Include the end_header line
    let end = pos + pattern.len();

    std::str::from_utf8(&bytes[..end])
        .map_err(|e| PlyError::InvalidHeader(format!("header is not valid UTF-8: {e}")))
}

/// Find the byte offset immediately after the end_header line terminator.
fn find_end_header_offset(bytes: &[u8]) -> Result<usize, PlyError> {
    let pattern = b"end_header";
    let pos = bytes
        .windows(pattern.len())
        .position(|w| w == pattern)
        .ok_or_else(|| PlyError::InvalidHeader("missing end_header".into()))?;

    let mut offset = pos + pattern.len();

    // Skip line ending: \r\n or \n
    if offset < bytes.len() && bytes[offset] == b'\r' {
        offset += 1;
    }
    if offset < bytes.len() && bytes[offset] == b'\n' {
        offset += 1;
    }

    Ok(offset)
}

// ---------------------------------------------------------------------------
// ASCII body parsing
// ---------------------------------------------------------------------------

fn parse_ascii_body(header: &PlyHeader, body: &[u8]) -> Result<RawPlyData, PlyError> {
    let text = std::str::from_utf8(body)
        .map_err(|e| PlyError::InvalidHeader(format!("body is not valid UTF-8: {e}")))?;

    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let mut raw = RawPlyData {
        positions: Vec::new(),
        normals: Vec::new(),
        uvs: Vec::new(),
        vertex_colors: Vec::new(),
        faces: Vec::new(),
    };

    for element in &header.elements {
        match element.name.as_str() {
            "vertex" => {
                let layout = VertexLayout::from_properties(&element.properties);
                if !layout.has_positions() {
                    return Err(PlyError::InvalidHeader(
                        "vertex element missing x/y/z properties".into(),
                    ));
                }

                raw.positions.reserve(element.count);
                if layout.has_normals() {
                    raw.normals.reserve(element.count);
                }
                if layout.has_uvs() {
                    raw.uvs.reserve(element.count);
                }
                if layout.has_colors() {
                    raw.vertex_colors.reserve(element.count);
                }

                for i in 0..element.count {
                    let line = lines.next().ok_or_else(|| {
                        PlyError::InvalidHeader(format!(
                            "unexpected end of data at vertex {i}/{}",
                            element.count
                        ))
                    })?;
                    let tokens: Vec<&str> = line.split_whitespace().collect();

                    let get_f32 = |idx: usize| -> Result<f32, PlyError> {
                        tokens
                            .get(idx)
                            .ok_or_else(|| {
                                PlyError::InvalidHeader(format!(
                                    "vertex {i}: missing property at index {idx}"
                                ))
                            })?
                            .parse::<f64>()
                            .map(|v| v as f32)
                            .map_err(|e| {
                                PlyError::InvalidHeader(format!(
                                    "vertex {i}: invalid float at index {idx}: {e}"
                                ))
                            })
                    };

                    raw.positions.push([
                        get_f32(layout.x.unwrap())?,
                        get_f32(layout.y.unwrap())?,
                        get_f32(layout.z.unwrap())?,
                    ]);

                    if layout.has_normals() {
                        raw.normals.push([
                            get_f32(layout.nx.unwrap())?,
                            get_f32(layout.ny.unwrap())?,
                            get_f32(layout.nz.unwrap())?,
                        ]);
                    }

                    if layout.has_uvs() {
                        raw.uvs
                            .push([get_f32(layout.u.unwrap())?, get_f32(layout.v.unwrap())?]);
                    }

                    if layout.has_colors() {
                        let r = get_f32(layout.red.unwrap())?;
                        let g = get_f32(layout.green.unwrap())?;
                        let b = get_f32(layout.blue.unwrap())?;
                        let a = layout
                            .alpha
                            .map(|idx| get_f32(idx))
                            .transpose()?
                            .unwrap_or(255.0);

                        // Detect if colors are in 0-255 range and normalize
                        let (r, g, b, a) =
                            normalize_color(r, g, b, a, &element.properties, &layout);
                        raw.vertex_colors.push([r, g, b, a]);
                    }
                }
            }
            "face" => {
                raw.faces.reserve(element.count);
                for i in 0..element.count {
                    let line = lines.next().ok_or_else(|| {
                        PlyError::InvalidHeader(format!(
                            "unexpected end of data at face {i}/{}",
                            element.count
                        ))
                    })?;
                    let tokens: Vec<&str> = line.split_whitespace().collect();

                    if tokens.is_empty() {
                        return Err(PlyError::InvalidHeader(format!("face {i}: empty line")));
                    }

                    // First token is the vertex count for this face
                    let n: usize = tokens[0].parse().map_err(|e| {
                        PlyError::InvalidHeader(format!(
                            "face {i}: invalid vertex count '{}': {e}",
                            tokens[0]
                        ))
                    })?;

                    if tokens.len() < 1 + n {
                        return Err(PlyError::InvalidHeader(format!(
                            "face {i}: expected {n} indices but got {}",
                            tokens.len() - 1
                        )));
                    }

                    let mut face = Vec::with_capacity(n);
                    for j in 0..n {
                        let idx: u32 = tokens[1 + j].parse().map_err(|e| {
                            PlyError::InvalidHeader(format!(
                                "face {i}: invalid index '{}': {e}",
                                tokens[1 + j]
                            ))
                        })?;
                        face.push(idx);
                    }
                    raw.faces.push(face);
                }
            }
            _ => {
                // Skip unknown elements — consume their lines
                for _ in 0..element.count {
                    let _ = lines.next();
                }
            }
        }
    }

    Ok(raw)
}

// ---------------------------------------------------------------------------
// Binary body parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
enum Endian {
    Little,
    Big,
}

/// A cursor over a byte slice for binary reading.
struct BinaryReader<'a> {
    data: &'a [u8],
    pos: usize,
    endian: Endian,
}

impl<'a> BinaryReader<'a> {
    fn new(data: &'a [u8], endian: Endian) -> Self {
        Self {
            data,
            pos: 0,
            endian,
        }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn read_bytes(&mut self, n: usize) -> Result<&'a [u8], PlyError> {
        if self.pos + n > self.data.len() {
            return Err(PlyError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!(
                    "unexpected end of binary data at offset {} (need {n} bytes, have {})",
                    self.pos,
                    self.remaining()
                ),
            )));
        }
        let slice = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    fn read_scalar_as_f64(&mut self, ty: ScalarType) -> Result<f64, PlyError> {
        let bytes = self.read_bytes(ty.byte_size())?;
        Ok(match (ty, self.endian) {
            (ScalarType::Char, _) => bytes[0] as i8 as f64,
            (ScalarType::UChar, _) => bytes[0] as f64,
            (ScalarType::Short, Endian::Little) => i16::from_le_bytes([bytes[0], bytes[1]]) as f64,
            (ScalarType::Short, Endian::Big) => i16::from_be_bytes([bytes[0], bytes[1]]) as f64,
            (ScalarType::UShort, Endian::Little) => u16::from_le_bytes([bytes[0], bytes[1]]) as f64,
            (ScalarType::UShort, Endian::Big) => u16::from_be_bytes([bytes[0], bytes[1]]) as f64,
            (ScalarType::Int, Endian::Little) => {
                i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64
            }
            (ScalarType::Int, Endian::Big) => {
                i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64
            }
            (ScalarType::UInt, Endian::Little) => {
                u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64
            }
            (ScalarType::UInt, Endian::Big) => {
                u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64
            }
            (ScalarType::Float, Endian::Little) => {
                f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64
            }
            (ScalarType::Float, Endian::Big) => {
                f32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64
            }
            (ScalarType::Double, Endian::Little) => f64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            (ScalarType::Double, Endian::Big) => f64::from_be_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
        })
    }

    fn read_scalar_as_u32(&mut self, ty: ScalarType) -> Result<u32, PlyError> {
        let val = self.read_scalar_as_f64(ty)?;
        Ok(val as u32)
    }

    /// Skip over a scalar property without interpreting it.
    fn skip_scalar(&mut self, ty: ScalarType) -> Result<(), PlyError> {
        self.read_bytes(ty.byte_size())?;
        Ok(())
    }

    /// Skip over a list property.
    fn skip_list(
        &mut self,
        count_type: ScalarType,
        value_type: ScalarType,
    ) -> Result<(), PlyError> {
        let count = self.read_scalar_as_u32(count_type)? as usize;
        self.read_bytes(count * value_type.byte_size())?;
        Ok(())
    }
}

fn parse_binary_body(
    header: &PlyHeader,
    body: &[u8],
    endian: Endian,
) -> Result<RawPlyData, PlyError> {
    let mut reader = BinaryReader::new(body, endian);
    let mut raw = RawPlyData {
        positions: Vec::new(),
        normals: Vec::new(),
        uvs: Vec::new(),
        vertex_colors: Vec::new(),
        faces: Vec::new(),
    };

    for element in &header.elements {
        match element.name.as_str() {
            "vertex" => {
                let layout = VertexLayout::from_properties(&element.properties);
                if !layout.has_positions() {
                    return Err(PlyError::InvalidHeader(
                        "vertex element missing x/y/z properties".into(),
                    ));
                }

                raw.positions.reserve(element.count);
                if layout.has_normals() {
                    raw.normals.reserve(element.count);
                }
                if layout.has_uvs() {
                    raw.uvs.reserve(element.count);
                }
                if layout.has_colors() {
                    raw.vertex_colors.reserve(element.count);
                }

                for _i in 0..element.count {
                    let mut vals = Vec::with_capacity(element.properties.len());

                    for prop in &element.properties {
                        match prop {
                            PropertyDef::Scalar { scalar_type, .. } => {
                                vals.push(reader.read_scalar_as_f64(*scalar_type)?);
                            }
                            PropertyDef::List {
                                count_type,
                                value_type,
                                ..
                            } => {
                                // Unusual to have a list property on vertex, but skip it
                                reader.skip_list(*count_type, *value_type)?;
                                vals.push(0.0); // placeholder
                            }
                        }
                    }

                    raw.positions.push([
                        vals[layout.x.unwrap()] as f32,
                        vals[layout.y.unwrap()] as f32,
                        vals[layout.z.unwrap()] as f32,
                    ]);

                    if layout.has_normals() {
                        raw.normals.push([
                            vals[layout.nx.unwrap()] as f32,
                            vals[layout.ny.unwrap()] as f32,
                            vals[layout.nz.unwrap()] as f32,
                        ]);
                    }

                    if layout.has_uvs() {
                        raw.uvs.push([
                            vals[layout.u.unwrap()] as f32,
                            vals[layout.v.unwrap()] as f32,
                        ]);
                    }

                    if layout.has_colors() {
                        let r = vals[layout.red.unwrap()] as f32;
                        let g = vals[layout.green.unwrap()] as f32;
                        let b = vals[layout.blue.unwrap()] as f32;
                        let a = layout.alpha.map(|idx| vals[idx] as f32).unwrap_or(255.0);

                        let (r, g, b, a) =
                            normalize_color(r, g, b, a, &element.properties, &layout);
                        raw.vertex_colors.push([r, g, b, a]);
                    }
                }
            }
            "face" => {
                raw.faces.reserve(element.count);

                // Find the face list property (typically "vertex_indices" or "vertex_index")
                let list_prop_idx = element.properties.iter().position(|p| {
                    matches!(p, PropertyDef::List { name, .. }
                        if name == "vertex_indices" || name == "vertex_index")
                });

                for _i in 0..element.count {
                    let mut face_indices: Option<Vec<u32>> = None;

                    for (pi, prop) in element.properties.iter().enumerate() {
                        match prop {
                            PropertyDef::Scalar { scalar_type, .. } => {
                                reader.skip_scalar(*scalar_type)?;
                            }
                            PropertyDef::List {
                                count_type,
                                value_type,
                                ..
                            } => {
                                if Some(pi) == list_prop_idx {
                                    let count = reader.read_scalar_as_u32(*count_type)? as usize;
                                    let mut face = Vec::with_capacity(count);
                                    for _ in 0..count {
                                        face.push(reader.read_scalar_as_u32(*value_type)?);
                                    }
                                    face_indices = Some(face);
                                } else {
                                    reader.skip_list(*count_type, *value_type)?;
                                }
                            }
                        }
                    }

                    if let Some(face) = face_indices {
                        raw.faces.push(face);
                    }
                }
            }
            _ => {
                // Skip unknown elements
                for _ in 0..element.count {
                    for prop in &element.properties {
                        match prop {
                            PropertyDef::Scalar { scalar_type, .. } => {
                                reader.skip_scalar(*scalar_type)?;
                            }
                            PropertyDef::List {
                                count_type,
                                value_type,
                                ..
                            } => {
                                reader.skip_list(*count_type, *value_type)?;
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(raw)
}

// ---------------------------------------------------------------------------
// Triangulation
// ---------------------------------------------------------------------------

/// Fan-triangulate face lists into a flat triangle index buffer.
///
/// For a face with vertices [v0, v1, v2, v3, ...], produces triangles:
/// (v0, v1, v2), (v0, v2, v3), (v0, v3, v4), ...
fn triangulate_faces(faces: &[Vec<u32>], vertex_count: usize) -> Result<Vec<u32>, PlyError> {
    let mut indices = Vec::new();

    for (fi, face) in faces.iter().enumerate() {
        if face.len() < 3 {
            return Err(PlyError::InvalidHeader(format!(
                "face {fi} has fewer than 3 vertices ({})",
                face.len()
            )));
        }

        // Validate indices
        for &idx in face {
            if idx as usize >= vertex_count {
                return Err(PlyError::InvalidHeader(format!(
                    "face {fi}: index {idx} out of bounds for {vertex_count} vertices"
                )));
            }
        }

        // Fan triangulation: (v0, v1, v2), (v0, v2, v3), ...
        for i in 1..face.len() - 1 {
            indices.push(face[0]);
            indices.push(face[i] as u32);
            indices.push(face[i + 1] as u32);
        }
    }

    Ok(indices)
}

// ---------------------------------------------------------------------------
// Color normalization
// ---------------------------------------------------------------------------

/// Determine if color values are in 0-255 range (integer types) and normalize
/// to 0.0-1.0 range.
fn normalize_color(
    r: f32,
    g: f32,
    b: f32,
    a: f32,
    properties: &[PropertyDef],
    layout: &VertexLayout,
) -> (f32, f32, f32, f32) {
    // Check if the red property is an integer type (uchar, short, etc.)
    // If so, normalize from 0-255 to 0.0-1.0
    let is_integer_color = layout
        .red
        .and_then(|idx| match &properties[idx] {
            PropertyDef::Scalar { scalar_type, .. } => match scalar_type {
                ScalarType::UChar | ScalarType::Char | ScalarType::UShort | ScalarType::Short => {
                    Some(true)
                }
                _ => Some(false),
            },
            _ => None,
        })
        .unwrap_or(false);

    if is_integer_color {
        (r / 255.0, g / 255.0, b / 255.0, a / 255.0)
    } else {
        (r, g, b, a)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::fixture;

    // ── ASCII parsing ─────────────────────────────────────────────────

    #[test]
    fn parse_ascii_triangle() {
        let bytes = fixture("triangle.ply");
        let mesh = parse(&bytes).unwrap();

        assert!(mesh.validate().is_ok());
        assert_eq!(mesh.vertex_count(), 3);
        assert_eq!(mesh.triangle_count(), 1);
        assert!(!mesh.has_vertex_colors());
    }

    #[test]
    fn parse_ascii_triangle_has_positions() {
        let bytes = fixture("triangle.ply");
        let mesh = parse(&bytes).unwrap();

        // Check positions are roughly correct (right triangle at origin)
        assert_eq!(mesh.positions[0], [0.0, 0.0, 0.0]);
        assert_eq!(mesh.positions[1], [1.0, 0.0, 0.0]);
        assert_eq!(mesh.positions[2], [0.0, 1.0, 0.0]);
    }

    #[test]
    fn parse_ascii_with_normals() {
        let bytes = fixture("triangle.ply");
        let mesh = parse(&bytes).unwrap();

        // Our triangle.ply has normals
        assert!(mesh.has_normals());
        if let Some(ref normals) = mesh.normals {
            for n in normals {
                // All normals should point in +Z
                assert!((n[2] - 1.0).abs() < 0.01, "expected +Z normal, got {n:?}");
            }
        }
    }

    // ── Binary LE parsing ─────────────────────────────────────────────

    #[test]
    fn parse_binary_le_triangle() {
        let bytes = fixture("triangle_binary.ply");
        let mesh = parse(&bytes).unwrap();

        assert!(mesh.validate().is_ok());
        assert_eq!(mesh.vertex_count(), 3);
        assert_eq!(mesh.triangle_count(), 1);
    }

    #[test]
    fn binary_le_matches_ascii_positions() {
        let ascii = parse(&fixture("triangle.ply")).unwrap();
        let binary = parse(&fixture("triangle_binary.ply")).unwrap();

        assert_eq!(ascii.vertex_count(), binary.vertex_count());
        assert_eq!(ascii.triangle_count(), binary.triangle_count());

        for i in 0..ascii.positions.len() {
            for j in 0..3 {
                assert!(
                    (ascii.positions[i][j] - binary.positions[i][j]).abs() < 1e-6,
                    "position mismatch at [{i}][{j}]: ascii={}, binary={}",
                    ascii.positions[i][j],
                    binary.positions[i][j]
                );
            }
        }
    }

    // ── Vertex colors ─────────────────────────────────────────────────

    #[test]
    fn parse_ascii_with_vertex_colors() {
        let bytes = fixture("triangle_colors.ply");
        let mesh = parse(&bytes).unwrap();

        assert!(mesh.validate().is_ok());
        assert!(mesh.has_vertex_colors());
        let colors = mesh.vertex_colors.as_ref().unwrap();
        assert_eq!(colors.len(), 3);

        // Colors should be normalized to 0.0-1.0
        for c in colors {
            for &v in c {
                assert!(
                    (0.0..=1.0).contains(&v),
                    "color component out of range: {v}"
                );
            }
        }
    }

    #[test]
    fn vertex_colors_are_correct() {
        let bytes = fixture("triangle_colors.ply");
        let mesh = parse(&bytes).unwrap();

        let colors = mesh.vertex_colors.as_ref().unwrap();
        // Red vertex
        assert!((colors[0][0] - 1.0).abs() < 0.01, "expected red=1.0");
        assert!(colors[0][1] < 0.01, "expected green=0.0");
        assert!(colors[0][2] < 0.01, "expected blue=0.0");
    }

    // ── Quad / n-gon triangulation ────────────────────────────────────

    #[test]
    fn parse_quad_triangulated() {
        let bytes = fixture("quad.ply");
        let mesh = parse(&bytes).unwrap();

        assert!(mesh.validate().is_ok());
        assert_eq!(mesh.vertex_count(), 4);
        assert_eq!(mesh.triangle_count(), 2); // quad → 2 triangles
    }

    // ── Recompute normals option ──────────────────────────────────────

    #[test]
    fn recompute_normals() {
        let opts = PlyOptions {
            recompute_normals: true,
        };
        let mesh = parse_with_options(&fixture("triangle.ply"), &opts).unwrap();

        assert!(mesh.validate().is_ok());
        assert!(mesh.has_normals());
    }

    // ── Error cases ───────────────────────────────────────────────────

    #[test]
    fn empty_data_returns_error() {
        let err = parse(b"").unwrap_err();
        assert!(matches!(err, PlyError::InvalidHeader(_)));
    }

    #[test]
    fn missing_magic_returns_error() {
        let data = b"not a ply file\nend_header\n";
        let err = parse(data).unwrap_err();
        assert!(matches!(err, PlyError::InvalidHeader(_)));
    }

    #[test]
    fn missing_end_header_returns_error() {
        let data = b"ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\n";
        let err = parse(data).unwrap_err();
        assert!(matches!(err, PlyError::InvalidHeader(_)));
    }

    #[test]
    fn no_vertices_returns_error() {
        let data = b"ply\nformat ascii 1.0\nelement vertex 0\nproperty float x\nproperty float y\nproperty float z\nend_header\n";
        let err = parse(data).unwrap_err();
        assert!(matches!(err, PlyError::EmptyMesh));
    }

    // ── Header parsing ────────────────────────────────────────────────

    #[test]
    fn header_parses_format() {
        let data = b"ply\nformat binary_little_endian 1.0\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nelement face 0\nproperty list uchar int vertex_indices\nend_header\n\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00";
        let header = parse_header(data).unwrap();
        assert_eq!(header.format, PlyFormat::BinaryLittleEndian);
    }

    #[test]
    fn header_parses_elements() {
        let data = b"ply\nformat ascii 1.0\nelement vertex 10\nproperty float x\nproperty float y\nproperty float z\nelement face 5\nproperty list uchar int vertex_indices\nend_header\n";
        let header = parse_header(data).unwrap();
        assert_eq!(header.elements.len(), 2);
        assert_eq!(header.elements[0].name, "vertex");
        assert_eq!(header.elements[0].count, 10);
        assert_eq!(header.elements[1].name, "face");
        assert_eq!(header.elements[1].count, 5);
    }

    #[test]
    fn header_skips_comments() {
        let data = b"ply\ncomment this is a comment\nformat ascii 1.0\ncomment another comment\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nend_header\n0 0 0\n1 0 0\n0 1 0\n";
        let header = parse_header(data).unwrap();
        assert_eq!(header.format, PlyFormat::Ascii);
        assert_eq!(header.elements[0].count, 3);
    }

    // ── Triangulation ─────────────────────────────────────────────────

    #[test]
    fn triangulate_triangle() {
        let faces = vec![vec![0u32, 1, 2]];
        let indices = triangulate_faces(&faces, 3).unwrap();
        assert_eq!(indices, vec![0, 1, 2]);
    }

    #[test]
    fn triangulate_quad() {
        let faces = vec![vec![0u32, 1, 2, 3]];
        let indices = triangulate_faces(&faces, 4).unwrap();
        assert_eq!(indices, vec![0, 1, 2, 0, 2, 3]);
    }

    #[test]
    fn triangulate_pentagon() {
        let faces = vec![vec![0u32, 1, 2, 3, 4]];
        let indices = triangulate_faces(&faces, 5).unwrap();
        assert_eq!(indices, vec![0, 1, 2, 0, 2, 3, 0, 3, 4]);
    }

    #[test]
    fn triangulate_degenerate_face_errors() {
        let faces = vec![vec![0u32, 1]]; // only 2 verts
        let err = triangulate_faces(&faces, 3).unwrap_err();
        assert!(matches!(err, PlyError::InvalidHeader(_)));
    }

    #[test]
    fn triangulate_out_of_bounds_errors() {
        let faces = vec![vec![0u32, 1, 5]]; // index 5 is out of bounds for 3 verts
        let err = triangulate_faces(&faces, 3).unwrap_err();
        assert!(matches!(err, PlyError::InvalidHeader(_)));
    }

    // ── Scalar type parsing ───────────────────────────────────────────

    #[test]
    fn scalar_type_aliases() {
        assert_eq!(ScalarType::from_str("float").unwrap(), ScalarType::Float);
        assert_eq!(ScalarType::from_str("float32").unwrap(), ScalarType::Float);
        assert_eq!(ScalarType::from_str("double").unwrap(), ScalarType::Double);
        assert_eq!(ScalarType::from_str("float64").unwrap(), ScalarType::Double);
        assert_eq!(ScalarType::from_str("uchar").unwrap(), ScalarType::UChar);
        assert_eq!(ScalarType::from_str("uint8").unwrap(), ScalarType::UChar);
        assert_eq!(ScalarType::from_str("int").unwrap(), ScalarType::Int);
        assert_eq!(ScalarType::from_str("int32").unwrap(), ScalarType::Int);
    }

    #[test]
    fn scalar_type_unknown_errors() {
        assert!(ScalarType::from_str("string").is_err());
    }

    #[test]
    fn scalar_type_byte_sizes() {
        assert_eq!(ScalarType::UChar.byte_size(), 1);
        assert_eq!(ScalarType::Short.byte_size(), 2);
        assert_eq!(ScalarType::Float.byte_size(), 4);
        assert_eq!(ScalarType::Double.byte_size(), 8);
    }
}
