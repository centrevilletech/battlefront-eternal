// Frozen de43cf0a cold constructors, captured before their JS deletion. These
// fixtures need no archive/content files and run in the native CI test suite.
use bfe_render::static_cards::{Cards, Definition};
use bfe_render::static_world::{Geometry, Record, Terrain};
use serde::Deserialize;
#[derive(Deserialize)]
struct Fixture { foliage: Vec<Foliage>, terrain: Vec<Ground> }
#[derive(Deserialize)]
struct Foliage { grass: bool, def: Definition, #[serde(rename="authoredBits")] authored_bits: Vec<String>, vertices: Vec<u32>, index: Vec<u32>, wiggle: u32 }
#[derive(Deserialize)]
struct Level { index: Vec<u32>, cells: Vec<[f64;8]> }
#[derive(Deserialize)]
struct Ground { record: Record, vertices: Vec<u32>, index: Vec<u32>, full: Level, reduced: Level, sphere: [f64;4] }
fn fixture() -> Fixture { serde_json::from_str(include_str!("fixtures/static-descriptors.json")).unwrap() }
#[test]
fn foliage_cold_vertex_index_and_material_bits() {
    for f in fixture().foliage {
        let authored = f.authored_bits.iter().map(|v|f64::from_bits(v.parse().unwrap())).collect::<Vec<_>>();
        let cards = Cards::new(f.grass,&authored,&f.def).unwrap();
        let mismatch = cards.vertices.iter().zip(&f.vertices).enumerate().find(|(_, (a,b))|a.to_bits()!=**b);
        assert!(mismatch.is_none(),"foliage grass={} parts={:?} first mismatch={mismatch:?}",f.grass,f.def.num_parts);
        assert_eq!(cards.index,f.index);
        assert_eq!(cards.wiggle.to_bits(),f.wiggle);
    }
}
#[test]
fn terrain_cold_index_cells_lod_and_bounds_bits() {
    for f in fixture().terrain {
        let vertices = f.vertices.iter().map(|v|f32::from_bits(*v)).collect::<Vec<_>>();
        let g = Geometry::new(&f.record,&vertices,&f.index).unwrap();
        let t = Terrain::new(&g);
        assert_eq!(t.full.index,f.full.index);
        assert_eq!(t.reduced.index,f.reduced.index);
        assert_eq!(t.full.cells.iter().flatten().map(|v|v.to_bits()).collect::<Vec<_>>(),f.full.cells.iter().flatten().map(|v|v.to_bits()).collect::<Vec<_>>());
        assert_eq!(t.reduced.cells.iter().flatten().map(|v|v.to_bits()).collect::<Vec<_>>(),f.reduced.cells.iter().flatten().map(|v|v.to_bits()).collect::<Vec<_>>());
        assert_eq!(t.sphere.map(f64::to_bits),f.sphere.map(f64::to_bits));
    }
}
// Reject malformed cold input before binding a VAO or indexing a fixed atlas.
// Failed admission leaves live descriptors untouched and can report one load
// error; ordinary frames never discover a bad authored resource.
#[test]
fn static_geometry_checks_counts_streams_and_indices() {
    let record = Record { vertex_count:3,stride:3,attributes:vec!["position".into()],..Record::default() };
    let vertices = [0.,0.,0., 1.,0.,0., 0.,1.,0.];
    assert!(Geometry::new(&record,&vertices,&[0,1,2]).is_ok());
    assert!(Geometry::new(&record,&vertices,&[0,1,3]).is_err());
    assert!(Geometry::new(&record,&vertices[..8],&[0,1,2]).is_err());
    assert!(Geometry::new(&record,&vertices,&[0,1]).is_err());
    // Geometry constructors preserve NaN UV payloads on authored atlas edges;
    // only positions must be finite for native bounds and spatial selection.
    let uv_record=Record {vertex_count:3,stride:5,attributes:vec!["position".into(),"uv".into()],..Record::default()};
    let payload=f32::from_bits(0x7fc01234);
    let uv_vertices=[0.,0.,0.,payload,0., 1.,0.,0.,0.,0., 0.,1.,0.,0.,0.];
    let uv=Geometry::new(&uv_record,&uv_vertices,&[0,1,2]).unwrap();
    assert_eq!(uv.vertices[3].to_bits(),payload.to_bits());
    let mut invalid_position=uv_vertices;invalid_position[0]=payload;
    assert!(Geometry::new(&uv_record,&invalid_position,&[0,1,2]).is_err());
    let huge = Record { vertex_count:usize::MAX,..record };
    assert!(Geometry::new(&huge,&vertices,&[0,1,2]).is_err());
}

#[derive(Deserialize)]
struct ExtraFixture { special: Vec<Special>, material: Vec<Surface> }
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct Special { record:Record, vertices:Vec<u32>, index:Vec<u32>, expected:Vec<u32>, expected_index:Vec<u32>, index_width:u32, bounds:[f64;10] }
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct Surface { record:Record, vertices:Vec<u32>, index:Vec<u32>, features:Vec<u32>, samplers:Vec<i32>, key:String, flags:u32, receive_shadow:bool, values:std::collections::BTreeMap<String,Vec<u32>> }
#[test]
fn fabric_and_original_tree_geometry_match_frozen_components_and_index_width() {
    let fixture:ExtraFixture=serde_json::from_str(include_str!("fixtures/static-descriptors.json")).unwrap();
    for f in fixture.special {
        let vertices=f.vertices.iter().map(|v|f32::from_bits(*v)).collect::<Vec<_>>();
        let g=Geometry::new(&f.record,&vertices,&f.index).unwrap();
        let actual=if let Some(groups)=&g.tree_groups {
            g.vertices.chunks_exact(g.stride as usize/4).zip(groups).flat_map(|(row,id)|row.iter().chain(std::iter::once(id)).map(|v|v.to_bits())).collect::<Vec<_>>()
        } else { g.vertices.iter().map(|v|v.to_bits()).collect() };
        let mismatch=actual.iter().zip(&f.expected).enumerate().find(|(_, (a,b))|a!=b);
        assert_eq!(actual.len(),f.expected.len());
        assert!(mismatch.is_none(),"{} first component mismatch={mismatch:?}",f.record.texture);
        assert_eq!(g.index,f.expected_index);
        assert_eq!(g.index_width,f.index_width);
        assert_eq!(g.bounds.map(f64::to_bits),f.bounds.map(f64::to_bits),"{} bounds",f.record.texture);
    }
}
#[test]
fn material_catalog_raster_uniform_and_sampler_descriptors_match_js() {
    let fixture:ExtraFixture=serde_json::from_str(include_str!("fixtures/static-descriptors.json")).unwrap();
    for f in fixture.material {
        let vertices=f.vertices.iter().map(|v|f32::from_bits(*v)).collect::<Vec<_>>();
        let g=Geometry::new(&f.record,&vertices,&f.index).unwrap();
        let m=bfe_render::static_world::Material::new(&f.record,&g,&[],&f.samplers,&f.features).unwrap();
        assert_eq!(m.key,f.key);assert_eq!(bfe_render::static_depth::shadow_side(&f.record),m.flags&3);
        assert_eq!(m.flags,f.flags);assert_eq!(m.receive_shadow,f.receive_shadow);
        for (name,bits) in f.values {assert_eq!(m.values[name.as_str()].iter().map(|v|v.to_bits()).collect::<Vec<_>>(),bits,"{} {name}",m.key);}
        if f.samplers[0]>=0 {assert!(m.samplers.contains(&("map",0,7)));}
        else if f.record.far {assert!(m.samplers.contains(&("map",0,9)));}
        if f.samplers[1]>=0 {assert!(m.samplers.contains(&("bumpMap",2,8)));}
    }
}
