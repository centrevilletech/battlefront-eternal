//! Optional native battle measurements. Query results are read only after availability.
use glow::HasContext;
use std::{collections::{BTreeMap, VecDeque}};

impl<C: HasContext, M: crate::core::MultiDraw> crate::core::Core<C, M> {
    pub fn start_profile(&mut self) {
        self.profile.enable(&self.gl);
        eprintln!("graphics water: hardware-depth-clamp programs={}", self.programs.iter().filter(|p| p.depth_clamp).count());
    }
}

/// Whether this context's timestamp queries tell the time: the driver must offer them (desktop OpenGL, or
/// EXT_disjoint_timer_query on OpenGL ES), and one read back after a finish must hold a time. macOS's OpenGL
/// lists the timer extension but counts nothing, and reading its queries back each frame makes the driver
/// wait for the GPU (frames of 75 ms where 25 ms were drawn). Asked once per context: it finishes the queue.
pub fn gpu_timestamps<C: HasContext>(gl: &C) -> bool {
    if gl.version().is_embedded && !gl.supported_extensions().iter().any(|e| e.contains("disjoint_timer_query")) { return false; }
    unsafe {
        let Ok(query) = gl.create_query() else { return false; };
        gl.query_counter(query, glow::TIMESTAMP);
        gl.finish();
        let time = gl.get_query_parameter_u64(query, glow::QUERY_RESULT);
        gl.delete_query(query);
        time != 0
    }
}

pub struct CpuSample{wall:std::time::Instant,thread:Option<f64>}
impl CpuSample{
    pub fn now()->Self{Self{wall:std::time::Instant::now(),thread:thread_cpu_ms()}}
    pub fn elapsed_ms(&self)->f64{self.thread.zip(thread_cpu_ms()).map_or_else(||self.wall.elapsed().as_secs_f64()*1000.,|(start,end)|(end-start).max(0.))}
}
// The thread's own CPU time. macOS needs it more than any: its OpenGL waits for the GPU inside draw calls,
// so wall time around a frame's drawing reads a GPU-bound frame as a CPU-bound one.
#[cfg(all(any(target_os="linux",target_os="macos"),target_pointer_width="64"))]
fn thread_cpu_ms()->Option<f64>{
    #[repr(C)]struct Time{seconds:std::ffi::c_long,nanoseconds:std::ffi::c_long}
    unsafe extern "C"{fn clock_gettime(clock:i32,value:*mut Time)->i32;}
    // CLOCK_THREAD_CPUTIME_ID
    const THREAD:i32=if cfg!(target_os="macos"){16}else{3};
    let mut t=Time{seconds:0,nanoseconds:0};
    if unsafe{clock_gettime(THREAD,&mut t)}==0{Some(t.seconds as f64*1000.+t.nanoseconds as f64/1e6)}else{None}
}
#[cfg(not(all(any(target_os="linux",target_os="macos"),target_pointer_width="64")))]
fn thread_cpu_ms()->Option<f64>{None}

