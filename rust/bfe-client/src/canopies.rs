//! Near tree crowns share the authored billboard footprints and retained selector.
use crate::{render_bridge::Bridge,resources::Resources,scene::View};
use bfe_render::{static_layout as L,scene::layout::{scene_canopy_view as V,scene_canopy_header as H,scene_canopy_selection as S}};
use serde_json::Value;
use three::{Matrix4,Vector3};
struct Group { center:Vec<f32>,size:Vec<f32>,shade:Vec<f32>,sphere:[f64;4] }
pub struct Canopies {data:Value,groups:Vec<Group>,texture:u32,state:u32,admitted:bool,pub enabled:bool}
fn array<const N:usize>(v:&Value)->Result<[f64;N],String> where [f64;N]:serde::de::DeserializeOwned{serde_json::from_value(v.clone()).map_err(|e|e.to_string())}
fn point(m:&Matrix4,p:[f64;3])->[f64;3]{let mut v=Vector3::new(p[0],p[1],p[2]);v.apply_matrix4(m);[v.x,v.y,v.z]}
impl Canopies {
    pub fn new(data:Value,texture:u32,state:u32)->Self{Self{data,texture,state,groups:vec![],admitted:false,enabled:false}}
    pub fn configure(&mut self,bridge:&mut Bridge,enabled:bool,sun:[f64;3])->Result<(),String>{
        self.enabled=enabled;
        if !enabled||self.admitted{return Ok(());}
        let instances=self.data["instances"].as_array().ok_or("missing canopy instances")?;
        for(index,source)in instances.iter().enumerate(){
            let mut matrix=Matrix4::default();matrix.elements=array(&source["matrix"])?;
            let lo:[f64;3]=array(&source["bounds"][0])?;let hi:[f64;3]=array(&source["bounds"][1])?;
            let mut min=[f64::INFINITY;3];let mut max=[f64::NEG_INFINITY;3];
            for k in 0..8{let p=point(&matrix,std::array::from_fn(|i|if k&(1<<i)==0{lo[i]}else{hi[i]}));for i in 0..3{min[i]=min[i].min(p[i]);max[i]=max[i].max(p[i]);}}
            let center=std::array::from_fn::<_,3,_>(|i|(min[i]+max[i])*0.5);
            let radius=(0..3).map(|i|(max[i]-center[i]).powi(2)).sum::<f64>().sqrt();
            let mut group=Group{center:Vec::with_capacity(L::CANOPIES_LEAVES*3),size:Vec::with_capacity(L::CANOPIES_LEAVES*2),shade:Vec::with_capacity(L::CANOPIES_LEAVES*3),sphere:[center[0],center[1],center[2],radius]};
            let mut state=71387u32.wrapping_add(index as u32*9127);
            let mut random=||{state=state.wrapping_mul(1664525).wrapping_add(1013904223);state as f64/4294967296.};
            let(width,height,depth)=(hi[0]-lo[0],hi[1]-lo[1],hi[2]-lo[2]);
            let mut emit=|center:[f64;3],radius:[f64;3],count:usize,random:&mut dyn FnMut()->f64|{
                let(mut shell,mut direction)=(1.,[0.;3]);
                for i in 0..count{
                    if i%4==0{let y=2.*random()-1.;let angle=random()*std::f64::consts::TAU;shell=0.78+random()*0.22;let rim=(1.-y*y).max(0.).sqrt();direction=[angle.cos()*rim,y,angle.sin()*rim];}
                    let size=2.7+random()*1.1;let margin=size*0.65;
                    let local=std::array::from_fn(|k|(direction[k]*radius[k]*shell*(0.25+0.25*(i%4) as f64)+center[k]).min(hi[k]-margin).max(lo[k]+margin));
                    group.center.extend(point(&matrix,local).map(|v|v as f32));group.size.extend([size as f32,(size*(0.9+random()*0.35)) as f32]);
                    let mut branch=Vector3::new(direction[0],direction[1],direction[2]);branch.transform_direction(&matrix);
                    let light=(-(branch.x*sun[0]+branch.y*sun[1]+branch.z*sun[2])).max(0.);let shade=0.47+light*0.21+random()*0.14;
                    group.shade.extend([(shade*0.97)as f32,shade as f32,(shade*0.91)as f32]);
                }
            };
            for row in 0..2{for column in 0..4{
                let rx=width*(0.105+random()*0.025);let ry=height*(0.22+random()*0.07);let rz=(depth*0.29).min(rx*0.95);
                let edge=((column as f64-1.5)/1.5).abs();let top=hi[1]-height*(0.06+0.17*edge*edge+row as f64*0.08);
                let center=[lo[0]+width*(0.15+column as f64*0.23)+(random()-0.5)*1.4,top-ry-random()*height*0.04,lo[2]+depth*if row!=0{0.67}else{0.3}];
                emit(center,[rx,ry,rz],84,&mut random);
            }}
            for column in 0..4{emit([lo[0]+width*(0.14+column as f64*0.24),lo[1]+height*0.25,lo[2]+depth*0.65],[width*0.16,height*0.24,depth*0.32],60,&mut random);}
            self.groups.push(group);
        }
        bridge.scene_canopy_reserve(self.groups.len() as u32,1);
        for (i,g)in self.groups.iter().enumerate(){bridge.r.scene.canopies.bounds[i*4..i*4+4].copy_from_slice(&g.sphere);}
        bridge.canopies_mesh((L::CANOPIES_MAX_GROUPS*L::CANOPIES_LEAVES) as u32)?;
        self.admitted=true;Ok(())
    }
    pub fn frame(&self,bridge:&mut Bridge,resources:&mut Resources,view:&View,shadows:bool)->Result<(),String>{
        if !self.admitted{return Ok(());}
        bridge.r.canopies.configure_shadows(&mut bridge.r.core,shadows)?;
        let selection=&mut bridge.r.scene.canopies;let v=&mut selection.view;
        v[V::GENERATION]=1.;v[V::CAMERA]=1.;v[V::ENABLED]=self.enabled as u8 as f64;v[V::LAYERS]=1.;v[V::NODE_LAYERS]=1.;
        v[V::PROJECTION..V::PROJECTION+16].copy_from_slice(&view.values[..16]);v[V::VIEW..V::VIEW+16].copy_from_slice(&view.values[16..32]);
        let mut world=Matrix4::default();world.elements.copy_from_slice(&view.values[16..32]);world.invert();
        v[V::WORLD..V::WORLD+16].copy_from_slice(&world.elements);v[V::EYE..V::EYE+3].copy_from_slice(&view.values[32..35]);
        let count=selection.select();let mut first=usize::MAX;let mut end=0;
        for i in 0..count{let row=&selection.output[H::STRIDE+i*S::STRIDE..][..S::STRIDE];if row[S::CHANGED]==0.{continue;}
            let group=&self.groups[row[S::ID]as usize];first=first.min(i*L::CANOPIES_LEAVES);end=(i+1)*L::CANOPIES_LEAVES;
            let max=L::CANOPIES_MAX_GROUPS*L::CANOPIES_LEAVES;
            for(offset,width,data)in [(0,3,&group.center),(max*3,2,&group.size),(max*5,3,&group.shade)]{let at=offset+i*L::CANOPIES_LEAVES*width;bridge.r.canopies.leaves[at..at+data.len()].copy_from_slice(data);}
        }
        if selection.output[H::CHANGED]!=0.{resources.update_canopy_state(self.state,crate::session::bytes(&selection.states))?;bridge.r.core.state.reset();}
        let c=&mut bridge.r.canopies.input;c[bfe_render::canopies::C_SHOWN]=(self.enabled&&count>0)as u8 as f64;c[bfe_render::canopies::C_SHADOWS]=shadows as u8 as f64;c[bfe_render::canopies::C_TEXTURE]=self.texture as f64;c[bfe_render::canopies::C_INSTANCES]=(count*L::CANOPIES_LEAVES)as f64;
        c[bfe_render::canopies::C_COPIED]=if first==usize::MAX{0.}else{first as f64};c[bfe_render::canopies::C_COPIED+1]=end as f64;
        Ok(())
    }
}
