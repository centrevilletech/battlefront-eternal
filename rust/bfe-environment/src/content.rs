//! Cold environment recipes from Atmosphere, SkyFlyers, Snow, Weather and
//! EnvironmentEffects. Browser hosts resolve asset identities and copy these
//! facts; authored selection, seeds, transforms and limits stay native.
use serde_json::{json, Value};
use three::{Matrix4, Quaternion, Vector3};
fn n(v:&Value,key:&str,default:f64)->f64 {v[key].as_f64().unwrap_or(default)}
fn first(v:&Value,key:&str,default:f64)->f64 {v[key][0].as_f64().unwrap_or(default)}
fn clamp(v:f64,lo:f64,hi:f64)->f64 {v8math::min(v8math::max(v,lo),hi)}
fn array(v:&Value)->impl Iterator<Item=&Value> {v.as_array().into_iter().flatten()}
fn vector(v:Vector3)->Value {json!([v.x,v.y,v.z])}
fn enhanced(name:&str)->bool {matches!(name,"waterfountain"|"prop_steamgrate"|"prop_steampipe")}

// battleFlyers keeps mission order and drops absent or zero-count classes.
// Returning indices retains the authored JS identity needed by model caches.
fn flyers(data:&Value,mission:&str,mode:bool)->Vec<usize> {
    let table=if mode&&data["enhanced"].is_object(){&data["enhanced"]}else{&data["missions"]};
    array(&table[mission]).filter_map(Value::as_u64).map(|n|n as usize)
        .filter(|&i|data["flyers"].get(i).is_some_and(|d|n(d,"count",0.)>0.)).collect()
}
// Snow.constructor rounds combined density before limiting the repeated boxes.
// It must not round the two density inputs separately.
fn snow(data:&Value)->Value {
    let p=&data["precipitation"];
    let count=clamp(jsrt::conv::js_round(first(p,"ParticleDensity",30.)+first(p,"ParticleDensityRange",0.)),1.,2048.);
    let alpha=p["AlphaMinMax"][0].as_f64().unwrap_or(0.5);
    let range=clamp(first(p,"Range",10.),2.,60.);
    let boxes=clamp((crate::layout::snow::MAX_FLAKES as f64/count).floor(),1.,crate::layout::snow::MAX_BOXES as f64);
    use crate::layout::snow_config as config;
    let mut c=vec![0.;config::STRIDE];c[config::COUNT]=count;c[config::FALL]=first(p,"Velocity",10.);c[config::WANDER]=first(p,"VelocityRange",1.);
    c[config::ALPHA_MIN]=alpha;c[config::ALPHA_MAX]=p["AlphaMinMax"][1].as_f64().unwrap_or(alpha);
    c[config::SPIN_BASE]=first(p,"Rotation",0.);c[config::SPIN_RANGE]=first(p,"RotationRange",2.);
    c[config::WIND]=data["wind"][0].as_f64().unwrap_or(0.);c[config::WIND+1]=data["wind"][1].as_f64().unwrap_or(0.);
    c[config::GUST_RANGE]=n(&data["gusts"],"range",0.);c[config::GUST_RATE]=n(&data["gusts"],"rate",0.);c[config::RANGE]=range;c[config::BOX_LIMIT]=boxes;
    // Snow's byte tint uses per-component white defaults when authored,
    // while an absent tint retains the original grey-blue snowfall profile.
    let tint=(0..4).map(|i|p["Color"][i].as_f64().unwrap_or(if p["Color"].is_array(){255.}else{[192.,192.,192.,160.][i]})/255.).collect::<Vec<_>>();
    json!({"config":c,"tint":tint,"size":first(p,"ParticleSize",0.02),"count":count,"range":range,"boxes":boxes,"coverSize":16,"alpha":[c[config::ALPHA_MIN],c[config::ALPHA_MAX]]})
}
// Weather.rangeFor and applyMode share this cold preset policy. No mode
// change rebuilds seeds or chooses a new procedural random sequence.
fn weather(data:&Value)->Value {
    let p=&data["precipitation"];let range=clamp(first(p,"Range",12.5),4.,40.);
    let count=clamp(jsrt::conv::js_round(first(p,"ParticleDensity",256.)),32.,2048.);
    let color=(0..3).map(|i|p["Color"][i].as_f64().unwrap_or([216.,220.,228.][i])/255.).collect::<Vec<_>>();
    let wind=[data["wind"][0].as_f64().unwrap_or(0.),data["wind"][1].as_f64().unwrap_or(0.)];
    let alpha=if p["AlphaMinMax"].is_array(){[p["AlphaMinMax"][0].as_f64().unwrap_or(0.),p["AlphaMinMax"][1].as_f64().unwrap_or(0.)]}else{[0.3,0.45]};
    json!({"sheetSpeed":v8math::max(6.,first(p,"Velocity",9.)*1.4),"range":range,"enhancedRange":v8math::min(40.,range*3.2),"count":count,"capacity":v8math::min(8192.,count*32.),
        "color":color,"wind":wind,"alpha":alpha,"velocity":first(p,"Velocity",9.),"velocityRange":first(p,"VelocityRange",1.),"width":first(p,"ParticleSize",0.02),"streak":first(p,"StreakLength",1.)})
}
// Atmosphere.constructor hashes UTF-16 units, including both halves of an
// astral name, just as charCodeAt(0) did for each JS string iterator value.
fn atmosphere(data:&Value)->Value {
    let mut classes=vec![];
    if let Some(defs)=data["dust"]["classes"].as_array(){for entry in defs {
        let name=entry[0].as_str().unwrap_or("");let def=&entry[1];
        let instances:Vec<_>=array(&data["dust"]["instances"]).filter(|i|i["class"].as_str()==Some(name)).map(|i|i["position"].clone()).collect();
        if instances.is_empty()||n(def,"count",0.)==0. {continue;}
        let mut seed=0u32;
        for ch in name.chars(){let mut units=[0;2];let unit=ch.encode_utf16(&mut units)[0];seed=seed.wrapping_mul(31).wrapping_add(unit as u32);}
        classes.push(json!({"name":name,"seed":seed,"instances":instances}));
    }}
    let fog=&data["fogCloud"];
    let tint=[fog["color"][0].as_f64().unwrap_or(255.)/255.,fog["color"][1].as_f64().unwrap_or(255.)/255.,fog["color"][2].as_f64().unwrap_or(255.)/255.,1.];
    json!({"classes":classes,"tint":tint,"alpha":fog["color"][3].as_f64().unwrap_or(255.)/255.})
}
// EnvironmentEffects.constructor prepares all source volumes and the nearest
// fountain basin once. Keep the strict comparison so tied basins stay first.
fn environment(data:&Value)->Value {
    let mut instances=vec![];let mut slot=0;
    for item in array(&data["instances"]) {
        let mut m=Matrix4::default();for(i,v)in array(&item["matrix"]).take(16).enumerate(){m.elements[i]=v.as_f64().unwrap_or(0.);}
        let position=Vector3::new(m.elements[12],m.elements[13],m.elements[14]);
        let mut q=Quaternion::default();q.set_from_rotation_matrix(&m);
        let name=item["effect"].as_str().unwrap_or("");let fountain=name=="waterfountain";let pipe=name=="prop_steampipe";let classic=!enhanced(name);
        let mut direction=if pipe{Vector3::new(0.,0.,1.)}else{Vector3::new(0.,1.,0.)};direction.apply_quaternion(&q);
        let extent=if fountain{[10.,1.5,10.]}else if pipe{[2.5,4.,2.5]}else if matches!(item["hardpoint"].as_str(),Some("hp_steam_1"|"hp_steam_5")){[7.,6.,7.]}else{[6.,5.5,6.]};
        let up=Vector3::new(0.,1.,0.);let mut center=position;center.add_scaled_vector(if classic{&up}else{&direction},if classic{8.}else{2.});
        let mut row=json!({"position":vector(position),"quaternion":[q.x,q.y,q.z,q.w],"direction":vector(direction),"extent":extent,
            "kind":if fountain{2}else if pipe{1}else{0},"classic":classic,"fountain":fountain,"sphere":[center.x,center.y,center.z,if classic{16.}else if fountain{9.}else{7.}]});
        if fountain {
            let mut basin=None;let mut distance=f64::INFINITY;
            for b in array(&data["basins"]) {let d=v8math::hypot(b["center"][0].as_f64().unwrap_or(0.)-position.x,b["center"][2].as_f64().unwrap_or(0.)-position.z);
                if basin.is_none()||d<distance{basin=Some(b);distance=d;}
            }
            let water=basin.and_then(|b|b["center"][1].as_f64()).unwrap_or(position.y-0.2);
            row["slot"]=json!(slot);row["level"]=json!(water+0.797);row["low"]=json!(water-0.05);slot+=1;
        }
        instances.push(row);
    }
    let mut warmup=serde_json::Map::new();
    if let Some(effects)=data["effects"].as_array(){for entry in effects {
        let name=entry[0].as_str().unwrap_or("");let defs=&entry[1];
        let mut max=f64::NEG_INFINITY;for d in array(defs){max=v8math::max(max,n(&d["transformer"],"lifetime",0.));}
        warmup.insert(name.to_owned(),json!(v8math::min(4.,max)));
    }}
    json!({"instances":instances,"warmup":warmup})
}
// Water.constructor's bounds reflect Float32 draw positions. Patched cells
// contribute their authored vertices while individual visibility boxes retain
// the full unrounded cell footprint used by reflection eligibility.
fn water(data:&Value)->Value {
    let grid=n(data,"grid",0.);let cell=n(data,"cell",0.);let height=n(data,"height",0.);
    let ox=data["offset"][0].as_f64().unwrap_or(0.);let oz=data["offset"][1].as_f64().unwrap_or(0.);
    let(mut west,mut east,mut north,mut south)=(false,false,false,false);let mut boxes=vec![];
    let mut lo=[f64::INFINITY;3];let mut hi=[f64::NEG_INFINITY;3];
    let mut point=|p:[f64;3]|{for k in 0..3 {let v=p[k] as f32 as f64;lo[k]=v8math::min(lo[k],v);hi[k]=v8math::max(hi[k],v);}};
    for c in array(&data["cells"]) {let i=c[0].as_f64().unwrap_or(0.);let j=c[1].as_f64().unwrap_or(0.);
        west|=i==0.;east|=i==grid-1.;north|=j==0.;south|=j==grid-1.;
        boxes.push([i*cell-ox,height-1.,j*cell-oz,(i+1.)*cell-ox,height+1.,(j+1.)*cell-oz]);
        if !c[2].as_bool().unwrap_or(c[2].as_f64().unwrap_or(0.)!=0.){for a in [0.,1.] {for b in [0.,1.] {point([i*cell-ox+a*cell,height,j*cell-oz+b*cell]);}}}
    }
    for patch in array(&data["patches"]) {for p in array(&patch["positions"]){point(std::array::from_fn(|i|p[i].as_f64().unwrap_or(0.)));}}
    lo[1]-=1.;hi[1]+=1.;
    let rgba=|key:&str,alpha:f64|->Vec<f64>{(0..4).map(|i|data["look"][key][i].as_f64().unwrap_or(if i==3{alpha}else{255.})/255.).collect()};
    // Water.frames resolves authored sequence names once. Missing browser
    // images are still filtered by the asset host, preserving actual counts.
    let mut frames=serde_json::Map::new();
    for key in ["NormalMapTextures","BumpMapTextures","SpecularMaskTextures"] {
        if data["look"][key].as_array().is_none_or(|v|v.len()<3){continue;}
        let count=clamp(jsrt::conv::js_round(data["look"][key][1].as_f64().unwrap_or(0.)),0.,65536.) as usize;
        let prefix=data["look"][key][0].as_str().unwrap_or("");
        let names=(0..count).map(|i|format!("{prefix}{i}")).collect::<Vec<_>>();
        frames.insert(key.into(),json!({"names":names,"fps":data["look"][key][2]}));
    }
    let sea=grid!=0.&&west&&east&&north&&south;let ocean=first(&data["look"],"OceanEnable",0.)>0.;
    json!({"frames":frames,"oceanAlways":ocean,"ocean":ocean||sea,"sea":sea,"empty":!lo[0].is_finite(),"bounds":[lo,hi],"cells":boxes,"tint":rgba("RefractionColor",255.),"underwater":rgba("UnderwaterColor",0.)})
}
pub fn recipe(kind:u32,data:&Value)->Value {
    use crate::layout::environment_recipe as recipe;
    if kind==recipe::SNOW as u32{return snow(data);}
    if kind==recipe::WEATHER as u32{return weather(data);}
    if kind==recipe::ATMOSPHERE as u32{return atmosphere(data);}
    if kind==recipe::SOURCES as u32{return environment(data);}
    if kind==recipe::WATER as u32{return water(data);}
    if kind==recipe::FLYERS as u32{return json!(flyers(&data["data"],data["mission"].as_str().unwrap_or(""),data["enhanced"].as_bool().unwrap_or(false)));}
    if kind==recipe::FOUNTAIN_SKY as u32{return json!((0..3).map(|i|v8math::min(1.,data[i].as_f64().unwrap_or(0.)*1.4+[0.15,0.16,0.2][i])).collect::<Vec<_>>());}
    if kind==recipe::RADIUS as u32{
        let radius=array(data).fold(0f64,|r,b|{let x=b[0].as_f64().unwrap_or(0.);let y=b[1].as_f64().unwrap_or(0.);let z=b[2].as_f64().unwrap_or(0.);v8math::max(r,(x*x+y*y+z*z).sqrt()+b[3].as_f64().unwrap_or(0.))});return json!(radius);
    }
    Value::Null
}

