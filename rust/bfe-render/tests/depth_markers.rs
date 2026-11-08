//! Freeze the original depth-copy view hook before shader or upload conversion.
use bfe_render::cameras::{depth_marker::DepthMarker,layout::{depth_marker as o}};
use serde_json::Value;
fn number(v:&Value)->f64{let text=v.as_str().unwrap();let mut bytes=[0;8];for i in 0..8{bytes[i]=u8::from_str_radix(&text[i*2..i*2+2],16).unwrap();}f64::from_le_bytes(bytes)}
#[test]
fn frozen_marker_words_include_equal_depth_and_degenerate_views(){
 let fixtures:Value=serde_json::from_str(include_str!("depth-markers.json")).unwrap();let mut owner=DepthMarker::default();
 for (case,fixture) in fixtures.as_array().unwrap().iter().enumerate(){
  let screen:Vec<f64>=fixture["screen"].as_array().unwrap().iter().map(number).collect();owner.begin(screen.as_slice().try_into().unwrap());
  for key in fixture["keys"].as_array().unwrap(){for(i,v)in key.as_array().unwrap().iter().enumerate(){owner.key[i]=number(v);}assert!(owner.request());}
  owner.finish();for(i,v)in fixture["output"].as_array().unwrap().iter().enumerate(){assert_eq!(owner.output[i].to_bits(),number(v).to_bits(),"case {case} word {i}");}
 }
 owner.begin(&[0.;16]);owner.finish();assert_eq!(owner.output[o::VISIBLE],0.);
}
