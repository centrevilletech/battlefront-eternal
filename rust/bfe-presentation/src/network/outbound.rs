//! NetworkInputs' cumulative look, held transitions and publication receipts.
//! Transport callbacks own delivery; numeric state advances from their result.
use super::{Output, Result, Words, EXCHANGE};
use crate::network_layout::net_op as ops;
#[derive(Clone,Copy,Default)]
pub struct Fire {pub button:u8,pub pressed:bool,pub held:bool}
// One command can contain a burst of clicks accumulated during a pause.
// Fixed event arrays admit the complete bounded action window at birth;
// overflow increments a counter while the input loop continues processing.
pub struct Outbound {
    life: Option<[f64;2]>, look:[f64;2], sent_look:[f64;2], sent_sequence:f64,
    held:Option<([u16;4096],usize)>, transition:Option<[f64;2]>, acked:f64,
    revision:u64, sent_revision:u64, sent_tick:Option<f64>,
    fire_held:[bool;2],fires:[Fire;8192],fire_count:usize,actions:[u8;4096],action_count:usize,pub refused:u64,
}
impl Default for Outbound {
    fn default()->Self{Self{life:None,look:[0.;2],sent_look:[0.;2],sent_sequence:0.,held:None,transition:None,acked:0.,revision:0,sent_revision:0,sent_tick:None,
        fire_held:[false;2],fires:[Fire::default();8192],fire_count:0,actions:[0;4096],action_count:0,refused:0}}
}
// Held names arrive as counted UTF-16 spans, without page sorting or JSON.
// Sorting span descriptors preserves JS lexical order and repeated names;
// the canonical representation is compared exactly, rather than by a hash.
fn held(input:&mut Words)->Result<([u16;4096],usize)> {
    let length=input.uint(4096)?;let mut source=[0u16;4096];
    for n in &mut source[..length] {*n=input.uint(65535)? as u16;}
    let mut ranges=[(0usize,0usize);128];let mut count=0;let mut at=0;
    while at<length {
        if count==ranges.len(){return Err(EXCHANGE);}
        let size=source[at]as usize;at+=1;
        if size>length-at{return Err(EXCHANGE);}
        ranges[count]=(at,size);count+=1;at+=size;
    }
    ranges[..count].sort_unstable_by(|&(a,n),&(b,m)|source[a..a+n].cmp(&source[b..b+m]));
    let mut result=[0u16;4096];let mut end=0;
    for &(at,size) in &ranges[..count] {
        result[end]=size as u16;end+=1;
        result[end..end+size].copy_from_slice(&source[at..at+size]);end+=size;
    }
    Ok((result,end))
}
impl Outbound {
    pub fn fires(&self)->&[Fire]{&self.fires[..self.fire_count]}
    pub fn actions(&self)->&[u8]{&self.actions[..self.action_count]}
    // A held callback is expendable presentation; press/release callbacks
    // enter the reliable lane once, which owns their subsequent retries.
    fn fire(&mut self,button:usize,pressed:bool,held:bool){
        if self.fire_count==self.fires.len(){self.refused+=1;return;}
        self.fires[self.fire_count]=Fire{button:button as u8,pressed,held};self.fire_count+=1;
    }
    // FireEdges.each preserves taps between held samples. A continuing hold
    // emits only a cosmetic callback; durable press/release edges remain on
    // the reliable transport until the server explicitly receipts them.
    fn triggers(&mut self,held:&[u16],length:usize,taps:[usize;2]){
        for (button, &tap_count) in taps.iter().enumerate() {
            let key=[77u16,111,117,115,101,if button==0{48}else{50}];
            let mut at=0;let mut down=false;
            while at<length{let n=held[at]as usize;at+=1;if held[at..at+n]==key{down=true;}at+=n;}
            let mut emitted=false;
            for tap in 0..tap_count{
                if self.fire_held[button]{self.fire(button,false,false);self.fire_held[button]=false;}
                self.fire(button,true,false);self.fire_held[button]=true;emitted=true;
                if tap+1<tap_count||!down{self.fire(button,false,false);self.fire_held[button]=false;}
            }
            if down!=self.fire_held[button]{self.fire(button,down,false);self.fire_held[button]=down;emitted=true;}
            if !emitted&&self.fire_held[button]{self.fire(button,true,true);}
        }
    }
    // EntityInputSender's cadence uses the most recently delivered held
    // revision. A failed transport leaves that revision and clock unchanged.
    pub fn allow(&self,tick:f64,edges:usize,interval:usize)->bool {
        edges!=0 || self.sent_tick.is_none_or(|last|self.revision!=self.sent_revision || tick<last || tick-last>=interval.clamp(1,2)as f64)
    }
    // Explicit cold fixture capture is the only JS reader of accounting.
    // Live send never creates a second look/held/life/ACK state on the page.
    pub fn capture(&self)->String {
        let names=self.held.as_ref().map(|(words,length)|{
            let mut names=Vec::new();let mut at=0;
            while at<*length {let size=words[at]as usize;at+=1;names.push(String::from_utf16_lossy(&words[at..at+size]));at+=size;}
            serde_json::to_string(&names).unwrap()
        });
        serde_json::json!({"life":self.life.map(|p|format!("{}:{}",if p[0]==0.{0.}else{p[0]},if p[1]==0.{0.}else{p[1]})),"look":self.look,"sentLook":self.sent_look,
            "sentSequence":self.sent_sequence,"held":names,"acked":self.acked,"refused":self.refused}).to_string()
    }
    // A frame batch validates every command before committing its first one.
    // This walk checks framing only: held-name sorting and look arithmetic
    // still happen once, in run, after the enclosing exchange is accepted.
    pub fn validate_command(input:&mut Words)->Result<()> {
        input.span(4)?;
        let length=input.uint(4096)?;
        let names=input.span(length)?;
        let mut at=0;let mut count=0;
        while at<names.len(){
            if count==128{return Err(EXCHANGE);}count+=1;
            let size=names[at];
            if !size.is_finite()||size<0.||size.fract()!=0.||size>65535.{return Err(EXCHANGE);}
            at+=1;let end=at.checked_add(size as usize).ok_or(EXCHANGE)?;
            let span=names.get(at..end).ok_or(EXCHANGE)?;
            for &word in span{if !word.is_finite()||word<0.||word.fract()!=0.||word>65535.{return Err(EXCHANGE);}}
            at=end;
        }
        let looks=input.uint(super::ROOM/2)?;input.span(looks*2)?;
        let actions=input.uint(4096)?;input.span(actions)?;input.finish()
    }
    pub fn run(&mut self,op:u32,input:&mut Words,out:&mut Output)->Result<()> {
        let required=match op{ops::OUTBOUND_COMMAND=>9,ops::OUTBOUND_ACK=>2,ops::OUTBOUND_SENT=>4,ops::OUTBOUND_RESET|ops::OUTBOUND_CLEAR=>0,_=>return Err(EXCHANGE)};
        if out.values.len().saturating_sub(out.at)<required{return Err("page networking output exceeds bounds");}
        match op {
            ops::OUTBOUND_RESET=>{input.finish()?;*self=Self::default();}
            // Focus pauses retain cumulative look but abandon held backups.
            // Their next movement publication starts a fresh cadence window.
            ops::OUTBOUND_CLEAR=>{input.finish()?;self.held=None;self.transition=None;self.sent_tick=None;self.fire_held=[false;2];}
            ops::OUTBOUND_ACK=>{
                let sequence=input.number()?;input.finish()?;
                if sequence.is_finite() && sequence.fract()==0. && sequence.abs()<=9007199254740991. && sequence>self.acked {
                    self.acked=sequence;
                    if self.transition.is_some_and(|p|p[0]<=sequence){self.transition=None;}
                }
                out.extend(&[self.acked,self.transition.map_or(0.,|p|p[0])])?;
            }
            ops::OUTBOUND_COMMAND=>{
                let life=[input.number()?,input.number()?];let sequence=input.number()?;let tick=input.number()?;
                let (held,length)=held(input)?;let count=input.uint(super::ROOM/2)?;
                let looks=input.span(count*2)?;
                let count=input.uint(4096)?;let actions=input.span(count)?;
                input.finish()?;
                let reset=self.life!=Some(life);
                // A new life starts with both triggers released, including
                // a reconnect whose first sample already holds a fire key.
                if reset {*self=Self::default();self.life=Some(life);}
                // Add individual samples in source order. A page-side sum
                // would change floating-point cancellation across commands.
                for look in looks.as_chunks::<2>().0{self.look[0]+=look[0];self.look[1]+=look[1];}
                let changed=self.held.as_ref().is_some_and(|(last,n)|*n!=length || last[..*n]!=held[..length]);
                if changed {self.transition=Some([sequence,tick]);}
                if changed || self.held.is_none(){self.revision=self.revision.wrapping_add(1);}
                // Action indices are transport registry values. Unknown DOM
                // names remain absent from the reliable action publication.
                let mut taps=[0usize;2];self.fire_count=0;self.action_count=0;
                for &action in actions{
                    if action==21.{taps[0]+=1;}else if action==22.{taps[1]+=1;}else if (0.0..21.0).contains(&action)&&action.fract()==0.{self.actions[self.action_count]=action as u8;self.action_count+=1;}
                }
                self.triggers(&held,length,taps);
                self.held=Some((held,length));
                if self.transition.is_some_and(|p|p[0]<=self.acked || p[1]<tick-24.){self.transition=None;}
                out.extend(&[reset as u8 as f64,changed as u8 as f64,self.transition.map_or(0.,|p|p[0]),self.look[0],self.look[1],0.,0.,self.revision as f64,self.transition.is_some_and(|p|p[0]<sequence)as u8 as f64])?;
            }
            // A receipt belongs to the original state's revision, including
            // a held backup sent alongside a newer state from the same life.
            ops::OUTBOUND_SENT=>{
                let sequence=input.number()?;let look=[input.number()?,input.number()?];
                let tick=input.number()?;let revision=input.number()?;input.finish()?;
                if sequence>self.sent_sequence {
                    out.extend(&[1.,look[0]-self.sent_look[0],look[1]-self.sent_look[1],sequence])?;
                    // Cadence follows this state's original held revision;
                    // an older transition backup cannot masquerade as latest.
                    self.sent_look=look;self.sent_sequence=sequence;self.sent_tick=Some(tick);self.sent_revision=revision as u64;
                } else {out.extend(&[0.,0.,0.,self.sent_sequence])?;}
            }
            _=>return Err(EXCHANGE),
        }
        input.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_exchanges_leave_accounting_and_callbacks_unchanged(){
        let mut state=Outbound::default();let mut numbers=[0.;16];
        let command=[1.,1.,1.,1.,0.,1.,0.25,-0.5,1.,21.];
        state.run(ops::OUTBOUND_COMMAND,&mut Words::new(&command),&mut Output{values:&mut numbers,at:0}).unwrap();
        let before=state.capture();let fires=state.fire_count;let actions=state.action_count;
        // A reset with trailing data, incomplete look pair and malformed
        // held framing must never reset a life or accumulate a partial look.
        for (op,words) in [(ops::OUTBOUND_RESET,&[9.][..]),(ops::OUTBOUND_CLEAR,&[9.][..]),(ops::OUTBOUND_ACK,&[99.,9.][..]),
            (ops::OUTBOUND_COMMAND,&[2.,2.,2.,2.,0.,1.,100.][..]),(ops::OUTBOUND_COMMAND,&[2.,2.,2.,2.,2.,4.,65.,0.,0.][..]),
            (ops::OUTBOUND_COMMAND,&[2.,2.,2.,2.,0.,1.,100.,200.,1.][..]),(ops::OUTBOUND_SENT,&[99.,1.,2.,3.,4.,5.][..])]{
            assert!(state.run(op,&mut Words::new(words),&mut Output{values:&mut numbers,at:0}).is_err());
            assert_eq!(state.capture(),before);assert_eq!(state.fire_count,fires);assert_eq!(state.action_count,actions);
        }
        assert!(state.run(ops::OUTBOUND_COMMAND,&mut Words::new(&command),&mut Output{values:&mut [],at:0}).is_err());
        assert_eq!(state.capture(),before);
    }
}
