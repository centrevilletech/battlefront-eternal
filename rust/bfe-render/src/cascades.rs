//! Graphics.fitCascades/updateCascadeWindows and LightShadow.updateMatrices.
//! Each window retains its static cache until the authored coverage fails.
use crate::cameras::{Camera,bias,vector,layout::{input as I,cascade as O,lens as N,limit as L}};
use three::{Vector3 as V,Matrix4,Matrix3,Box3};
use three::matrix::CoordinateSystem::WebGL;
use jsrt::{v8math as m,conv::js_min};
use crate::shader::{SHADOW_VALUES,SHADOW_MATRICES,SHADOW_PLANES,SHADOW_SLICES};

#[derive(Clone,Copy)]
struct Window {center:V,half:f64,held:bool,dirty:bool,moved:bool}
impl Default for Window {fn default()->Self{Self{center:V::default(),half:0.,held:false,dirty:true,moved:false}}}
pub struct Cascades {
    pub output:Vec<f64>,pub(crate) cameras:[Camera;L::CASCADES],pub count:usize,
    windows:[Window;L::CASCADES],vertices:[[V;8];L::CASCADES],breaks:[f64;L::CASCADES],
    matrices:[Matrix4;L::CASCADES],published:[Matrix4;L::CASCADES],planes:[Matrix3;L::CASCADES],
    pub range:[f64;2],slices:[[f64;2];L::CASCADES],lens:[f64;4],configuration:[f64;5],reset:f64,
}
impl Default for Cascades {
    fn default()->Self {Self{output:vec![0.;L::CASCADES*O::LEN],cameras:[Camera::default();L::CASCADES],count:0,
        windows:[Window::default();L::CASCADES],vertices:[[V::default();8];L::CASCADES],breaks:[0.;L::CASCADES],
        matrices:[Matrix4::default();L::CASCADES],published:[Matrix4::default();L::CASCADES],planes:[Matrix3::default();L::CASCADES],
        range:[0.5,200.],slices:[[0.;2];L::CASCADES],lens:[f64::NAN;4],configuration:[f64::NAN;5],reset:f64::NAN}}
}
impl Cascades {
    // CascadeWindow.invalidate is a cold semantic command. Rebuilding a
    // target invalidates coverage without losing the last published matrices.
    pub fn invalidate(&mut self){for i in 0..L::CASCADES{self.invalidate_window(i);}}
    pub fn invalidate_window(&mut self,index:usize){if let Some(w)=self.windows.get_mut(index){w.held=false;w.dirty=true;self.output[index*O::LEN+O::HELD]=0.;self.output[index*O::LEN+O::DIRTY]=1.;}}
    fn reset(&mut self){self.output.fill(0.);for i in 0..L::CASCADES{self.output[i*O::LEN+O::DIRTY]=1.;}self.windows.fill(Window::default());self.published.fill(Matrix4::default());self.planes.fill(Matrix3::default());self.lens.fill(f64::NAN);}
    // CSM.getBreaks/initCascades/updateShadowBounds plus Graphics' overlap.
    // View-space corners are projected first, then clipped to maxFar, then
    // interpolated. Reordering those operations changes small shadow texels.
    fn fit(&mut self,input:&[f64],camera:Camera){
        let near=camera.lens[N::NEAR];let far=js_min(camera.lens[N::FAR],input[I::MAX_FAR]);
        for i in 1..self.count{let log=near*m::pow(far/near,i as f64/self.count as f64);let uniform=near+(far-near)*i as f64/self.count as f64;self.breaks[i-1]=(0.85*log+0.15*uniform)/far;}
        if self.count>0{self.breaks[self.count-1]=1.;}
        // CSMFrustum projects the near/far corners before interpolating any
        // split. Clipping an already split corner is not bit equivalent.
        let mut inverse=camera.projection;inverse.invert();let signs=[(1.,1.),(1.,-1.),(-1.,-1.),(-1.,1.)];let mut main=[V::default();8];
        for(i,(x,y))in signs.into_iter().enumerate(){main[i]=V::new(x,y,-1.);main[i].apply_matrix4(&inverse);main[i+4]=V::new(x,y,1.);main[i+4].apply_matrix4(&inverse);let z=main[i+4].z.abs();main[i+4].multiply_scalar(js_min(input[I::MAX_FAR]/z,1.));}
        for i in 0..self.count {
            let mut vertices=[V::default();8];
            for j in 0..4 {
                if i==0{vertices[j]=main[j];}else{vertices[j].lerp_vectors(&main[j],&main[j+4],self.breaks[i-1]);}
                if i+1==self.count{vertices[j+4]=main[j+4];}else{vertices[j+4].lerp_vectors(&main[j],&main[j+4],self.breaks[i]);}
            }
            if i>0 {let end=self.breaks[i-1];let start=if i>1{self.breaks[i-2]}else{0.};let overlap=end-(end-start)*0.1;for j in 0..4{vertices[j].lerp_vectors(&main[j],&main[j+4],overlap);}}
            let p=vertices[4];let q=if p.distance_to(&vertices[6])>p.distance_to(&vertices[2]){vertices[6]}else{vertices[2]};let width=p.distance_to(&q);
            let left=-width/2.;let right=width/2.;let top=width/2.;let bottom=-width/2.;
            // OrthographicCamera.updateProjectionMatrix recomputes the
            // centre and half extents even at zoom 1. Keep its signed zeros.
            let dx=(right-left)/2.;let dy=(top-bottom)/2.;let cx=(right+left)/2.;let cy=(top+bottom)/2.;
            let mut c=Camera::default();c.lens[N::NEAR]=input[I::LIGHT_RANGE];c.lens[N::FAR]=input[I::LIGHT_RANGE+1];c.lens[N::ZOOM]=1.;
            c.projection.make_orthographic(cx-dx,cx+dx,cy+dy,cy-dy,c.lens[N::NEAR],c.lens[N::FAR],WebGL);c.projection_inverse=c.projection;c.projection_inverse.invert();
            self.cameras[i]=c;self.vertices[i]=vertices;
            let at=i*O::LEN;self.output[at+O::EXTENT..at+O::EXTENT+4].copy_from_slice(&[left,right,top,bottom]);
        }
    }
    // applyShadows replaces light shells before a possible mirror. Retire
    // their matrices immediately, but keep receiver slices until the old
    // updateCascades boundary publishes the next camera's fitted breaks.
    pub fn configure(&mut self,input:&[f64]){
        let count=(input[I::CASCADE_COUNT] as usize).min(L::CASCADES);
        let configuration=[count as f64,input[I::MAP_SIZE],input[I::MAX_FAR],input[I::LIGHT_RANGE],input[I::LIGHT_RANGE+1]];
        if configuration!=self.configuration||input[I::RESET]!=self.reset {self.reset();self.configuration=configuration;self.reset=input[I::RESET];}
        self.count=count;
    }
    pub(crate) fn prepare(&mut self,input:&[f64],camera:Camera){
        self.configure(input);let count=self.count;
        let lens=[camera.lens[N::FOV],camera.lens[N::ASPECT],camera.lens[N::NEAR],camera.lens[N::FAR]];
        if lens!=self.lens {self.fit(input,camera);self.lens=lens;}
        self.range=[camera.lens[N::NEAR],js_min(camera.lens[N::FAR],input[I::MAX_FAR])];
        for i in 0..L::CASCADES{self.output[i*O::LEN+O::RANGE..i*O::LEN+O::RANGE+2].copy_from_slice(&self.range);self.slices[i]=[if i>0&&i-1<count{self.breaks[i-1]}else{0.},if i<count{self.breaks[i]}else{1.}];}
        // Graphics.updateCascadeWindows uses a rotation-only light frame.
        // Its held centre remains in that space until the coverage fails.
        let direction=vector(input,I::SUN);let mut orientation=Matrix4::default();orientation.look_at(&V::default(),&direction,&V::new(0.,1.,0.));let mut inverse=orientation;inverse.invert();let mut to_light=Matrix4::default();to_light.multiply_matrices(&inverse,&camera.world);
        for i in 0..count {
            // The old bounds walk interleaves near and far corners. Keep
            // that order and the strict held-window coverage inequalities.
            let mut bounds=Box3::default();for j in 0..4 {let mut p=self.vertices[i][j];p.apply_matrix4(&to_light);bounds.expand_by_point(&p);let mut p=self.vertices[i][j+4];p.apply_matrix4(&to_light);bounds.expand_by_point(&p);}
            let at=i*O::LEN;let half=self.output[at+O::EXTENT+1];let texel=(half-self.output[at+O::EXTENT])/input[I::MAP_SIZE];let margin=input[I::MARGIN];let w=&mut self.windows[i];let c=w.center;
            let held=w.held&&w.half==half&&bounds.min.x>=c.x-half&&bounds.max.x<=c.x+half&&bounds.min.y>=c.y-half&&bounds.max.y<=c.y+half&&bounds.max.z+margin*0.5<=c.z;
            if !held {bounds.get_center(&mut w.center);w.center.z=bounds.max.z+margin;w.center.x=(w.center.x/texel).floor()*texel;w.center.y=(w.center.y/texel).floor()*texel;w.half=half;w.held=true;w.dirty=true;w.moved=true;}
            let mut position=w.center;position.apply_matrix4(&orientation);let mut target=position;target.add(&direction);
            let c=&mut self.cameras[i];c.p=position;c.up=V::new(0.,1.,0.);let mut rotation=Matrix4::default();rotation.look_at(&position,&target,&c.up);c.q.set_from_rotation_matrix(&rotation);c.world.compose(&position,&c.q,&V::new(1.,1.,1.));c.inverse=c.world;c.inverse.invert();
            let mut projection_view=Matrix4::default();projection_view.multiply_matrices(&c.projection,&c.inverse);let mut matrix=bias();matrix.multiply(&projection_view);self.matrices[i]=matrix;
            self.output[at+O::CENTER..at+O::CENTER+3].copy_from_slice(&[w.center.x,w.center.y,w.center.z]);self.output[at+O::HALF]=w.half;self.output[at+O::HELD]=w.held as u8 as f64;
            self.output[at+O::DIRTY]=w.dirty as u8 as f64;self.output[at+O::MOVED]=w.moved as u8 as f64;
            self.output[at+O::BREAKS]=if i==0{0.}else{self.breaks[i-1]};self.output[at+O::BREAKS+1]=self.breaks[i];
            for(j,v)in self.vertices[i].iter().enumerate(){self.output[at+O::VERTICES+j*3..at+O::VERTICES+j*3+3].copy_from_slice(&[v.x,v.y,v.z]);}
            self.output[at+O::POSITION..at+O::POSITION+3].copy_from_slice(&[position.x,position.y,position.z]);self.output[at+O::TARGET..at+O::TARGET+3].copy_from_slice(&[target.x,target.y,target.z]);
        }
    }
    // Mirrors read the preceding shadow publication. Only a selected shadow
    // pass advances its matrix/plane, at LightShadow.updateMatrices' boundary.
    pub fn publish(&mut self,index:usize){
        if index>=self.count{return;}self.published[index]=self.matrices[index];let mut plane=Matrix3::default();plane.set_from_matrix4(&self.published[index]).invert().transpose();self.planes[index]=plane;
        let at=index*O::LEN;self.output[at+O::MATRIX..at+O::MATRIX+16].copy_from_slice(&self.published[index].elements);self.output[at+O::PLANE..at+O::PLANE+9].copy_from_slice(&plane.elements);
    }
    pub fn finish(&mut self,drawn:&[f64]){for i in 0..self.count{if drawn.get(i).copied().unwrap_or(0.)!=0.{self.windows[i].dirty=false;}self.windows[i].moved=false;self.output[i*O::LEN+O::DIRTY]=self.windows[i].dirty as u8 as f64;self.output[i*O::LEN+O::MOVED]=0.;}}
    // The existing std140 block includes unused receiver slices as well.
    // Keep all four slices, while only current lights own matrices/metadata.
    pub fn write_shadow(&self,out:&mut[f64],metadata:&[f64]){
        out.fill(0.);for i in 0..L::CASCADES{out[SHADOW_SLICES+i*4..SHADOW_SLICES+i*4+2].copy_from_slice(&self.slices[i]);}
        for i in 0..self.count {out[SHADOW_VALUES+i*8..SHADOW_VALUES+i*8+6].copy_from_slice(&metadata[i*6..i*6+6]);out[SHADOW_MATRICES+i*16..SHADOW_MATRICES+i*16+16].copy_from_slice(&self.published[i].elements);for column in 0..3{for row in 0..3{out[SHADOW_PLANES+i*12+column*4+row]=self.planes[i].elements[column*3+row];}}}
    }
}
