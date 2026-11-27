//! Blended world surfaces (barriers, lamp skirts, reflective floors) take the
//! transparent catalog and keep cutout precedence.
use bfe_render::static_world::{ExtendedBlend, ExtendedRecord, ExtendedSurface, Geometry, Material, Record};

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

#[test]
fn three_layer_painted_terrain_uses_its_own_catalog_variant() {
    let record = Record {
        terrain: true, vertex_count: 3, stride: 7,
        attributes: vec!["position".into(), "color".into()],
        layers: vec!["a".into(), "b".into(), "c".into()], ..Record::default()
    };
    let vertices = [0.,0.,0.,1.,1.,1.,1., 1.,0.,0.,1.,1.,1.,1., 0.,0.,1.,1.,1.,1.,1.];
    let geometry = Geometry::new(&record, &vertices, &[0,1,2]).unwrap();
    let weights = [64, 64, 127, 0, 0, 0, 0, 0].repeat(3);
    let samplers = [0; bfe_render::static_layout::SAMPLERS_LENGTH];
    // Non-extended 2004 records keep the released rejection behaviour.
    assert!(Material::new(&record, &geometry, &weights, &samplers, &[4, 8, 1, 0]).is_err());
    let ext = ExtendedRecord { base: record, ..ExtendedRecord::default() };
    let surface = ExtendedSurface::resolve(&ext).unwrap();
    for (shadows, csm) in [(0, 0), (2, 1), (3, 1), (4, 1)] {
        let material = Material::new_swbf2(&ext.base, &surface, &geometry, &weights, &samplers, &[shadows, 8, csm, 0]).unwrap();
        let spec = bfe_render::programs::spec(&material.key).unwrap();
        assert!(spec.fragment_prefix.contains("#define EXTENDED_LIGHT"));
        assert!(spec.fragment_prefix.contains("TERRAIN_LAYERS 3"));
    }
}

#[test]
fn released_2004_admission_ignores_unknown_extended_fields() {
    let vertices = [0.,0.,0., 0.,1.,0., 0.,0., 1.,0.,0., 0.,1.,0., 1.,0., 0.,0.,1., 0.,1.,0., 0.,1.];
    let baseline_json = r#"{"texture":"stone_wall","flags":1,"vertexCount":3,"stride":8,"attributes":["position","normal","uv"]}"#;
    let baseline_rec: Record = serde_json::from_str(baseline_json).unwrap();
    let baseline_geo = Geometry::new(&baseline_rec, &vertices, &[0, 1, 2]).unwrap();
    let baseline_mat = Material::new(&baseline_rec, &baseline_geo, &[], &[7, -1], &[4, 14, 1, 0]).unwrap();

    // All five extra JSON shapes that older 2004 maps or tools may carry must
    // still parse and compile identically to the baseline 2004 record.
    for extra in [
        r#","material":"annotated""#,
        r#","glow":{"version":1}"#,
        r#","glow":true"#,
        r#","material":{"scroll":[0.27,0.19]}"#,
        r#","extendedLight":true"#,
    ] {
        let json = format!(
            r#"{{"texture":"stone_wall","flags":1,"vertexCount":3,"stride":8,"attributes":["position","normal","uv"]{extra}}}"#
        );
        let rec: Record = serde_json::from_str(&json).expect(&json);
        let geo = Geometry::new(&rec, &vertices, &[0, 1, 2]).unwrap();
        let mat = Material::new(&rec, &geo, &[], &[7, -1], &[4, 14, 1, 0]).unwrap();
        assert_eq!(mat.key, baseline_mat.key, "extra field changed 2004 program: {extra}");
        assert_eq!(mat.flags, baseline_mat.flags, "extra field changed 2004 flags: {extra}");
        assert_eq!(mat.blend, baseline_mat.blend, "extra field changed 2004 blend: {extra}");
        assert_eq!(mat.values, baseline_mat.values, "extra field changed 2004 values: {extra}");
    }
}

