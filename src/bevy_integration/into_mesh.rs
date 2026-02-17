//! Conversion from [`MeshData`] to [`bevy::mesh::Mesh`].
//!
//! This is the bridge between loadfu's format-agnostic core and Bevy's
//! rendering types. The conversion is intentionally simple — just mapping
//! fields to mesh attributes.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};

use crate::MeshData;

impl From<MeshData> for Mesh {
    fn from(data: MeshData) -> Self {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD,
        );

        // Positions — always present
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, data.positions);

        // Normals — optional. If absent, Bevy can compute them.
        if let Some(normals) = data.normals {
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        }

        // UVs — optional.
        if let Some(uvs) = data.uvs {
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        }

        // Indices — always present, always u32
        mesh.insert_indices(Indices::U32(data.indices));

        mesh
    }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{sample_triangle, sample_triangle_bare};
    use bevy::mesh::VertexAttributeValues;

    #[test]
    fn full_triangle_converts() {
        let mesh_data = sample_triangle();
        let bevy_mesh: Mesh = mesh_data.into();

        // Check topology
        assert_eq!(
            bevy_mesh.primitive_topology(),
            PrimitiveTopology::TriangleList
        );

        // Check positions
        let positions = bevy_mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .expect("should have positions");
        match positions {
            VertexAttributeValues::Float32x3(verts) => {
                assert_eq!(verts.len(), 3);
                assert_eq!(verts[0], [0.0, 0.0, 0.0]);
                assert_eq!(verts[1], [1.0, 0.0, 0.0]);
                assert_eq!(verts[2], [0.0, 1.0, 0.0]);
            }
            _ => panic!("unexpected position attribute type"),
        }

        // Check normals
        let normals = bevy_mesh
            .attribute(Mesh::ATTRIBUTE_NORMAL)
            .expect("should have normals");
        match normals {
            VertexAttributeValues::Float32x3(norms) => {
                assert_eq!(norms.len(), 3);
                for n in norms {
                    assert_eq!(*n, [0.0, 0.0, 1.0]);
                }
            }
            _ => panic!("unexpected normal attribute type"),
        }

        // Check UVs
        let uvs = bevy_mesh
            .attribute(Mesh::ATTRIBUTE_UV_0)
            .expect("should have UVs");
        match uvs {
            VertexAttributeValues::Float32x2(coords) => {
                assert_eq!(coords.len(), 3);
            }
            _ => panic!("unexpected UV attribute type"),
        }

        // Check indices
        let indices = bevy_mesh.indices().expect("should have indices");
        match indices {
            Indices::U32(idx) => {
                assert_eq!(idx, &[0, 1, 2]);
            }
            _ => panic!("expected U32 indices"),
        }
    }

    #[test]
    fn bare_triangle_converts_without_normals_or_uvs() {
        let mesh_data = sample_triangle_bare();
        let bevy_mesh: Mesh = mesh_data.into();

        assert!(bevy_mesh.attribute(Mesh::ATTRIBUTE_POSITION).is_some());
        assert!(bevy_mesh.attribute(Mesh::ATTRIBUTE_NORMAL).is_none());
        assert!(bevy_mesh.attribute(Mesh::ATTRIBUTE_UV_0).is_none());
        assert!(bevy_mesh.indices().is_some());
    }

    #[test]
    fn render_asset_usages_is_main_world() {
        let mesh_data = sample_triangle();
        let bevy_mesh: Mesh = mesh_data.into();

        assert_eq!(bevy_mesh.asset_usage, RenderAssetUsages::MAIN_WORLD);
    }
}
