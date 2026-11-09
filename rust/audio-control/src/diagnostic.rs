//! AudioPanel's speaker and around-room checks share native render slots with
//! the battle. A test can never overwrite a live source's physical slot.
use crate::{engine::{Engine,NONE},streams::Streams,layout::{event as e,event_kind},render};
#[derive(Clone,Copy)]
struct Lane {slot:usize,ends:f64,around:bool,start:f64}
impl Default for Lane {fn default()->Self{Self{slot:NONE,ends:0.0,around:false,start:0.0}}}
#[derive(Default)]
pub struct Diagnostic {lanes:[Lane;16]}
impl Diagnostic {
    /// ports AudioPanel.test/around's scheduling. The UI supplies requested
    /// device channel and generated PCM; the native owner reserves and frees
    /// slots in the same pool used for ordinary battle rendering.
    pub fn start(&mut self, engine:&mut Engine,streams:&mut Streams,row:&[f64]) {
        let lane=self.lanes.iter_mut().find(|lane|lane.slot==NONE).expect("speaker test capacity exhausted");
        let around=row[e::KIND] as usize==event_kind::TEST_AROUND; let at=row[e::AMOUNT]; let duration=row[e::SECONDS];
        let slot=if around{engine.reserve_slot()}else{streams.reserve_slot()};
        *lane=Lane{slot,ends:engine.wall_now+(at-engine.now)+duration+0.1,around,start:engine.wall_now}; let out=&mut engine.commands;
        if around {
            out.extend_from_slice(&[render::BIND as f64,slot as f64,1.0]);
            out.extend_from_slice(&[render::START as f64,slot as f64,row[e::SOUND],0.0,1.0,1.0,engine.sample_rate.min(40000.0)/2.0,at]);
            out.extend_from_slice(&[render::GAIN_SET as f64,slot as f64,row[e::HEALTH],at]);
        }else{
            out.extend_from_slice(&[render::STREAM_BIND as f64,slot as f64,1.0,3.0+row[e::VALUE],0.0]);
            out.extend_from_slice(&[render::STREAM_GAIN as f64,slot as f64,row[e::HEALTH],0.0,at,1.0]);
            out.extend_from_slice(&[render::STREAM_START as f64,slot as f64,0.0,row[e::SOUND],at,0.0,1.0,0.0]);
        }
    }
    /// ports AudioPanel.around's angular sweep. It uses device time so the
    /// wiring check continues while the pause menu stops battle clocks.
    pub fn update(&mut self,engine:&mut Engine,streams:&mut Streams){
        for lane in &mut self.lanes {
            if lane.slot==NONE{continue;}
            let slot=lane.slot as f64;
            if engine.wall_now>=lane.ends {
                if lane.around{engine.commands.extend_from_slice(&[render::STOP as f64,slot]);engine.return_slot(lane.slot);}
                else{engine.commands.extend_from_slice(&[render::STREAM_RELEASE as f64,slot]);streams.return_slot(lane.slot);}
                lane.slot=NONE;continue;
            }
            if !lane.around{continue;}
            let fraction=((engine.wall_now-lane.start)/6.0).clamp(0.0,1.0);let angle=fraction*std::f64::consts::TAU;
            let x=-v8math::sin(angle);let z=-v8math::cos(angle);
            if engine.light{engine.commands.extend_from_slice(&[render::PLACE as f64,slot,x,0.0,z,0.3,engine.now]);}
            else if engine.hrtf{for (axis,value) in [x*3.0,0.0,z*3.0].into_iter().enumerate(){engine.commands.extend_from_slice(&[render::POSITION as f64,slot,axis as f64,value,0.01,engine.now,0.0]);}}
            else{engine.placements.extend_from_slice(&[slot,x,0.0,z,0.0,0.0]);}
        }
    }
}
