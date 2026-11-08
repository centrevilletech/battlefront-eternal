//! Frozen Three publications cover multiplication, normal and UV f64 order.
use bfe_render::{math::UniformTransforms,cameras::layout::{uniform_input as i,uniform_output as o}};
#[test]
fn frozen_uniform_publications_match_every_f64_word(){
    let cases:serde_json::Value=serde_json::from_str(include_str!("uniform-transforms.json")).unwrap();
    let mut owner=UniformTransforms::default();
    for case in cases.as_array().unwrap(){
        let input=case["input"].as_array().unwrap();assert_eq!(input.len(),i::LEN*2);
        for k in 0..i::LEN{owner.input[k]=f64::from_bits(input[k*2].as_u64().unwrap()|(input[k*2+1].as_u64().unwrap()<<32));}
        owner.frame(case["uv"].as_bool().unwrap());let output=case["output"].as_array().unwrap();
        if !case["uv"].as_bool().unwrap(){assert_eq!(bfe_render::math::determinant_mirrors(&owner.input[i::WORLD..i::WORLD+16].try_into().unwrap()),case["mirrored"].as_bool().unwrap());}
        assert_eq!(output.len(),o::LEN*2);
        for k in 0..o::LEN{
            if (o::UV..o::UV+9).contains(&k)!=case["uv"].as_bool().unwrap(){continue;}
            assert_eq!(owner.output[k].to_bits(),output[k*2].as_u64().unwrap()|(output[k*2+1].as_u64().unwrap()<<32),"{} word {k}",case["name"]);
        }
    }
}
