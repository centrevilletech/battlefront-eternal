//! SoundControl::drain and AudioOutput::controls without a page or worklet.
//! Admission runs the Rust control owner from recorded facts. Only its newly
//! computed commands enter the DSP schedule; reference banks are checks only.
use audio::{output::Output, FRAMES};
use audio_control::{runtime::Runtime, layout::*, Collision, Hit, Point};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};

// Synthetic maps use the same imported planes as sim::audio_control::Map.
// A live World supplies Collision at the control boundary when it is admitted.
struct Planes(Vec<[f64; plane::STRIDE]>);
impl Collision for Planes {
    fn cast(&mut self, from: Point, direction: Point, reach: f64) -> Option<Hit> {
        let mut distance = f64::INFINITY;
        for p in &self.0 {
            let d = p[plane::NX]*direction[0]+p[plane::NY]*direction[1]+p[plane::NZ]*direction[2];
            if d > 0.0 { let origin=p[plane::NX]*from[0]+p[plane::NY]*from[1]+p[plane::NZ]*from[2]; distance=distance.min((p[plane::DISTANCE]-origin)/d); }
        }
        (distance >= 0.0 && distance <= reach).then_some(Hit { distance, point:std::array::from_fn(|i|from[i]+direction[i]*distance), terrain:false })
    }
}
struct Batch { commands: Vec<f64>, placements: Vec<f64>, room: [f64; audio::reverb::PARAM_COUNT], mask: u32 }
impl Batch {
    fn control(r: &Runtime) -> Self {
        Self { commands:r.phase.engine.commands.clone(), placements:r.phase.engine.placements.clone(),
            room:std::array::from_fn(|i|r.info[info::ROOM+i]), mask:r.info[info::ROOM_CHANGED] as u32 }
    }
    fn apply(&self, output: &mut Output) {
        if self.mask != 0 { if let Some(master)=&mut output.master { master.room(&self.room,self.mask,false); } }
        output.commands[..self.commands.len()].copy_from_slice(&self.commands);
        output.apply(self.commands.len());
        // output_params rounds all placement values to the worklet's f32 ABI.
        // Keep that store before Mixer::take, including slot and flags.
        for row in self.placements.chunks_exact(6) {
            let row:[f32;6]=std::array::from_fn(|i|row[i] as f32); output.mixer.take(&row,false);
        }
    }
}
enum Command { Controls(Batch), Volume(f32,f64), Room([f64;audio::reverb::PARAM_COUNT],u32) }
struct Timed { block: u64, command: Command }

