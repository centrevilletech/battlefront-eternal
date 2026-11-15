//! Blended world surfaces (barriers, lamp skirts, reflective floors) take the
//! transparent catalog and keep cutout precedence.
use bfe_render::static_world::{Geometry, Material, Record};

#[test]
fn blended_world_uses_transparent_catalog_and_preserves_cutout_precedence() {
    let mut record = Record { texture: "bes2_prop_lamp_skirt".into(), flags: 4,
        scroll: Some([0.171875, 0.09375]), vertex_count: 3, stride: 8,
        attributes: vec!["position".into(), "normal".into(), "uv".into()], ..Record::default() };
    let vertices = [0.,0.,0., 0.,1.,0., 0.,0., 1.,0.,0., 0.,1.,0., 1.,0., 0.,0.,1., 0.,1.,0., 0.,1.];
    let geometry = Geometry::new(&record, &vertices, &[0,1,2]).unwrap();
    let material = Material::new(&record, &geometry, &[], &[7,-1], &[4,14,1,0]).unwrap();
    assert_eq!(material.blend.unwrap()[3..], [glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA, glow::ONE, glow::ONE_MINUS_SRC_ALPHA]);
    assert_eq!(material.flags & 3, 2);
    assert_eq!(material.values["scroll"], [0.171875, 0.09375]);
    assert!(bfe_render::programs::spec(&material.key).unwrap().fragment_prefix.contains("#define BLENDED"));
    record.flags = 7;
    record.scroll = None;
    let cutout = Material::new(&record, &geometry, &[], &[7,-1], &[4,14,1,0]).unwrap();
    assert!(cutout.blend.is_none());
    assert_ne!(cutout.flags & 4, 0);
}
