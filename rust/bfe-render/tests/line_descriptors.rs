// Frozen LineBasicMaterial and AxesHelper constructors. These synthetic
// descriptors run in CI without content files or a browser shader compiler.
use bfe_render::line_resources::Descriptor;
#[test]
fn frozen_line_materials_and_axes_geometry(){
 let expected:serde_json::Value=serde_json::from_str(include_str!("fixtures/line-descriptors.json")).unwrap();
 for kind in 0..3 {
  let source=Descriptor::new(kind).unwrap();
  for k in 0..3{assert_eq!(source.color[k].to_bits(),expected["colors"][kind as usize][k].as_f64().unwrap().to_bits());}
  assert_eq!(source.depth,expected["depth"][kind as usize].as_bool().unwrap());
  assert_eq!(source.segments,kind!=0);assert_eq!(source.fixed,kind==2);
 }
 let (positions,colors)=Descriptor::axes();
 for (key,actual) in [("positions",positions),("axisColors",colors)]{
  let expected=expected[key].as_array().unwrap().iter().map(|x|x.as_u64().unwrap() as u32).collect::<Vec<_>>();
  assert_eq!(actual.map(f32::to_bits).as_slice(),expected.as_slice());
 }
 assert!(Descriptor::new(u32::MAX).is_none());
}