pub struct Token<Q> { start: CpuSample, query: Option<(Q, Q)>, draws: u64, triangles: u64 }
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
    gpu_scale: f64,
    frame: u64,
    pending: VecDeque<(&'static str, C::Query, C::Query, u64)>,
    free: Vec<C::Query>,
    samples: BTreeMap<&'static str, Samples>,
    pub draws: u64,
    pub triangles: u64,
}
impl<C: HasContext> Default for Profile<C> {
    fn default() -> Self { Self { enabled:false, gpu_supported:false, gpu_scale:1., frame:0, pending:VecDeque::new(), free:vec![], samples:BTreeMap::new(), draws:0, triangles:0 } }
}
impl<C: HasContext> Profile<C> {
    pub fn enable(&mut self, gl: &C) {
        self.enabled = true;
        self.gpu_supported = gpu_timestamps(gl);
        eprintln!("graphics trace GPU={} CPU={} renderer={} API={}", self.gpu_supported, if thread_cpu_ms().is_some(){"thread"}else{"wall"}, unsafe { gl.get_parameter_string(glow::RENDERER) }, unsafe { gl.get_parameter_string(glow::VERSION) });
        // Some drivers expose a timestamp period that differs from wall time.
        // Calibrate once when tracing starts, before collecting battle samples.
        // Ordinary play never pays this synchronisation or diagnostic delay.
        #[cfg(not(target_arch="wasm32"))]
        if self.gpu_supported { self.gpu_scale = calibrate_gpu_clock(gl); }
    }
    pub fn clock(&self) -> Option<CpuSample> { (self.enabled && self.frame % 7 == 0).then(CpuSample::now) }
    pub fn cpu(&mut self, label: &'static str, start: Option<CpuSample>) {
        if let Some(start) = start { self.samples.entry(label).or_default().cpu(self.frame,start.elapsed_ms()); }
    }
    pub fn begin(&mut self, gl: &C) -> Option<Token<C::Query>> {
        let start = self.clock()?;
        let query = if self.gpu_supported && self.pending.len() < 512 {
            let a = self.free.pop().or_else(|| unsafe { gl.create_query().ok() });
            let b = self.free.pop().or_else(|| unsafe { gl.create_query().ok() });
            match (a, b) {
                (Some(a), Some(b)) => Some((a, b)),
                (a, b) => { self.free.extend(a); self.free.extend(b); None }
            }
        } else { None };
        if let Some((a, _)) = query { unsafe { gl.query_counter(a, glow::TIMESTAMP); } }
        Some(Token { start, query, draws: self.draws, triangles: self.triangles })
    }
    // Timestamps bracket completed commands. Unlike elapsed queries, a
    // whole-frame pair can contain pass pairs on the same sampled frame.
    pub fn begin_frame(&mut self, gl: &C) -> Option<Token<C::Query>> { self.begin(gl) }
    pub fn end(&mut self, gl: &C, label: &'static str, token: Option<Token<C::Query>>) {
        if let Some(t) = token {
            if let Some((a, b)) = t.query {
                unsafe { gl.query_counter(b, glow::TIMESTAMP); }
                self.pending.push_back((label, a, b, self.frame));
            }
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
            self.free.extend(self.pending.drain(..).flat_map(|(_,a,b,_)|[a,b]));return;
        }
        // Never wait for the GPU; keep unfinished queries for a later frame.
        while let Some(&(label,a,b,frame))=self.pending.front() {
            if unsafe {gl.get_query_parameter_u32(b,glow::QUERY_RESULT_AVAILABLE)}==0 {break;}
            let ns=unsafe {gl.get_query_parameter_u64(b,glow::QUERY_RESULT).saturating_sub(gl.get_query_parameter_u64(a,glow::QUERY_RESULT))};
            self.pending.pop_front();self.free.extend([a,b]);
            self.samples.entry(label).or_default().gpu(frame,ns as f64*self.gpu_scale/1e6);
        }
    }
    pub fn report(&mut self, gl: &C) {
        if !self.enabled {return;}
        self.frame(gl);
        eprintln!("graphics table: section | samples | CPU p50/p95 ms | GPU p50/p95 ms | draws/active frame | triangles/active frame");
        for (label,s) in &mut self.samples {
            fn pct(v:&mut [f64])->(f64,f64) {v.sort_by(f64::total_cmp);if v.is_empty(){return (0.,0.);} (v[v.len()/2],v[((v.len() as f64*0.95).ceil() as usize).saturating_sub(1)])}
            if s.cpu_pending {s.cpu.push(s.cpu_sum);s.cpu_pending=false;}
            if s.gpu_pending {s.gpu.push(s.gpu_sum);s.gpu_pending=false;}
            let n=s.cpu.len();let cpu=pct(&mut s.cpu);let gpu=pct(&mut s.gpu);
            eprintln!("graphics row: {label} | {n} | {:.6}/{:.6} | {:.6}/{:.6} | {:.2} | {:.2}",cpu.0,cpu.1,gpu.0,gpu.1,s.draws as f64/n.max(1) as f64,s.triangles as f64/n.max(1) as f64);
        }
        eprintln!("graphics totals: frames={} draws={} triangles={} GPU-pending={}",self.frame,self.draws,self.triangles,self.pending.len());
        for q in self.free.drain(..).chain(self.pending.drain(..).flat_map(|(_,a,b,_)|[a,b])) {unsafe {gl.delete_query(q);}}
        self.enabled=false;
    }
}

#[cfg(not(target_arch="wasm32"))]
fn calibrate_gpu_clock<C: HasContext>(gl: &C) -> f64 {
    unsafe {
        let Ok(start) = gl.create_query() else { return 1.; };
        let Ok(end) = gl.create_query() else { gl.delete_query(start); return 1.; };
        gl.query_counter(start, glow::TIMESTAMP);
        gl.finish();
        let wall = std::time::Instant::now();
        std::thread::sleep(std::time::Duration::from_millis(100));
        gl.query_counter(end, glow::TIMESTAMP);
        gl.finish();
        let wall_ms = wall.elapsed().as_secs_f64()*1000.;
        let gpu_ms = gl.get_query_parameter_u64(end, glow::QUERY_RESULT)
            .saturating_sub(gl.get_query_parameter_u64(start, glow::QUERY_RESULT)) as f64/1e6;
        let disjoint = gl.version().is_embedded && gl.get_parameter_i32(0x8FBB) != 0;
        gl.delete_query(start);gl.delete_query(end);
        let scale = wall_ms/gpu_ms;
        if disjoint || !scale.is_finite() || !(0.1..10.).contains(&scale) {
            eprintln!("graphics GPU clock: calibration unavailable (wall={wall_ms:.3} ms raw={gpu_ms:.3} ms disjoint={disjoint})");
            return 1.;
        }
        eprintln!("graphics GPU clock: wall={wall_ms:.3} ms raw={gpu_ms:.3} ms scale={scale:.6}");
        scale
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
