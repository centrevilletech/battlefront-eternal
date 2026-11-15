//! Optional native battle measurements. Query results are read only after availability.
use glow::HasContext;
use std::{collections::{BTreeMap, VecDeque}};

pub struct CpuSample{wall:std::time::Instant,thread:Option<f64>}
impl CpuSample{
    pub fn now()->Self{Self{wall:std::time::Instant::now(),thread:thread_cpu_ms()}}
    pub fn elapsed_ms(&self)->f64{self.thread.zip(thread_cpu_ms()).map_or_else(||self.wall.elapsed().as_secs_f64()*1000.,|(start,end)|(end-start).max(0.))}
}
#[cfg(all(target_os="linux",target_pointer_width="64"))]
fn thread_cpu_ms()->Option<f64>{
    #[repr(C)]struct Time{seconds:std::ffi::c_long,nanoseconds:std::ffi::c_long}
    unsafe extern "C"{fn clock_gettime(clock:i32,value:*mut Time)->i32;}
    let mut t=Time{seconds:0,nanoseconds:0};
    if unsafe{clock_gettime(3,&mut t)}==0{Some(t.seconds as f64*1000.+t.nanoseconds as f64/1e6)}else{None}
}
#[cfg(not(all(target_os="linux",target_pointer_width="64")))]
fn thread_cpu_ms()->Option<f64>{None}

