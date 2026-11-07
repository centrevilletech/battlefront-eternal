//! StormSky.strike and the pulse state from StormSky.update. The cloud render
//! target, light shells and draw scheduling stay with the retained renderer.
use crate::{surface_math::Math, layout::surface as lim, random::Random};
pub struct Storm {
    random: Random,
    next: f64,
    start: f64,
    pub strikes: u32,
    pub position: [f64;3],
    pub flash: f64,
    pub count: usize,
    pub dirty: bool,
    pub positions: Vec<f32>,
    pub tangents: Vec<f32>,
    pub edges: Vec<f32>,
    pub widths: Vec<f32>,
    pub energies: Vec<f32>,
}
impl Default for Storm { fn default() -> Self { Self::new() } }
impl Storm {
    pub fn new() -> Self {
        // StormSky's cold 64-cubed noise texture consumed this stream before
        // its first strike. Retain that sequence independently of resource IO.
        let mut random=Random::new(8217);for _ in 0..64*64*64 { random.next(); }
        Self {random,next:6.0,start:-100.0,strikes:0,position:[0.0;3],flash:0.0,count:0,dirty:false,
            positions:vec![0.0;lim::STORM_VERTICES*3],tangents:vec![0.0;lim::STORM_VERTICES*3],edges:vec![0.0;lim::STORM_VERTICES],
            widths:vec![0.0;lim::STORM_VERTICES],energies:vec![0.0;lim::STORM_VERTICES]}
    }
    // StormSky.update restores lighting in the resource host before applying
    // this flash. The three overlapping pulses retain their original spans,
    // inclusive endpoints and multiply/divide order from the page expression.
    pub fn update<M: Math>(&mut self,time:f64,eye:[f64;3],enabled:bool) {
        self.dirty=false;if !enabled {return;}
        if time>=self.next {self.strike::<M>(time,eye);}
        let age=time-self.start;
        let pulse=|start:f64,span:f64,power:f64| if age<start||age>start+span {0.0} else {power*M::sin(std::f64::consts::PI*(age-start)/span)};
        self.flash=pulse(0.0,0.12,1.0)+pulse(0.19,0.08,0.6)+pulse(0.34,0.19,0.8);
    }
    // StormSky.strike schedules from the current displayed ocean time. It
    // deliberately does not catch up missed strikes after a long pause.
    // Each new bolt is distributed around the current eye, not the reticle.
    fn strike<M: Math>(&mut self,time:f64,eye:[f64;3]) {
        self.start=time;self.next=time+7.0+self.random.next()*11.0;self.strikes+=1;
        let angle=self.random.next()*std::f64::consts::PI*2.0;let distance=430.0+self.random.next()*220.0;
        let (x,z)=(eye[0]+M::cos(angle)*distance,eye[2]+M::sin(angle)*distance);
        let top=[x,300.0+self.random.next()*100.0,z];self.position=top;self.count=0;
        // Positions and tangents round only when written to f32 attributes.
        // Branch traversal retains f64 values until its next segment is made.
        // This preserves the same trunk even when its preceding branch bends.
        let mut a=top;
        // StormSky.strike interleaves each branch's random draws before the
        // next trunk point. Reordering branches changes every later vertex.
        for i in 1..=28 {
            let b=[a[0]+(self.random.next()-0.5)*22.0,top[1]+(-8.0-top[1])*i as f64/28.0,a[2]+(self.random.next()-0.5)*22.0];
            self.segment(a,b,0.7,1.0);
            if i>3&&i<22&&i%5==0 {
                let mut branch=a;let dx=(self.random.next()-0.5)*30.0;let dz=(self.random.next()-0.5)*30.0;
                for j in 0..6 {
                    let tip=[branch[0]+dx+(self.random.next()-0.5)*10.0,branch[1]-10.0-self.random.next()*12.0,branch[2]+dz];
                    self.segment(branch,tip,0.45,(1.0-j as f64/7.0)*0.65);branch=tip;
                }
            }
            a=b;
        }
        self.dirty=true;
    }
    fn segment(&mut self,a:[f64;3],b:[f64;3],width:f64,energy:f64) {
        // Two layers of six vertices, with the authored triangle winding.
        // The 1,536-vertex allocation exceeds the 624 vertices per strike.
        for layer in [1.0,4.0] {for (end,edge) in [(false,-1.0),(true,-1.0),(false,1.0),(false,1.0),(true,-1.0),(true,1.0)] {
            if self.count>=lim::STORM_VERTICES {return;}
            let i=self.count;self.count+=1;let p=if end {b} else {a};
            for axis in 0..3 {self.positions[i*3+axis]=p[axis] as f32;self.tangents[i*3+axis]=(b[axis]-a[axis]) as f32;}
            self.edges[i]=edge;self.widths[i]=(width*layer) as f32;self.energies[i]=(energy*if layer==1.0 {1.0} else {0.16}) as f32;
        }}
    }
}