#[cfg(target_arch="wasm32")]
mod exports {
    use super::*;use std::cell::RefCell;
    thread_local! {static INPUT:RefCell<Vec<u8>>=const{RefCell::new(Vec::new())};static OUTPUT:RefCell<Vec<u8>>=const{RefCell::new(Vec::new())};}
    #[no_mangle]pub extern "C" fn content_input(length:u32)->*mut u8 {INPUT.with_borrow_mut(|v|{v.resize(length as usize,0);v.as_mut_ptr()})}
    #[no_mangle]pub extern "C" fn content_recipe(kind:u32)->u32 {
        let value=INPUT.with_borrow(|v|serde_json::from_slice::<Value>(v));
        match value{Ok(v)=>{if kind==crate::layout::environment_recipe::WATER as u32{crate::surface_policy::configure(&v);}
            OUTPUT.with_borrow_mut(|out|*out=serde_json::to_vec(&recipe(kind,&v)).unwrap_or_default());0},Err(_)=>1}
    }
    #[no_mangle]pub extern "C" fn content_output()->*const u8 {OUTPUT.with_borrow(|v|v.as_ptr())}
    #[no_mangle]pub extern "C" fn content_length()->u32 {OUTPUT.with_borrow(|v|v.len() as u32)}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]fn snow_and_weather_limits_follow_authored_rounding(){
        let v=json!({"precipitation":{"ParticleDensity":[15.25],"ParticleDensityRange":[0.25],"Range":[100.],"AlphaMinMax":[0.4],"Velocity":[2.]}});
        let s=snow(&v);assert_eq!(s["count"],16.);assert_eq!(s["range"],60.);assert_eq!(s["alpha"],json!([0.4,0.4]));
        let r=weather(&v);assert_eq!(r["count"],32.);assert_eq!(r["enhancedRange"],40.);assert_eq!(r["sheetSpeed"],6.);
        assert_eq!(snow(&json!({"precipitation":{"ParticleDensity":[1e8]}}))["count"],2048.);
    }
    #[test]fn dust_selection_retains_authored_order_and_seed(){
        let data=json!({"dust":{"classes":[["z",{"count":2}],["a",{"count":1}]],"instances":[{"class":"a","position":[1,2,3]},{"class":"z","position":[4,5,6]}]}});
        let recipe=atmosphere(&data);let classes=&recipe["classes"];assert_eq!(classes[0]["name"],"z");assert_eq!(classes[0]["seed"],122);assert_eq!(classes[1]["seed"],97);
    }
    #[test]fn placed_sources_keep_transforms_basins_and_warmup(){
        let data=json!({"instances":[{"effect":"waterfountain","matrix":[1,0,0,0,0,1,0,0,0,0,1,0,10,2,20,1]}],
            "basins":[{"center":[9,3,20]},{"center":[11,8,20]}],"effects":[["mist",[{"transformer":{"lifetime":7.}}]]]});
        let r=environment(&data);assert_eq!(r["instances"][0]["position"],json!([10.,2.,20.]));
        assert_eq!(r["instances"][0]["level"],3.797);assert_eq!(r["instances"][0]["low"],2.95);assert_eq!(r["warmup"]["mist"],4.);
        assert_eq!(r["instances"][0]["sphere"],json!([10.,4.,20.,9.]));
    }
    #[test]fn water_bounds_distinguish_rounded_patches_and_cell_visibility(){
        let data=json!({"height":1.1,"cell":32.,"offset":[16.,16.],"grid":2,"cells":[[0,0],[1,1]],"look":{}});
        let r=water(&data);assert_eq!(r["sea"],true);assert_eq!(r["cells"][0],json!([-16.,0.10000000000000009,-16.,16.,2.1,16.]));
        assert_eq!(r["bounds"][0][1],(1.1f64 as f32 as f64)-1.);assert_eq!(r["underwater"][3],0.);
    }
}
