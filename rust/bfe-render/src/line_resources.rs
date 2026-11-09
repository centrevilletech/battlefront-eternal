//! drawTethers and FlightDebug publish positions; immutable line descriptors
//! own topology, colour conversion, raster state and uniform bindings. Utility
//! shader programs are admitted separately from their geometry and appearance.
use crate::{core::{Core,MultiDraw},frame::FrameBank,frame_plan::layout::{line as L,limit as B},line_layout::{kind as K,limit as C,view as V,row as R},math,post as P};
use glow::HasContext;

#[derive(Debug)]
pub struct Descriptor {pub color:[f64;3],pub depth:bool,pub segments:bool,pub fixed:bool}
impl Descriptor {
    // Graphics disables ColorManagement: LineBasicMaterial.setHex preserves
    // raw gamma-space channels. Its default white stays exactly one.
    pub fn new(kind:u32)->Option<Self>{
        let rgb=match kind as usize {K::TOW=>[183.,181.,171.],K::FLIGHT=>[0.,255.,255.],K::AXES=>[255.;3],_=>return None};
        let color=rgb.map(|v|v/255.);
        Some(Self{color,depth:kind!=K::FLIGHT as u32,segments:kind!=K::TOW as u32,fixed:kind==K::AXES as u32})
    }
    // AxesHelper(12)'s six positions and endpoint colours retain its exact
    // Float32 storage order; animated helpers supply only their live points.
    pub fn axes()->([f32;18],[f32;18]){([0.,0.,0.,12.,0.,0.,0.,0.,0.,0.,12.,0.,0.,0.,0.,0.,0.,12.],[1.,0.,0.,1.,0.6,0.,0.,1.,0.,0.6,1.,0.,0.,0.,1.,0.,0.6,1.])}
}
struct Source {geometry:u32,buffer:u32,program:u32,capacity:usize,descriptor:Descriptor,uniforms:Vec<(usize,u32)>,serial:f64,center:Option<[f64;3]>}
pub struct Lines {sources:Vec<Source>,pub input:Vec<f64>,pub data:Vec<f32>,order:Vec<(usize,f64,f64,f64,f64)>,pub clipped:u64}
impl Default for Lines {fn default()->Self{Self{sources:vec![],input:vec![0.;V::LEN+C::SOURCES*R::LEN],data:vec![0.;C::SOURCES*C::VERTICES*3],order:Vec::with_capacity(C::SOURCES),clipped:0}}}
impl Lines {
    // LinesPass.discover compiles a utility program once. Native admission
    // resolves its numeric bindings and reserves the complete position VBO.
    pub fn admit<Cx:HasContext,Md:MultiDraw>(&mut self,core:&mut Core<Cx,Md>,program:u32,kind:u32,capacity:usize)->Result<Option<u32>,String>{
        if self.sources.len()==C::SOURCES{self.clipped=self.clipped.saturating_add(1);return Ok(None);}
        let descriptor=Descriptor::new(kind).ok_or("unknown line source")?;
        let p=core.programs.get(program as usize).ok_or("unknown line program")?;
        let uniforms=p.uniforms.iter().enumerate().filter(|(_,(_,location,_))|location.is_some()).map(|(slot,(name,_,_))|Ok((slot,match name.as_str(){"projectionMatrix"=>16,"modelViewMatrix"=>17,"modelMatrix"=>18,"viewMatrix"=>19,"cameraPosition"=>20,"normalMatrix"=>9,"isOrthographic"=>5,"diffuse"=>3,"opacity"=>1,_=>return Err(format!("unsupported line uniform {name}"))}))).collect::<Result<Vec<_>,String>>()?;
        if uniforms.len()>P::MAX_UNIFORMS{return Err("line uniform capacity".into());}
        let geometry=core.add_geometry();let capacity=if descriptor.fixed{6}else{capacity.clamp(1,C::VERTICES)};
        let vertices=if descriptor.fixed{Descriptor::axes().0.to_vec()}else{vec![0.;capacity*3]};
        let buffer=core.geometry_buffer(geometry,crate::core::bytes(&vertices));
        // plainProgram imports utility attributes in declared numeric order.
        // Both layouts start with position; the axis program also reads colour.
        core.geometry_pointer(geometry,buffer,0,3,glow::FLOAT,false,0,0);
        if descriptor.fixed{let color=core.geometry_buffer(geometry,crate::core::bytes(&Descriptor::axes().1));core.geometry_pointer(geometry,color,1,3,glow::FLOAT,false,0,0);}
        core.geometry_primitive(geometry,if descriptor.segments{glow::LINES}else{glow::LINE_STRIP},0,false,1.);
        let id=self.sources.len() as u32;self.sources.push(Source{geometry,buffer,program,capacity,descriptor,uniforms,serial:f64::NAN,center:None});core.state.reset();Ok(Some(id))
    }
    // LinesPass.prepare's one numeric publication for a view. Source positions
    // upload only after their producer serial changes; matrices remain per view.
    // Counts and ids clip before slicing either transport or GPU storage.
    pub fn frame<Cx:HasContext,Md:MultiDraw>(&mut self,core:&mut Core<Cx,Md>,frames:&mut FrameBank<Cx>,view:usize){
        if view>=3{return;}
        let prior=self.clipped;let count=(self.input[V::COUNT] as usize).min(C::SOURCES);self.order.clear();
        let projection:math::Mat4=self.input[V::PROJECTION..V::PROJECTION+16].try_into().unwrap();
        let inverse:math::Mat4=self.input[V::INVERSE..V::INVERSE+16].try_into().unwrap();
        let screen=math::mul(&projection,&inverse);
        for i in 0..count{
            let at=V::LEN+i*R::LEN;let raw=self.input[at+R::SOURCE];if !raw.is_finite()||raw<0.{self.clipped=self.clipped.saturating_add(1);continue;}let id=raw as usize;
            let Some(source)=self.sources.get_mut(id)else{self.clipped=self.clipped.saturating_add(1);continue};
            let requested=self.input[at+R::COUNT] as usize;let n=if source.descriptor.fixed{6}else{requested.min(source.capacity)};
            if n==0{continue;}self.clipped=self.clipped.saturating_add(requested.saturating_sub(n) as u64);
            if !source.descriptor.fixed&&source.serial!=self.input[at+R::SERIAL]{
                let data=&self.data[id*C::VERTICES*3..id*C::VERTICES*3+n*3];
                core.geometry_buffer_sub_data(source.geometry,source.buffer,0,crate::core::bytes(data));source.serial=self.input[at+R::SERIAL];
                // Tow geometry keeps its first computed bounding sphere. A
                // FlightDebug replacement geometry recomputes it each update.
                if source.center.is_none()||source.descriptor.segments{source.center=Some(bounds_center(data));}
            }
            let world:math::Mat4=self.input[at+R::WORLD..at+R::WORLD+16].try_into().unwrap();
            let center=source.center.unwrap_or([6.;3]);let z=crate::transparent::three_z(&screen,Some(&world),&center);
            self.order.push((i,self.input[at+R::GROUP_ORDER],self.input[at+R::RENDER_ORDER],z,self.input[at+R::OBJECT]));
        }
        // Three's opaque list orders material identities before depth. Native
        // admission preserves that creation order without a retained Material.
        self.order.sort_unstable_by(|a,b|a.1.total_cmp(&b.1).then(a.2.total_cmp(&b.2)).then_with(||{
            let source=|i:usize|self.input[V::LEN+i*R::LEN+R::SOURCE] as usize;source(a.0).cmp(&source(b.0))
        }).then(a.3.total_cmp(&b.3)).then(a.4.total_cmp(&b.4)));
        let mut shown=0;
        for &(i,group,order,z,object) in &self.order{
            if shown==B::LINES{self.clipped=self.clipped.saturating_add(1);break;}
            let at=V::LEN+i*R::LEN;let source=&self.sources[self.input[at+R::SOURCE] as usize];
            let out=(view*B::LINES+shown)*L::LEN;if out+L::LEN>frames.lines.len(){self.clipped=self.clipped.saturating_add(1);break;}
            let row=&mut frames.lines[out..out+L::LEN];row.fill(0.);row[L::KEY..L::KEY+4].copy_from_slice(&[group,order,z,object]);
            let s=L::STEP;row[s+P::STEP_PROGRAM]=source.program as f64;row[s+P::STEP_TARGET]=P::TARGET_BOUND;row[s+P::STEP_UNIFORMS]=source.uniforms.len() as f64;
            let world:math::Mat4=self.input[at+R::WORLD..at+R::WORLD+16].try_into().unwrap();let model_view=math::mul(&inverse,&world);
            for (u,&(slot,kind)) in source.uniforms.iter().enumerate(){let p=s+P::STEP_FIRST+u*P::UNIFORM;row[p]=slot as f64;row[p+1]=if matches!(kind,17..=19){16.}else if kind==20{3.}else{kind as f64};match kind{
                16=>row[p+2..p+18].copy_from_slice(&projection),17=>row[p+2..p+18].copy_from_slice(&model_view),18=>row[p+2..p+18].copy_from_slice(&world),19=>row[p+2..p+18].copy_from_slice(&inverse),20=>row[p+2..p+5].copy_from_slice(&self.input[V::WORLD+12..V::WORLD+15]),9=>row[p+2..p+11].copy_from_slice(&math::normal_matrix(&model_view)),5=>row[p+2]=self.input[V::ORTHOGRAPHIC],3=>row[p+2..p+5].copy_from_slice(&source.descriptor.color),_=>row[p+2]=1.,
            }}
            let s=L::STATE;row[s+P::STATE_FLAGS]=(P::STATE_OWN_GEOMETRY|P::STATE_DEPTH_WRITE|if source.descriptor.depth{P::STATE_DEPTH_TEST}else{0}) as f64;row[s+P::STATE_DEPTH_FUNC]=glow::LEQUAL as f64;
            row[s+P::STATE_GEOMETRY]=source.geometry as f64;row[s+P::STATE_GEOMETRY+1]=if source.descriptor.fixed{6.}else{self.input[at+R::COUNT].max(0.).min(source.capacity as f64)};shown+=1;
        }
        frames.line_counts[view]=shown;if self.clipped!=prior{frames.clipped=true;}core.state.reset();
    }
}
fn bounds_center(data:&[f32])->[f64;3]{let mut lo=[f64::INFINITY;3];let mut hi=[f64::NEG_INFINITY;3];for p in data.as_chunks::<3>().0{for k in 0..3{lo[k]=lo[k].min(p[k] as f64);hi[k]=hi[k].max(p[k] as f64);}}std::array::from_fn(|k|(lo[k]+hi[k])*0.5)}