// Device callbacks retain the unused stereo quantum exactly as MasterOutput
// does. Sample uploads, resource imports, hashes and JSON finish before play.
pub struct SoundOutput {
    output: Output, schedule: Vec<Timed>, at: usize, next: usize,
    pub checks: usize, pub frames: usize,
}
impl SoundOutput {
    pub fn load(dir: &Path) -> Result<Self,String> {
        // The retained owner asserts its internal invariants. An invalid cold
        // archive is rejected here, before any cpal callback can observe it.
        std::panic::catch_unwind(||Self::admit(dir)).map_err(|_|"invalid sound archive records".to_string())?
    }
    fn admit(dir: &Path) -> Result<Self,String> {
        let meta:Value=serde_json::from_slice(&std::fs::read(dir.join("sound.json")).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        if meta["version"]!=1 || meta["rate"]!=48000 { return Err("unsupported sound archive".into()); }
        let bytes=std::fs::read(dir.join("blobs.bin")).map_err(|e|e.to_string())?;
        let data=|span:&Value| -> Result<&[u8],String> { let at=number(&span[0])?;let len=number(&span[1])?;bytes.get(at..at.checked_add(len).ok_or("sound span overflow")?).ok_or("sound span exceeds archive".into()) };
        let mut runtime=Runtime::new(number(&meta["seed"])? as u32);
        let mut collision=Planes(Vec::new());let mut batches=BTreeMap::new();let mut checks=0;
        for (i,op) in meta["ops"].as_array().ok_or("missing sound operations")?.iter().enumerate() {
            let values=f64s(data(&op["data"])?)?;
            if values.len()>runtime.input.len() { return Err("sound input capacity exceeded".into()); }
            runtime.input[..values.len()].copy_from_slice(&values);
            match op["name"].as_str() {
                Some("import") => { runtime.import(number(&op["kind"])?,values.len()); collision.0=runtime.planes.clone(); }
                Some("step") => {
                    runtime.clear_output();runtime.feed(values.len());
                    if runtime.phase.frame[frame::WORLD]>=0.0 { return Err("sound archive requires a native World collision owner".into()); }
                    runtime.step(&mut collision);
                }
                Some("ready") => { if values.len()!=ready::STRIDE { return Err("invalid sound readiness row".into()); } runtime.ready(); }
                _ => return Err("unknown sound operation".into()),
            }
            if op["name"]!="import" {
                let banks:[&[f64];3]=[&runtime.info,&runtime.phase.engine.commands,&runtime.phase.engine.placements];
                for (n,bank) in banks.iter().enumerate() { let hash=format!("{:x}",Sha256::digest(crate::session::bytes(bank))); if op["expected"][n].as_str()!=Some(hash.as_str()) { return Err(format!("sound control output differs at operation {i}, bank {n}")); } checks+=1; }
                batches.insert(i,Batch::control(&runtime));
            }
        }
        let o=&meta["options"];
        if number(&o["channels"])?!=2 || o["headphones"]==true || o["binaural"]==true { return Err("native sound archive requires stereo speakers".into()); }
        let mut output=Output::new(48000.0,2,o["enhanced"]==true,false,false,o["mono"]==true,cfg!(target_arch="x86_64"));
        output.reserve_controls(65536);
        let speakers=o["speakers"].as_array().ok_or("missing output speakers")?.iter().map(|s|if s.is_null(){Ok(None)}else{Ok(Some((finite(&s[0])?,finite(&s[1])?)))}).collect::<Result<Vec<_>,String>>()?;
        // Classic speaker layouts carry the same authored decoder matrix as
        // the worklet. It is a cold mixer descriptor, not headphone DSP.
        let decoder=if o["decoder"].is_null(){None}else{
            let d=&o["decoder"];let rows=d["rows"].as_array().ok_or("missing output decoder rows")?;
            if rows.len()>12{return Err("output decoder has too many channels".into());}
            let rows=rows.iter().map(|row|if row.is_null(){Ok(None)}else{let row=row.as_array().ok_or("invalid output decoder row")?;if row.len()>16{return Err("output decoder row is too wide".into());}Ok(Some(row.iter().map(finite).collect::<Result<Vec<_>,String>>()?))}).collect::<Result<Vec<_>,String>>()?;
            Some(audio::mixer::Decoder{front:finite(&d["front"])?,rows})
        };
        output.layout(number(&o["law"])? as u32,decoder,&speakers,integer(&o["center"])? as i32,integer(&o["lfe"])? as i32,finite(&o["original"])? as f32);
        if let Some(master)=&mut output.master {
            for eq in o["eq"].as_array().ok_or("missing output EQ")? { master.add_eq(audio::master::kind(number(&eq[0])? as u32),finite(&eq[1])? as f32,finite(&eq[2])? as f32,finite(&eq[3])? as f32); }
            if o["clarity"].as_f64().unwrap_or(0.0)!=0.0 { master.set_clarity(integer(&o["lfe"])?,finite(&o["clarity"])? as f32); }
            let (room,mask)=room(&o["room"])?;master.room(&room,mask,true);
            const DYNAMICS:[&str;audio::limiter::DYNAMICS_COUNT]=["threshold","ratio","knee","attack","release","makeup","skip"];
            let mut values=[0.0;audio::limiter::DYNAMICS_COUNT];let mut mask=0;
            for (i,key) in DYNAMICS.iter().enumerate() { if !o["dynamics"][key].is_null(){values[i]=finite(&o["dynamics"][key])?;mask|=1<<i;} }
            master.dynamics(&values,mask);
        }
        output.light_level(finite(&o["lightLevel"])?);output.volume(finite(&o["volume"])? as f32,0.0,finite(&o["time"])?,true);
        let mut samples=BTreeMap::new();let mut sample_count=0usize;let mut schedule=Vec::new();let mut previous=0;
        for m in meta["messages"].as_array().ok_or("missing output messages")? {
            let block=number(&m["block"])? as u64;if block<previous { return Err("sound messages are out of order".into()); }previous=block;
            let command=match m["type"].as_str() {
                Some("sample") => {
                    // Upload each version to an immutable native id. This is the
                    // loader's identity translation, not a change to voice policy.
                    let frames=number(&m["frames"])?;let channels=number(&m["channels"])?;let pcm=data(&m["data"])?;
                    if sample_count>=65536 || frames>16_777_216 || !(1..=12).contains(&channels) || pcm.len()!=frames.checked_mul(channels).and_then(|n|n.checked_mul(4)).ok_or("sample overflow")? { return Err("invalid sound sample capacity".into()); }
                    let dest=output.voices.sample_channels(sample_count,frames,channels,finite(&m["rate"])?);
                    for (v,b) in dest.iter_mut().zip(pcm.chunks_exact(4)){*v=f32::from_le_bytes(b.try_into().unwrap());if !v.is_finite(){return Err("non-finite decoded sample".into());}}
                    samples.insert(number(&m["id"])?,sample_count);sample_count+=1;continue;
                }
                Some("controls") => { let mut batch=batches.remove(&number(&m["control"])?).ok_or("missing or reused native sound output")?;remap(&mut batch.commands,&samples)?;Command::Controls(batch) }
                Some("voices") => { let mut commands=f64s(data(&m["data"])?)?;remap(&mut commands,&samples)?;Command::Controls(Batch {commands,placements:vec![],room:[0.0;audio::reverb::PARAM_COUNT],mask:0}) }
                Some("params") => Command::Controls(Batch {commands:vec![],placements:f64s(data(&m["data"])?)?,room:[0.0;audio::reverb::PARAM_COUNT],mask:0}),
                Some("volume") => Command::Volume(finite(&m["value"])? as f32,finite(&m["time"])?),
                Some("room") => { let (values,mask)=room(&m["params"])?;Command::Room(values,mask) }
                _ => return Err("unsupported native sound message".into()),
            };
            if let Command::Controls(b)=&command { if b.commands.len()>output.commands.len() || b.placements.len()>8192 || b.placements.len()%6!=0 {return Err("sound output batch capacity exceeded".into());} }
            schedule.push(Timed{block,command});
        }
        Ok(Self { output,schedule,at:0,next:FRAMES*2,checks,frames:number(&meta["frames"])? })
    }
    // Messages and process execute serially, as in AudioOutput. All sample
    // versions and command buffers already exist, so the callback only copies.
    fn quantum(&mut self) {
        let block=self.output.frame/FRAMES as u64;
        while let Some(timed)=self.schedule.get(self.at).filter(|m|m.block<=block) {
            match &timed.command {
                Command::Controls(b)=>b.apply(&mut self.output),
                Command::Volume(v,t)=>self.output.volume(*v,0.05,*t,false),
                Command::Room(v,m)=>if let Some(master)=&mut self.output.master {master.room(v,*m,false);},
            }
            self.at+=1;
        }
        self.output.process();self.next=0;
    }
    pub fn fill<T:cpal::SizedSample+cpal::FromSample<f32>>(&mut self,data:&mut [T],status:&crate::audio::Status) {
        use std::sync::atomic::Ordering;
        let mut peak=0.0f32;
        for sample in data {
            if self.next==FRAMES*2 {self.quantum();status.blocks.fetch_add(1,Ordering::Relaxed);}
            let v=self.output.output[(self.next%2)*FRAMES+self.next/2];peak=peak.max(v.abs());*sample=T::from_sample(v);self.next+=1;
        }
        status.peak.fetch_max(peak.to_bits(),Ordering::Relaxed);
    }
}
fn number(v:&Value)->Result<usize,String>{usize::try_from(v.as_u64().ok_or("invalid sound integer")?).map_err(|_|"sound integer overflow".into())}
fn integer(v:&Value)->Result<i64,String>{v.as_i64().ok_or("invalid signed sound integer".into())}
fn finite(v:&Value)->Result<f64,String>{v.as_f64().or_else(||v.as_bool().map(|b|u8::from(b) as f64)).filter(|v|v.is_finite()).ok_or("invalid finite sound value".into())}
fn f64s(data:&[u8])->Result<Vec<f64>,String>{if data.len()%8!=0{return Err("unaligned sound bank".into());}Ok(data.chunks_exact(8).map(|b|f64::from_le_bytes(b.try_into().unwrap())).collect())}
fn room(o:&Value)->Result<([f64;audio::reverb::PARAM_COUNT],u32),String>{
    const KEYS:[&str;audio::reverb::PARAM_COUNT]=["room","roomHF","decay","hfRatio","reflections","reflDelay","reverb","reverbDelay","diffusion","density","hfReference"];
    let mut values=[0.0;audio::reverb::PARAM_COUNT];let mut mask=0;for(i,key)in KEYS.iter().enumerate(){if !o[key].is_null(){values[i]=finite(&o[key])?;mask|=1<<i;}}Ok((values,mask))
}
fn remap(commands:&mut [f64],samples:&BTreeMap<usize,usize>)->Result<(),String>{
    use audio::commands::*;
    let mut at=0;while at<commands.len(){let op=commands[at] as u32;let n=arity(op);if n==0||at+n+1>commands.len(){return Err("invalid sound render command".into());}
        let index=match op{START=>Some(at+2),STREAM_START=>Some(at+3),_=>None};
        if let Some(i)=index{commands[i]=*samples.get(&(commands[i] as usize)).ok_or("sound command precedes decoded sample")? as f64;}
        at+=n+1;
    }Ok(())
}
