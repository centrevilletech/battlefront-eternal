//! PlayoutClock retains its arrival ring and sorting scratch. The existing
//! Rust arithmetic owns percentile delay and monotonic clock advancement;
//! native frames allocate neither a sample slice nor a sorted copy.
use bfe_sim::prediction::playout::{self,ArrivalInput,AdvanceInput,AdvanceMode};
pub struct Clock{gaps:[f64;120],scratch:[f64;120],at:f64,count:f64,arrived:Option<f64>,arrived_tick:f64,latest:f64,pub shown:f64,started:bool,waits:f64,min_delay:f64}
impl Default for Clock{fn default()->Self{Self{gaps:[0.;120],scratch:[0.;120],at:0.,count:0.,arrived:None,arrived_tick:0.,latest:0.,shown:0.,started:false,waits:0.,min_delay:2.}}}
impl Clock{
    /// A server's stream: remote bodies are drawn one 20 Hz publication (3
    /// ticks) plus a margin behind the newest packet, as game.js builds its
    /// PlayoutClock for a network battle; measured arrival gaps widen it. The
    /// world thread of Instant Action keeps the 2 tick minimum.
    pub fn online()->Self{Self{min_delay:4.,..Self::default()}}
    pub fn reset(&mut self){*self=Self{min_delay:self.min_delay,..Self::default()};}
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
        let delay=playout::measure_delay(gap,1.,self.min_delay,10.)?;
        let r=playout::advance(AdvanceInput{latest:self.latest,delay,clock:self.shown,started:self.started,hz:60.,trim:0.1,snap:12.,dt,waits:self.waits})?;
        self.shown=r.clock;self.started|=r.mode!=AdvanceMode::Idle;self.waits=r.waits;Ok(self.shown)
    }
}
#[cfg(test)]mod tests{
    use super::*;
    // 20 Hz snapshots arriving evenly: the server's stream stays at least two
    // ticks behind the newest one (a late snapshot does not stall the bodies),
    // also after a new battle reset the clock.
    #[test]fn a_server_stream_is_held_a_publication_and_a_margin_behind(){
        let mut c=Clock::online();
        for round in 0..2{
            for frame in 3..1200{
                if frame%3==0{c.arrive(frame as f64,frame as f64/60.,3.).unwrap();}
                let behind=(frame/3*3)as f64-c.advance(1./60.).unwrap();
                if frame>600{assert!((2.0..=6.0).contains(&behind),"round {round} frame {frame}: {behind} ticks behind");}
            }
            c.reset();
        }
    }
    #[test]fn arrival_loss_and_long_pause_keep_a_bounded_monotonic_clock(){let mut c=Clock::default();let mut previous=0.;for tick in (2..2400).step_by(2){if tick%10!=0{c.arrive(tick as f64,tick as f64/60.,2.).unwrap();}let shown=c.advance(1./30.).unwrap();assert!(shown>=previous);assert!(shown<=tick as f64);previous=shown;}}
}

/// The displayed sample, bounded exactly as the client's renderedViewTick.
pub fn rendered_tick(clock: f64, latest: f64) -> f64 {
    if clock.is_finite() && latest.is_finite() { clock.min(latest.floor()).max(0.) } else { 0. }
}
#[cfg(test)]
mod view_tests {
    use super::rendered_tick;
    #[test]
    fn fire_uses_the_fractional_display_sample_instead_of_a_fixed_delay() {
        assert_eq!(rendered_tick(97.25, 100.), 97.25);
        assert_eq!(rendered_tick(110., 100.75), 100.);
        assert_eq!(rendered_tick(-1., 100.), 0.);
        assert_eq!(rendered_tick(f64::NAN, 100.), 0.);
    }
}
