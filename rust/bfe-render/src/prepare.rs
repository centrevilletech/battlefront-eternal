//! Loading policy from Graphics.warmupPaced and actorKeys/effectProgramKeys.
//! The browser supplies settings and pacing; Rust selects and links the catalog.
#[path="program_warmup.rs"] mod catalog;
pub struct WarmSpec { pub key:&'static str, pub family:u32, pub cascades:i32, pub lamps:i32,
    pub dynamic:bool, pub derived:bool, pub shadow:i32, pub csm:i32 }
impl WarmSpec {
    fn matches(&self,family:u32,cascades:u32,lamps:bool,shadows:bool)->bool {
        if self.family!=family || self.derived {return false;}
        if self.cascades>0 && self.cascades!=cascades.min(4) as i32 {return false;}
        if self.lamps>=0 && (self.lamps!=0)!=(self.dynamic&&lamps) {return false;}
        if self.shadow>=0 && (self.shadow!=0)!=shadows {return false;}
        if self.csm>=0 && (self.csm!=0)!=(cascades!=0) {return false;}
        true
    }
}
#[derive(Default)]
pub struct Prepare { cursor:usize, family:u32, cascades:u32, lamps:bool, shadows:bool }
impl Prepare {
    // Settings are bounded cold facts; unknown families simply finish empty.
    pub fn begin(&mut self,family:u32,cascades:u32,lamps:bool,shadows:bool) {
        self.cursor=0;self.family=family;self.cascades=cascades.min(4);self.lamps=lamps;self.shadows=shadows;
    }
    pub fn next_key(&mut self)->Option<&'static str> {
        while let Some(row)=catalog::PROGRAMS.get(self.cursor) {
            self.cursor+=1;
            if row.matches(self.family,self.cascades,self.lamps,self.shadows) {return Some(row.key);}
        }
        None
    }
    // Settings changes consume a cold catalog receipt rather than inspecting
    // browser materials or manufacturing shader defines during a live draw.
    pub fn keys(family:u32,cascades:u32,lamps:bool,shadows:bool)->String {
        let mut owner=Self::default();owner.begin(family,cascades,lamps,shadows);
        let mut keys=Vec::new();while let Some(key)=owner.next_key(){keys.push(key);}
        serde_json::to_string(&keys).unwrap_or_else(|_|"[]".into())
    }
}
// CPU-only catalog inspection also supports game-file-free host contracts.
#[cfg_attr(target_arch="wasm32",wasm_bindgen::prelude::wasm_bindgen)]
pub fn program_warmup_keys(family:u32,cascades:u32,lamps:bool,shadows:bool)->String {
    Prepare::keys(family,cascades,lamps,shadows)
}
// WorldPass.prepareNative can lose its context after the recipe is journaled
// but before its id reaches the browser. Replay resolves that exact cold call;
// resuming it must consume the replayed id instead of allocating another draw.
pub struct PendingWorld {
    source:u32, far:bool, scale:u64, samplers:Vec<i32>, features:Vec<u32>, order:u32, slot:u32, object:u32,
    pub item:Option<u32>,
}
impl PendingWorld {
    // Retains the ported JS/GL call-site signature without an extra wrapper struct.
    #[allow(clippy::too_many_arguments)]
    pub fn new(source:u32,far:bool,scale:f64,samplers:&[i32],features:&[u32],order:u32,slot:u32,object:u32)->Self {
        Self{source,far,scale:scale.to_bits(),samplers:samplers.to_vec(),features:features.to_vec(),order,slot,object,item:None}
    }
    // Retains the ported JS/GL call-site signature without an extra wrapper struct.
    #[allow(clippy::too_many_arguments)]
    pub fn matches(&self,source:u32,far:bool,scale:f64,samplers:&[i32],features:&[u32],order:u32,slot:u32,object:u32)->bool {
        self.source==source&&self.far==far&&self.scale==scale.to_bits()&&self.samplers==samplers&&self.features==features&&self.order==order&&self.slot==slot&&self.object==object
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn interrupted_world_admission_qualifies_the_complete_request(){
        let row=PendingWorld::new(7,false,-0.,&[-1,3],&[2,4],9,11,13);
        assert!(row.matches(7,false,-0.,&[-1,3],&[2,4],9,11,13));
        assert!(!row.matches(8,false,-0.,&[-1,3],&[2,4],9,11,13));
        assert!(!row.matches(7,true,-0.,&[-1,3],&[2,4],9,11,13));
        assert!(!row.matches(7,false,0.,&[-1,3],&[2,4],9,11,13));
        assert!(!row.matches(7,false,-0.,&[-1,4],&[2,4],9,11,13));
        assert!(!row.matches(7,false,-0.,&[-1,3],&[2,5],9,11,13));
        assert!(!row.matches(7,false,-0.,&[-1,3],&[2,4],10,11,13));
        assert!(!row.matches(7,false,-0.,&[-1,3],&[2,4],9,12,13));
        assert!(!row.matches(7,false,-0.,&[-1,3],&[2,4],9,11,14));
    }
    #[test] fn utility_catalog_is_complete_and_unknown_families_are_empty(){
        let mut p=Prepare::default();p.begin(3,u32::MAX,true,true);
        let mut count=0;while let Some(key)=p.next_key(){assert!(key.starts_with("utility/line-"));count+=1;}
        assert_eq!(count,24);for _ in 0..100000{assert_eq!(p.next_key(),None);}
        p.begin(u32::MAX,4,true,true);assert_eq!(p.next_key(),None);
    }
    #[test] fn settings_select_exact_shadow_twins_and_actor_lamp_variants(){
        for cascades in [0,1,2,3,4] {for lamps in [false,true]{for shadows in [false,true]{
            let mut p=Prepare::default();p.begin(1,cascades,lamps,shadows);
            while let Some(key)=p.next_key(){let row=catalog::PROGRAMS.iter().find(|r|r.key==key).unwrap();assert!(row.cascades<=0||row.cascades==cascades as i32);assert!(!row.derived);}
            p.begin(2,cascades,lamps,shadows);let mut count=0;
            while let Some(key)=p.next_key(){let row=catalog::PROGRAMS.iter().find(|r|r.key==key).unwrap();assert!(row.shadow<0||(row.shadow!=0)==shadows);count+=1;}assert!(count>0);
        }}}
    }
}