#[test]
fn extended_2005_cutout_without_legacy_flag_bit_resolves_consistently_and_rejects_contradictions() {
    let vertices = [0.,0.,0., 0.,1.,0., 0.,0., 1.,0.,0., 0.,1.,0., 1.,0., 0.,0.,1., 0.,1.,0., 0.,1.];
    // Cutout defined via structured material without legacy bit 2 (and doubleSided):
    // must resolve as opaque alpha-test, never blended, and produce blend == None.
    let cutout_json = r#"{
        "texture":"market_awning","flags":1,"vertexCount":3,"stride":8,
        "attributes":["position","normal","uv"],
        "material":{"blendMode":"opaque","alphaCutout":0.5,"doubleSided":true}
    }"#;
    let ext: ExtendedRecord = serde_json::from_str(cutout_json).unwrap();
    let surface = ExtendedSurface::resolve(&ext).unwrap();
    assert!(surface.cutout);
    assert!(surface.double_sided);
    assert_eq!(surface.blend_mode, ExtendedBlend::Opaque);
    assert!(!surface.is_blended());
    assert_ne!(surface.flags & 2, 0);
    assert_eq!(surface.flags & 4, 0);
    let geo = Geometry::new(&ext.base, &vertices, &[0, 1, 2]).unwrap();
    let mat = Material::new_swbf2(&ext.base, &surface, &geo, &[], &[7, -1], &[4, 8, 1, 0]).unwrap();
    assert!(mat.blend.is_none());
    assert_ne!(mat.flags & 4, 0);

    // Contradictory records return admission errors instead of panicking.
    for bad in [
        r#"{"texture":"bad","flags":1,"vertexCount":3,"stride":8,"attributes":["position","normal","uv"],"material":{"blendMode":"alpha","alphaCutout":0.5}}"#,
        r#"{"texture":"bad","flags":129,"vertexCount":3,"stride":8,"attributes":["position","normal","uv"],"material":{"blendMode":"opaque"}}"#,
        r#"{"texture":"bad","flags":5,"vertexCount":3,"stride":8,"attributes":["position","normal","uv"],"material":{"blendMode":"opaque"}}"#,
        r#"{"texture":"bad","flags":1,"vertexCount":3,"stride":8,"attributes":["position","normal","uv"],"material":{"blendMode":"multiply"}}"#,
        r#"{"texture":"bad","flags":1,"vertexCount":3,"stride":8,"attributes":["position","normal","uv"],"material":{"blendMode":"opaque","alphaCutout":-0.25}}"#,
    ] {
        let ext: ExtendedRecord = serde_json::from_str(bad).unwrap();
        assert!(ExtendedSurface::resolve(&ext).is_err(), "contradictory record should fail admission: {bad}");
    }

    // An opaque, two-sided panel keeps its depth writes and its own culling.
    let ds_opaque: ExtendedRecord = serde_json::from_str(
        r#"{"texture":"panel","flags":1,"vertexCount":3,"stride":8,"attributes":["position","normal","uv"],"material":{"blendMode":"opaque","doubleSided":true}}"#,
    ).unwrap();
    let ds_surface = ExtendedSurface::resolve(&ds_opaque).unwrap();
    assert!(!ds_surface.is_blended());
    let panel = Material::new_swbf2(&ds_opaque.base, &ds_surface, &geo, &[], &[7, -1], &[4, 8, 1, 0]).unwrap();
    assert!(panel.blend.is_none());
    assert_eq!(panel.flags & 3, 2);
}

#[test]
fn placed_baked_programs_link_the_vertex_attributes_to_instance_light_rows() {
    use bfe_render::{actor_resources, programs, shader};
    for kind in ["rigid", "skinned"] {
        let s = programs::specs().iter().find(|s| s.key.starts_with(&format!("actors/{kind}-"))
            && s.key.ends_with("-baked") && !s.vertex_prefix.contains("NO_POINT")
            && !s.vertex_prefix.contains("ALPHA_TEST")).unwrap();
        assert!(s.attributes.contains(&("bakedLight", 5)));
        assert!(s.attributes.contains(&("vertexTint", 6)));
        let (vs, fs) = programs::sources(s);
        let vs = shader::rewrite(&vs, shader::Stage::Vertex, s.kind).unwrap();
        let fs = shader::rewrite(&fs, shader::Stage::Fragment, s.kind).unwrap();
        assert!(vs.contains("bfeLightColor( i ).rgb") && vs.contains("bfeLightSphere( i )"));
        assert!(fs.contains("clamp(baked, 0.0, 1.0) * tint"));
        for cascades in [0, 2, 3, 4] {
            for lamps in [false, true] {
                assert_eq!(actor_resources::variant(s.key, cascades, lamps).as_deref(), Some(s.key));
            }
        }
    }
}