pub struct Token<Q> { start: CpuSample, query: Option<Q>, draws: u64, triangles: u64 }
#[derive(Default)]
struct Samples { cpu: Vec<f64>, gpu: Vec<f64>, draws: u64, triangles: u64, calls: u64, cpu_frame:u64, cpu_sum:f64, cpu_pending:bool, gpu_frame:u64, gpu_sum:f64, gpu_pending:bool }
impl Samples {
    fn cpu(&mut self, frame:u64, ms:f64) {
        if self.cpu_pending && self.cpu_frame!=frame {if self.cpu.len()<16384 {self.cpu.push(self.cpu_sum);}self.cpu_sum=0.;}
        self.cpu_frame=frame;self.cpu_sum+=ms;self.cpu_pending=true;
    }
    fn gpu(&mut self, frame:u64, ms:f64) {
        if self.gpu_pending && self.gpu_frame!=frame {if self.gpu.len()<16384 {self.gpu.push(self.gpu_sum);}self.gpu_sum=0.;}
        self.gpu_frame=frame;self.gpu_sum+=ms;self.gpu_pending=true;
    }
}
pub struct Profile<C: HasContext> {
    pub enabled: bool,
    pub gpu_supported: bool,
    frame: u64,
    pending: VecDeque<(&'static str, C::Query, u64)>,
    free: Vec<C::Query>,
    samples: BTreeMap<&'static str, Samples>,
    pub draws: u64,
    pub triangles: u64,
}
impl<C: HasContext> Default for Profile<C> {
    fn default() -> Self { Self { enabled:false, gpu_supported:false, frame:0, pending:VecDeque::new(), free:vec![], samples:BTreeMap::new(), draws:0, triangles:0 } }
}
impl<C: HasContext> Profile<C> {
    pub fn enable(&mut self, gl: &C) {
        self.enabled = true;
        self.gpu_supported = !gl.version().is_embedded || gl.supported_extensions().iter().any(|e| e.contains("disjoint_timer_query"));
        eprintln!("graphics trace GPU={} CPU={} renderer={}", self.gpu_supported, if thread_cpu_ms().is_some(){"thread"}else{"wall"}, unsafe { gl.get_parameter_string(glow::RENDERER) });
    }
    pub fn clock(&self) -> Option<CpuSample> { (self.enabled && self.frame % 8 == 0).then(CpuSample::now) }
    pub fn cpu(&mut self, label: &'static str, start: Option<CpuSample>) {
        if let Some(start) = start { self.samples.entry(label).or_default().cpu(self.frame,start.elapsed_ms()); }
    }
    pub fn begin(&mut self, gl: &C) -> Option<Token<C::Query>> {
        let start = self.clock()?;
        let query = if self.gpu_supported && self.pending.len()<512 {
            self.free.pop().or_else(|| unsafe {gl.create_query().ok()})
        } else {None};
        if let Some(q)=query { unsafe {gl.begin_query(glow::TIME_ELAPSED,q);} }
        Some(Token {start,query,draws:self.draws,triangles:self.triangles})
    }
    pub fn end(&mut self, gl: &C, label: &'static str, token: Option<Token<C::Query>>) {
        if let Some(t)=token {
            if let Some(q)=t.query {unsafe {gl.end_query(glow::TIME_ELAPSED);} self.pending.push_back((label,q,self.frame));}
            let s=self.samples.entry(label).or_default();
            s.cpu(self.frame,t.start.elapsed_ms());
            s.draws+=self.draws-t.draws;s.triangles+=self.triangles-t.triangles;s.calls+=1;
        }
    }
    pub fn draw(&mut self, indices: u64, instances: u64, calls: u64) {
        if self.enabled {self.draws+=calls;self.triangles+=indices/3*instances;}
    }
    pub fn frame(&mut self, gl: &C) {
        if !self.enabled {return;}
        self.frame+=1;
        if self.gpu_supported && gl.version().is_embedded && unsafe {gl.get_parameter_i32(0x8FBB)}!=0 {
            self.free.extend(self.pending.drain(..).map(|(_,q,_)|q));return;
        }
        // Never wait for the GPU; keep unfinished queries for a later frame.
        while let Some(&(label,q,frame))=self.pending.front() {
            if unsafe {gl.get_query_parameter_u32(q,glow::QUERY_RESULT_AVAILABLE)}==0 {break;}
            let ns=unsafe {gl.get_query_parameter_u64(q,glow::QUERY_RESULT)};
            self.pending.pop_front();self.free.push(q);
            self.samples.entry(label).or_default().gpu(frame,ns as f64/1e6);
        }
    }
    pub fn report(&mut self, gl: &C) {
        if !self.enabled {return;}
        self.frame(gl);
        eprintln!("graphics table: section | samples | CPU p50/p95 ms | GPU p50/p95 ms | draws/frame | triangles/frame");
        for (label,s) in &mut self.samples {
            fn pct(v:&mut [f64])->(f64,f64) {v.sort_by(f64::total_cmp);if v.is_empty(){return (0.,0.);} (v[v.len()/2],v[((v.len() as f64*0.95).ceil() as usize).saturating_sub(1)])}
            if s.cpu_pending {s.cpu.push(s.cpu_sum);s.cpu_pending=false;}
            if s.gpu_pending {s.gpu.push(s.gpu_sum);s.gpu_pending=false;}
            let n=s.cpu.len();let cpu=pct(&mut s.cpu);let gpu=pct(&mut s.gpu);
            eprintln!("graphics row: {label} | {n} | {:.6}/{:.6} | {:.6}/{:.6} | {:.2} | {:.2}",cpu.0,cpu.1,gpu.0,gpu.1,s.draws as f64/n.max(1) as f64,s.triangles as f64/n.max(1) as f64);
        }
        eprintln!("graphics totals: frames={} draws={} triangles={} GPU-pending={}",self.frame,self.draws,self.triangles,self.pending.len());
        for q in self.free.drain(..).chain(self.pending.drain(..).map(|(_,q,_)|q)) {unsafe {gl.delete_query(q);}}
        self.enabled=false;
    }
}

#[cfg(test)]
mod tests {
    use super::Samples;
    #[test]
    fn material_runs_sum_into_one_pass_sample_per_frame() {
        let mut s=Samples::default();
        s.cpu(0,0.1);s.cpu(0,0.2);s.cpu(8,0.5);
        s.gpu(0,0.3);s.gpu(0,0.4);s.gpu(8,0.9);
        assert_eq!(s.cpu.len(),1);assert!((s.cpu[0]-0.3).abs()<1e-12);
        assert_eq!(s.gpu.len(),1);assert!((s.gpu[0]-0.7).abs()<1e-12);
    }
}
