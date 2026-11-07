//! PlayoutClock retains its arrival ring and sorting scratch. The existing
//! Rust arithmetic owns percentile delay and monotonic clock advancement;
//! native frames allocate neither a sample slice nor a sorted copy.
use bfe_sim::prediction::playout::{self,ArrivalInput,AdvanceInput,AdvanceMode};
pub struct Clock{gaps:[f64;120],scratch:[f64;120],at:f64,count:f64,arrived:Option<f64>,arrived_tick:f64,latest:f64,pub shown:f64,started:bool,waits:f64}
impl Default for Clock{fn default()->Self{Self{gaps:[0.;120],scratch:[0.;120],at:0.,count:0.,arrived:None,arrived_tick:0.,latest:0.,shown:0.,started:false,waits:0.}}}
impl Clock{
    pub fn reset(&mut self){*self=Self::default();}
    // PlayoutClock.arrive records packet arrival time before a frame samples.
    // A missing interval changes measured delay, not the authority tick.
    pub fn arrive(&mut self,tick:f64,now:f64,interval:f64)->Result<(),&'static str>{
        let r=playout::arrive(ArrivalInput{hz:60.,window:120.,latest:self.latest,arrived_at:self.arrived,arrived_tick:self.arrived_tick,gap_at:self.at,gap_count:self.count,ring_length:120,tick,now,interval})?;
        self.latest=r.latest;self.arrived=Some(r.arrived_at);self.arrived_tick=r.arrived_tick;self.at=r.gap_at;self.count=r.gap_count;
        if let Some(write)=r.ring_write{self.gaps[write.index]=write.value;}Ok(())
    }
    // Sort a retained scratch prefix so measuring delay does not reorder the
    // arrival ring or allocate a temporary TypedArray-equivalent collection.
    pub fn advance(&mut self,dt:f64)->Result<f64,&'static str>{
        let count=self.count as usize;self.scratch[..count].copy_from_slice(&self.gaps[..count]);
        let gap=playout::measure_gap_in_place(&mut self.scratch[..count])?;
        let delay=playout::measure_delay(gap,1.,2.,10.)?;
        let r=playout::advance(AdvanceInput{latest:self.latest,delay,clock:self.shown,started:self.started,hz:60.,trim:0.1,snap:12.,dt,waits:self.waits})?;
        self.shown=r.clock;self.started|=r.mode!=AdvanceMode::Idle;self.waits=r.waits;Ok(self.shown)
    }
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn arrival_loss_and_long_pause_keep_a_bounded_monotonic_clock(){let mut c=Clock::default();let mut previous=0.;for tick in (2..2400).step_by(2){if tick%10!=0{c.arrive(tick as f64,tick as f64/60.,2.).unwrap();}let shown=c.advance(1./30.).unwrap();assert!(shown>=previous);assert!(shown<=tick as f64);previous=shown;}}
}
