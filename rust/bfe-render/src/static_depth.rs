//! Static depth resources formerly constructed by shadow-casters.js. Colour
//! and depth share the native authored source; no Three geometry/material is
//! needed to retain a caster, its Morton cells or its restoration recipe.
use crate::static_world::{Attribute,Geometry};
use crate::math::Frustum;
use std::collections::HashMap;
use three::{Matrix4,Vector3};

// worldMaterial explicitly set shadowSide to its colour side. Preserve that
// override instead of applying Three's default front/back shadow inversion.
pub fn shadow_side(record:&crate::static_world::Record)->u32 {if record.flags&0x86!=0 {2} else {0}}

pub struct Cell {pub first:u32,pub end:u32,pub min:[f64;3],pub max:[f64;3]}
pub struct DepthGeometry {pub geometry:Geometry,pub cells:Vec<Cell>,pub layer:u32,pub clipped:u32}
struct Part {x:f64,z:f64,count:usize,min:[f64;3],max:[f64;3]}
fn spread(value:f64)->u64 {
    // render-culling.js::mortonOrder uses ToInt32 before its bit masks. The
    // low sixteen bits also handle negative and unusually large cold inputs.
    let mut v=(value+32768.).rem_euclid(65536.) as u32;
    v=(v|(v<<8))&0x00ff00ff;v=(v|(v<<4))&0x0f0f0f0f;
    v=(v|(v<<2))&0x33333333;v=(v|(v<<1))&0x55555555;
    v as u64
}
fn bins(g:&Geometry,numeric:bool)->(Vec<u32>,Vec<Cell>) {
    let width=g.stride as usize/4;
    let p=|v:u32,k:usize|g.vertices[v as usize*width+k] as f64;
    let mut parts:Vec<Part>=vec![];let mut lookup=HashMap::new();let mut cell_of=Vec::with_capacity(g.index.len()/3);
    // mergeStaticShadows uses a numeric cell key; partitionShadowCaster uses
    // a coordinate pair. Preserve that distinction and first-appearance order.
    for t in g.index.as_chunks::<3>().0 {
        let x=((p(t[0],0)+p(t[1],0)+p(t[2],0))/3./48.).floor();
        let z=((p(t[0],2)+p(t[1],2)+p(t[2],2))/3./48.).floor();
        let key=if numeric {(((x+32768.)*65536.+(z+32768.)).to_bits(),0)} else {(if x==0. {0} else {x.to_bits()},if z==0. {0} else {z.to_bits()})};
        let next=parts.len();let at=*lookup.entry(key).or_insert_with(|| {parts.push(Part{x,z,count:0,min:[f64::INFINITY;3],max:[f64::NEG_INFINITY;3]});next});
        let part=&mut parts[at];part.count+=3;cell_of.push(at);
        for &v in t {for k in 0..3 {part.min[k]=part.min[k].min(p(v,k));part.max[k]=part.max[k].max(p(v,k));}}
    }
    // mortonOrder is stable for equal keys. Equal Morton codes must retain
    // first appearance even when very distant cells wrap their low 16 bits.
    let mut order:Vec<_>=(0..parts.len()).collect();
    order.sort_by_key(|i|spread(parts[*i].x)*2+spread(parts[*i].z));
    let mut cells:Vec<_>=parts.iter().map(|p|Cell{first:0,end:0,min:p.min,max:p.max}).collect();let mut cursor=0;
    for at in order {cells[at].first=cursor;cursor+=parts[at].count as u32;cells[at].end=cursor;}
    let mut fill:Vec<_>=cells.iter().map(|c|c.first as usize).collect();let mut sorted=vec![0;g.index.len()];
    for (t,&at) in g.index.as_chunks::<3>().0.iter().zip(&cell_of) {let first=fill[at];sorted[first..first+3].copy_from_slice(t);fill[at]+=3;}
    (sorted,cells)
}
impl DepthGeometry {
    // partitionShadowCaster keeps small or compact cutouts on the live layer;
    // large buckets draw one Morton span on the cached static layer. Fabric
    // always remains live because its depth vertices sway with the frame time.
    pub fn cutout(source:&Geometry)->Self {
        let mut geometry=Geometry {vertices:source.vertices.clone(),index:source.index.clone(),index_width:source.index_width,interleaved:source.interleaved,
            fabric:source.fabric,tree_groups:None,attributes:source.attributes.iter().filter(|a|a.name!="treeGroup").cloned().collect(),stride:source.stride,bounds:source.bounds};
        let large=!geometry.fabric&&geometry.index.len()>=1536&&(geometry.bounds[7]-geometry.bounds[4]>=48.||geometry.bounds[9]-geometry.bounds[6]>=48.);
        let mut cells=vec![];let mut layer=0;
        if large {let (index,parts)=bins(&geometry,false);if parts.len()>1 {geometry.index=index;cells=parts;layer=2;}}
        Self {geometry,cells,layer,clipped:0}
    }
    // mergeStaticShadows's one position-only mesh per shadow side. Apply the
    // identity world transform in f64 before Float32 assignment just as the JS
    // copier did, including signed-zero arithmetic and unused source vertices.
    pub fn merged(sources:&[&Geometry])->Self {
        let mut vertices=vec![];let mut index=vec![];let mut base=0u32;let mut clipped=0;
        // Bound merged streams as well as individual assets. Preserve the
        // admitted prefix when a synthetic scene exceeds the native budget.
        for (i,g) in sources.iter().enumerate() {let width=g.stride as usize/4;
            let count=g.vertices.len()/width;
            if base as usize+count>8_388_608 || index.len()+g.index.len()>50_331_648 {clipped=(sources.len()-i) as u32;break;}
            for row in g.vertices.chunks_exact(width) {let mut p=Vector3::new(row[0] as f64,row[1] as f64,row[2] as f64);p.apply_matrix4(&Matrix4::default());vertices.extend_from_slice(&[p.x as f32,p.y as f32,p.z as f32]);}
            index.extend(g.index.iter().map(|v|*v+base));base+=(g.vertices.len()/width) as u32;
        }
        let mut geometry=Geometry {vertices,index,index_width:4,interleaved:false,fabric:false,tree_groups:None,attributes:vec![Attribute{name:"position".into(),size:3,offset:0}],stride:12,bounds:[0.;10]};
        geometry.update_bounds();let (index,cells)=bins(&geometry,true);geometry.index=index;
        Self {geometry,cells,layer:2,clipped}
    }
    // The old cached onBeforeShadow hook submits the first-to-last visible
    // Morton span, including intervening cells. It does not compact indices.
    pub fn range(&self,frustum:&Frustum)->(u32,u32) {range(&self.geometry.bounds,self.geometry.index.len() as u32,&self.cells,frustum)}
}

