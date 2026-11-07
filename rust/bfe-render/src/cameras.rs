//! Graphics' derived cameras and light inputs, preserving Three's f64 order.
//! Scene discovery and object visibility remain borrowed facts. No camera
//! operation walks a registry, changes an actor pose or calls a browser hook.
#[path="camera_layout.rs"]
pub mod layout;
use layout::{input as I,camera as C,lens as N,source as U,role as R,limit as L,mirror as H,light as P,pack as K,sky as Y};
use three::{Matrix4,Quaternion,Vector3 as V,Vector4,Plane};
use three::matrix::CoordinateSystem::WebGL;
use jsrt::{v8math as m,conv::{js_min,js_max,to_uint32}};

#[derive(Clone,Copy)]
pub(crate) struct Camera {
    pub p:V,pub q:Quaternion,pub up:V,pub lens:[f64;N::LEN],
    pub world:Matrix4,pub inverse:Matrix4,pub projection:Matrix4,pub projection_inverse:Matrix4,
}
impl Default for Camera {
    fn default()->Self {Self{p:V::default(),q:Quaternion::default(),up:V::new(0.,1.,0.),lens:[0.;N::LEN],world:Matrix4::default(),inverse:Matrix4::default(),projection:Matrix4::default(),projection_inverse:Matrix4::default()}}
}
impl Camera {
    // PerspectiveCamera.updateProjectionMatrix, including zoom/film/view
    // offsets. The native gameplay camera can instead publish its matrices.
    fn project(&mut self) {
        let f=&self.lens;let near=f[N::NEAR];
        let mut top=near*m::tan((std::f64::consts::PI/180.)*0.5*f[N::FOV])/f[N::ZOOM];
        let mut height=2.*top;let mut width=f[N::ASPECT]*height;let mut left=-0.5*width;
        if f[N::VIEW_ENABLED]!=0. {
            left+=f[N::OFFSET_X]*width/f[N::FULL_WIDTH];top-=f[N::OFFSET_Y]*height/f[N::FULL_HEIGHT];
            width*=f[N::WIDTH]/f[N::FULL_WIDTH];height*=f[N::HEIGHT]/f[N::FULL_HEIGHT];
        }
        if f[N::FILM_OFFSET]!=0. {left+=near*f[N::FILM_OFFSET]/(f[N::FILM_GAUGE]*js_min(f[N::ASPECT],1.));}
        self.projection.make_perspective(left,left+width,top,top-height,near,f[N::FAR],WebGL);
        self.projection_inverse=self.projection;self.projection_inverse.invert();
    }
    // Object3D.updateMatrixWorld followed by Camera's inverse. These derived
    // cameras have no transformed parent; cube children compose separately.
    fn compose(&mut self,scale:V) {self.world.compose(&self.p,&self.q,&scale);self.inverse=self.world;self.inverse.invert();}
    fn look(&mut self,at:V) {let mut rotation=Matrix4::default();rotation.look_at(&self.p,&at,&self.up);self.q.set_from_rotation_matrix(&rotation);self.compose(V::new(1.,1.,1.));}
}
pub struct Cameras {
    pub settings:crate::camera_lights::LightSettings,
    pub input:Vec<f64>,pub output:Vec<f64>,pub cascades:super::cascades::Cascades,
    pub sky:Vec<f64>,pub light_input:Vec<f64>,pub packs:Vec<f64>,pub mirror:[f64;H::LEN],
    cameras:[Camera;R::COUNT],saved:[Matrix4;R::COUNT],shifted:[bool;R::COUNT],
    pub clipped:bool,last_main:[f64;I::FAR_RANGE],main_valid:bool,
}
impl Default for Cameras {
    fn default()->Self {
        let mut input=vec![0.;I::LEN];input[I::SCALE..I::SCALE+3].fill(1.);input[I::UP+1]=1.;
        input[I::QUATERNION+3]=1.;input[I::LENS+N::FOV]=60.;input[I::LENS+N::ASPECT]=1.;
        input[I::LENS+N::NEAR]=0.5;input[I::LENS+N::FAR]=200.;input[I::LENS+N::ZOOM]=1.;input[I::LENS+N::FILM_GAUGE]=35.;
        Self{settings:crate::camera_lights::LightSettings::default(),input,output:vec![0.;R::COUNT*C::LEN],cascades:super::cascades::Cascades::default(),sky:vec![0.;L::SKY*3],
            light_input:vec![0.;L::RAW_LIGHTS*P::LEN],packs:vec![0.;L::PACKS*K::LEN],mirror:[0.;H::LEN],cameras:[Camera::default();R::COUNT],saved:[Matrix4::default();R::COUNT],shifted:[false;R::COUNT],clipped:false,last_main:[0.;I::FAR_RANGE],main_valid:false}
    }
}
pub(crate) fn vector(a:&[f64],at:usize)->V {V::new(a[at],a[at+1],a[at+2])}
fn put(a:&mut[f64],at:usize,v:V) {a[at]=v.x;a[at+1]=v.y;a[at+2]=v.z;}
pub(crate) fn bias()->Matrix4 {Matrix4{elements:[0.5,0.,0.,0.,0.,0.5,0.,0.,0.,0.,0.5,0.,0.5,0.5,0.5,1.]}}
impl Cameras {
    // One retained row is both shader input and a readback facade for current
    // camera consumers. Publishing never allocates a return array in bindgen.
    pub(crate) fn publish(&mut self,role:usize,camera:Camera) {
        if role>=R::COUNT {self.clipped=true;return;}
        self.cameras[role]=camera;let out=&mut self.output[role*C::LEN..(role+1)*C::LEN];
        put(out,C::POSITION,camera.p);put(out,C::UP,camera.up);
        out[C::QUATERNION..C::QUATERNION+4].copy_from_slice(&[camera.q.x,camera.q.y,camera.q.z,camera.q.w]);
        out[C::LENS..C::LENS+N::LEN].copy_from_slice(&camera.lens);
        for(at,value)in[(C::WORLD,camera.world),(C::INVERSE,camera.inverse),(C::PROJECTION,camera.projection),(C::PROJECTION_INVERSE,camera.projection_inverse)] {out[at..at+16].copy_from_slice(&value.elements);}
        // Camera.getWorldDirection normalizes the positive matrix column
        // before negating it. Keep that order, including signed zero.
        let e=&camera.world.elements;let mut direction=V::new(e[8],e[9],e[10]);direction.normalize().negate();put(out,C::DIRECTION,direction);
        let matrix=crate::math::mul(&camera.projection.elements,&camera.inverse.elements);
        let frustum=crate::math::Frustum::from_matrix(&matrix);
        for(i,p)in frustum.planes.iter().enumerate(){let at=C::FRUSTUM+i*4;out[at..at+3].copy_from_slice(&p.n);out[at+3]=p.c;}
    }
    // PageCameras already owns the main view. Its unmodified f64 publication
    // is imported directly; ordinary scene clients supply the same raw pose.
    pub fn main(&mut self)->bool {
        // Mirror, probe and main preparation can share one publication. An
        // unchanged f64 record must not repeat either math or host readback.
        if self.main_valid && !self.shifted[R::MAIN] && self.input[..I::FAR_RANGE].iter().zip(self.last_main.iter()).all(|(a,b)|a.to_bits()==b.to_bits()){return false;}
        self.last_main.copy_from_slice(&self.input[..I::FAR_RANGE]);self.main_valid=true;
        // Source matrices preserve the gameplay owner's lazy placement and
        // shake boundary. Recomposition here would create a second solver.
        let a=&self.input;let mut c=Camera::default();c.p=vector(a,I::POSITION);c.up=vector(a,I::UP);
        c.q=Quaternion::new(a[I::QUATERNION],a[I::QUATERNION+1],a[I::QUATERNION+2],a[I::QUATERNION+3]);
        c.lens.copy_from_slice(&a[I::LENS..I::LENS+N::LEN]);
        if a[I::PUBLISHED]!=0. {
            for(at,value)in[(U::WORLD,&mut c.world),(U::INVERSE,&mut c.inverse),(U::PROJECTION,&mut c.projection),(U::PROJECTION_INVERSE,&mut c.projection_inverse)] {value.elements.copy_from_slice(&a[I::MATRICES+at..I::MATRICES+at+16]);}
        }else{c.compose(vector(a,I::SCALE));c.project();}
        self.publish(R::MAIN,c);true
    }
    // Graphics.render's far and first-person views share the main pose but
    // preserve their own lens range. Recomposition repeats Three's copy/update.
    pub fn project_main(&mut self){let mut c=self.cameras[R::MAIN];c.project();self.publish(R::MAIN,c);}
    pub fn derived(&mut self,role:usize) {
        if role!=R::FAR&&role!=R::VIEW {self.clipped=true;return;}
        let mut c=self.cameras[R::MAIN];let at=if role==R::FAR{I::FAR_RANGE}else{I::VIEW_RANGE};
        c.lens[N::NEAR]=self.input[at];c.lens[N::FAR]=self.input[at+1];
        // The old derived cameras do not inherit a main camera's view offset.
        c.up=V::new(0.,1.,0.);c.lens[N::ZOOM]=1.;c.lens[N::FILM_GAUGE]=35.;c.lens[N::FILM_OFFSET]=0.;c.lens[N::VIEW_ENABLED]=0.;
        c.compose(V::new(1.,1.,1.));c.project();self.publish(role,c);
    }
    pub fn copy(&mut self,from:usize,to:usize) {if from<R::COUNT&&to<R::COUNT{self.publish(to,self.cameras[from]);}else{self.clipped=true;}}
    // Upscaler.shift/restore used only two projection elements. The inverse
    // follows that exact shifted/restored matrix, never an algebraic shortcut.
    pub fn shift(&mut self,role:usize) {
        if role>=R::COUNT{self.clipped=true;return;}let mut c=self.cameras[role];
        if !self.shifted[role]{self.saved[role]=c.projection;self.shifted[role]=true;}
        c.projection.elements[8]-=self.input[I::SHIFT];c.projection.elements[9]-=self.input[I::SHIFT+1];
        c.projection_inverse=c.projection;c.projection_inverse.invert();self.publish(role,c);
    }
    pub fn restore(&mut self) {for role in 0..R::COUNT {if self.shifted[role]{let mut c=self.cameras[role];c.projection=self.saved[role];c.projection_inverse=c.projection;c.projection_inverse.invert();self.shifted[role]=false;self.publish(role,c);}}}
    // Water.reflect's reflected pose, camera-space plane and oblique clipping.
    // Eligibility/hold policy remains in FramePlan; this owner supplies f64 facts.
    pub fn reflect(&mut self) {
        let main=self.cameras[R::MAIN];let mut c=main;let level=self.input[I::WATER_LEVEL];
        let near=self.input[I::NEAR_REFLECTION]!=0.;c.lens[N::NEAR]=if near{main.lens[N::NEAR]}else{let n=self.input[I::FAR_RANGE];if n==0.{2.}else{n}};
        c.lens[N::FAR]=if self.input[I::FAR_RANGE+1]==0.{main.lens[N::FAR]}else{self.input[I::FAR_RANGE+1]};
        c.lens[N::ZOOM]=1.;c.lens[N::FILM_GAUGE]=35.;c.lens[N::FILM_OFFSET]=0.;c.lens[N::VIEW_ENABLED]=0.;c.project();
        c.p.y=2.*level-main.p.y;
        let e=&main.world.elements;let mut forward=V::new(e[8],e[9],e[10]);forward.normalize().negate();forward.y=-forward.y;
        c.up=V::new(0.,1.,0.);c.up.apply_quaternion(&main.q);c.up.y=-c.up.y;
        let mut at=c.p;at.add(&forward);c.look(at);self.publish(R::MIRROR_PLAIN,c);
        let mut plane=Plane::new(V::new(0.,1.,0.),-level);plane.apply_matrix4(&c.inverse,None);
        let mut clip=Vector4::new(plane.normal.x,plane.normal.y,plane.normal.z,plane.constant);
        let e=&c.projection.elements;let sign=|v:f64|if v==0.{v}else if v.is_nan(){v}else{v.signum()};
        // Math.sign keeps signed zero; a signum shortcut changes clip bits.
        let q=Vector4::new((sign(clip.x)+e[8])/e[0],(sign(clip.y)+e[9])/e[5],-1.,(1.+e[10])/e[14]);
        let denominator=clip.dot(&q);self.mirror[H::VALID]=0.;
        self.mirror[H::CLIP..H::CLIP+4].copy_from_slice(&[clip.x,clip.y,clip.z,clip.w]);
        self.mirror[H::PROJECTION..H::PROJECTION+16].copy_from_slice(&c.projection.elements);
        // Water.reflect had already published the plain camera at refusal.
        // Its preceding reflection matrix remains valid for a later held view.
        if denominator.abs()<1e-6||!denominator.is_finite(){self.publish(R::MIRROR,c);return;}
        clip.multiply_scalar(2./denominator);let e=&mut c.projection.elements;e[2]=clip.x;e[6]=clip.y;e[10]=clip.z+1.;e[14]=clip.w;
        // Water changed projection without updating its inverse. Preserve that
        // publication quirk; the plain inverse remains attached to this view.
        let mut reflection=bias();reflection.multiply(&c.projection).multiply(&c.inverse);
        self.mirror[H::VALID]=1.;self.mirror[H::CLIP..H::CLIP+4].copy_from_slice(&[clip.x,clip.y,clip.z,clip.w]);
        self.mirror[H::PROJECTION..H::PROJECTION+16].copy_from_slice(&c.projection.elements);
        self.mirror[H::REFLECTION..H::REFLECTION+16].copy_from_slice(&reflection.elements);self.publish(R::MIRROR,c);
    }
    // CubeCamera.updateCoordinateSystem: each child composes its fixed local
    // look before the cube parent's translation. Six views precede mipmaps.
    pub fn probe(&mut self,slot:usize) {
        if slot>=4 {self.clipped=true;return;}
        let eye=self.cameras[R::MAIN].p;let mut cube=Camera::default();cube.p=eye;cube.compose(V::new(1.,1.,1.));let parent=cube.world;self.publish(R::PROBE_PARENT0+slot,cube);
        // Cube children keep local position zero. Their world translation
        // comes from the parent multiplication, which shaders read directly.
        let directions=[V::new(1.,0.,0.),V::new(-1.,0.,0.),V::new(0.,1.,0.),V::new(0.,-1.,0.),V::new(0.,0.,1.),V::new(0.,0.,-1.)];
        let ups=[V::new(0.,1.,0.),V::new(0.,1.,0.),V::new(0.,0.,-1.),V::new(0.,0.,1.),V::new(0.,1.,0.),V::new(0.,1.,0.)];
        for face in 0..6 {let mut c=Camera::default();c.up=ups[face];c.lens[N::FOV]=-90.;c.lens[N::ASPECT]=1.;c.lens[N::NEAR]=0.05;c.lens[N::FAR]=50.;c.lens[N::ZOOM]=1.;c.lens[N::FILM_GAUGE]=35.;c.project();c.look(directions[face]);let local=c.world;c.world.multiply_matrices(&parent,&local);c.inverse=c.world;c.inverse.invert();self.publish(R::PROBE0+slot*6+face,c);}
    }
    // Graphics.render and SkyProbe's sky-follow use the same authored scale.
    // Mirrors intentionally stand every layer at the reflected eye instead.
    pub fn sky(&mut self,role:usize,follow:bool) {
        if role>=R::COUNT{self.clipped=true;return;}let eye=self.cameras[role].p;
        for i in 0..(self.input[I::SKY_COUNT] as usize).min(L::SKY){let at=I::SKY_FOLLOW+i*Y::LEN;let mut p=eye;if follow&&self.input[at+Y::FOLLOW]!=0.{p.y=self.input[at+Y::SCALE]*eye.y+(1.-self.input[at+Y::SCALE])*self.input[at+Y::OFFSET];}put(&mut self.sky,i*3,p);}
    }
    // packPointLights's arithmetic and view-light proxies. Discovery, layers
    // and parent visibility arrive as graph facts, preserving source order.
    pub fn lights(&mut self,pack:usize) {
        if pack>=L::PACKS{self.clipped=true;return;}let eye=vector(&self.input,I::LIGHT_EYE);let far=self.input[I::LIGHT_FAR];let layers=to_uint32(self.input[I::LIGHT_LAYERS]);
        let mut parent=Matrix4::default();parent.elements.copy_from_slice(&self.input[I::VIEW_WORLD..I::VIEW_WORLD+16]);
        // Layer masks use JS ToUint32, including signed enableAll masks.
        // Pack order remains graph order, with a bounded first-16 prefix.
        let out=&mut self.packs[pack*K::LEN..(pack+1)*K::LEN];let mut count=0;
        for row in self.light_input.chunks_exact(P::LEN).take((self.input[I::LIGHT_COUNT] as usize).min(L::RAW_LIGHTS)) {
            let proxy=row[P::PROXY]!=0.;let shown=if proxy{row[P::INITIAL_SHOWN]!=0.}else{row[P::SHOWN]!=0.};
            let mask=to_uint32(if proxy{row[P::INITIAL_LAYERS]}else{row[P::LAYERS]});
            if !shown||mask&layers==0 {continue;}
            let intensity=if proxy&&row[P::SHOWN]==0.{0.}else{row[P::INTENSITY]};
            let r=row[P::COLOR]*intensity;let g=row[P::COLOR+1]*intensity;let b=row[P::COLOR+2]*intensity;
            if !(r>0.||g>0.||b>0.){continue;}
            let mut position=vector(row,P::POSITION);if proxy{position.apply_matrix4(&parent);}let range=row[P::RANGE];
            if range>0.&&position.distance_to(&eye)>far+range{continue;}
            let at=K::VALUES+count*8;out[at..at+8].copy_from_slice(&[position.x,position.y,position.z,range,r,g,b,0.]);count+=1;if count==16{break;}
        }
        out[K::COUNT]=count as f64;
    }
    pub fn cascades(&mut self) {
        let mut camera=self.cameras[R::MAIN];camera.up=V::new(0.,1.,0.);camera.lens[N::FOV]=(js_max(75.,camera.lens[N::FOV])/5.).ceil()*5.;camera.lens[N::ZOOM]=1.;camera.lens[N::FILM_GAUGE]=35.;camera.lens[N::FILM_OFFSET]=0.;camera.lens[N::VIEW_ENABLED]=0.;camera.project();camera.compose(V::new(1.,1.,1.));self.publish(R::CASCADE,camera);
        self.cascades.prepare(&self.input,camera);
        for i in 0..self.cascades.count {self.publish(R::SHADOW0+i,self.cascades.cameras[i]);}
    }
    pub fn invalidate(&mut self){self.main_valid=false;self.cascades.invalidate();self.shifted.fill(false);}
}
