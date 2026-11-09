//! Native glow, beam and shield policies formerly in glowModel,
//! BuildHologram and ShieldBubble. Live values use generated effect rows;
//! no material names, uniform introspection or texture versions cross a frame.
use crate::effect_layout as e;
#[derive(Default,serde::Deserialize)]
#[serde(default,rename_all="PascalCase")]
pub struct Look {pub color:Option<Vec<f64>>,pub specular_color:Option<Vec<f64>>}
#[derive(serde::Serialize)]
pub struct Policy {pub keys:Vec<(String,u32)>,pub blend:[u32;7],pub values:String,pub defaults:Vec<f64>}
impl Policy {
    // The old basic/glow double-sided draws execute back then front. Shield
    // forceSinglePass keeps its one double-sided draw; aura keeps front only.
    // BasicEffect, ShieldBubble and glowModel retain their existing catalog
    // programs and native value-cache layout through every context rebuild.
    pub fn new(kind:usize,shadows:bool,view:bool,texture:i32,look:&Look)->Result<Self,&'static str>{
        let (name,sides,blend)=match kind{
            e::ACTOR_GLOW=>("glow-basic",&[1,0][..],[1,32774,32774,770,1,770,1]),
            e::ACTOR_BASIC=>("basic",&[1,0][..],[1,32774,32774,770,1,770,1]),
            e::ACTOR_SHIELD=>("shield",&[2][..],[1,32774,32774,770,771,1,771]),
            e::ACTOR_AURA=>("shield-aura",&[0][..],[1,32774,32774,770,1,770,1]),
            _=>return Err("unknown actor effect")};
        let keys=sides.iter().map(|side|(format!("effects/{}{}-s{side}{}",if view{"view-"}else{""},name,if shadows&&!view{"-shadow"}else{""}),*side)).collect();
        let fields=e::PRODUCERS[kind].1;let mut values=vec![];let mut defaults=vec![];
        for &(name,size)in fields{let at=defaults.len();defaults.resize(at+size as usize,0.);let form=if name=="impacts"{4}else if matches!(name,"powers"|"blasts"){1}else{0};values.push(format!("{name}:{size}:{form}"));match name{
            "mapTransform"=>defaults[at..at+9].copy_from_slice(&[1.,-0.,0.,0.,1.,0.,0.,0.,1.]),
            "diffuse"=>defaults[at..at+3].fill(1.),"opacity"|"charge"|"enhanced"=>defaults[at]=1.,"birth"=>defaults[at]=10.,"hasMap"=>defaults[at]=if texture>=0{1.}else{0.},
            "fieldColor"|"specularColor"=>{let fallback=if name=="fieldColor"{[96.,160.,192.]}else{[216.,240.,255.]};let source=if name=="fieldColor"{&look.color}else{&look.specular_color};for i in 0..3{defaults[at+i]=source.as_ref().and_then(|v|v.get(i)).copied().unwrap_or(fallback[i])/255.;}},
            "impacts"=>for i in 0..6{defaults[at+i*4+2]=1.;defaults[at+i*4+3] = -1000.;},_=>{}}}
        Ok(Self{keys,blend,values:values.join(","),defaults})
    }
}