pub fn range(bounds:&[f64;10],count:u32,cells:&[Cell],frustum:&Frustum)->(u32,u32) {
    if !frustum.sphere(&[bounds[0],bounds[1],bounds[2]],bounds[3]) {return (0,0);}
    // Unpartitioned cutouts retain their complete range after sphere culling.
    // Partitioned groups preserve gaps between the first and last visible cell.
    if cells.is_empty() {return (0,count);}
    let (mut first,mut end)=(u32::MAX,0);
    for cell in cells {if frustum.aabb(&cell.min,&cell.max) {first=first.min(cell.first);end=end.max(cell.end);}}
    if first==u32::MAX {(0,0)} else {(first,end-first)}
}

// Native shadow resources retain GPU ids and bounds/ranges, sharing source
// streams with the colour bank. Scene traversal publishes only descriptor ids.
#[derive(Clone,Copy)]
pub struct Source {pub source:u32,pub texture:i32}
pub struct Record {
    pub geometry:u32,pub program:usize,pub side:u32,pub texture:i32,
    pub bounds:[f64;10],pub cells:Vec<Cell>,pub count:u32,pub active:bool,
    pub uniforms:Vec<(usize,u32,u32)>,
}
pub const MAX_RECORDS:usize=4096;
#[derive(Default)]
pub struct Bank {pub sources:Vec<Source>,pub records:Vec<Option<Record>>,pub clipped:u32}
impl Bank {
    // Admission is cold; a larger-than-content scene clips depth resources in
    // source order with telemetry instead of throwing in a slow frame.
    pub fn admit(&mut self,source:u32,texture:i32)->u32 {
        if self.sources.len()>=MAX_RECORDS-2 {self.clipped=self.clipped.saturating_add(1);return u32::MAX;}
        let id=self.sources.len() as u32;self.sources.push(Source{source,texture});id
    }
    pub fn draw<C:glow::HasContext,M:crate::core::MultiDraw>(&self, core:&mut crate::core::Core<C,M>,matrices:&mut HashMap<(usize,usize),[f32;16]>,id:usize,view:&[f64],frustum:&Frustum) {
        let Some(Some(r))=self.records.get(id) else {return;};
        if !r.active || r.program>=core.programs.len() || r.geometry as usize>=core.geometries.len() {return;}
        let (first,count)=range(&r.bounds,r.count,&r.cells,frustum);if count==0 {return;}
        core.state.use_program(&core.gl,&mut core.stats,core.programs[r.program].program);
        // shadow-casters.js anchors were identity children of static world
        // meshes. Only the light camera changes between these native draws.
        let mut unit=0;
        let identity=[1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.];
        let mut camera_view=[0.;16];camera_view.copy_from_slice(&view[16..32]);
        let model_view=crate::math::mul(&camera_view,&identity);
        // Slots and value kinds were resolved cold. Stack uniform records
        // avoid per-object host calls, name dispatch or frame allocations.
        for &(slot,ty,kind) in &r.uniforms {
            let mut u=[0.;crate::post::UNIFORM];u[0]=slot as f64;u[1]=ty as f64;
            match kind {
                0=>u[2..18].copy_from_slice(&model_view),
                1=>u[2..18].copy_from_slice(&view[..16]),
                2=>u[2]=view[32],3=>u[2]=r.texture as f64,4=>u[2]=0.5,
                6=>{u[3] = -0.;u[2]=1.;u[6]=1.;u[10]=1.;},7=>u[2..18].copy_from_slice(&camera_view),
                _=>{u[2]=1.;u[7]=1.;u[12]=1.;u[17]=1.;}
            }
            crate::post::set_uniform(core,matrices,r.program,&u,&mut unit);
        }
        let (gl,state,stats)=(&core.gl,&mut core.state,&mut core.stats);
        state.cull(gl,stats,r.side);state.front_face(gl,stats,glow::CCW);
        let g=&core.geometries[r.geometry as usize];
        unsafe {gl.bind_vertex_array(Some(g.vao));gl.draw_elements(glow::TRIANGLES,count as i32,g.index_type,first as i32*g.index_bytes);}
        stats.gl_calls+=2;stats.pass_draws[crate::core::PASS_SHADOW]+=1;
    }
}

#[cfg(test)]
mod capacity_tests {
    use super::*;
    #[test]
    fn depth_admission_over_content_capacity_is_bounded() {
        let mut bank=Bank::default();
        for i in 0..MAX_RECORDS*3 {let id=bank.admit(i as u32,-1);assert_eq!(id,if i<MAX_RECORDS-2 {i as u32} else {u32::MAX});}
        assert_eq!(bank.sources.len(),MAX_RECORDS-2);
        assert_eq!(bank.clipped as usize,MAX_RECORDS*2+2);
    }
}
