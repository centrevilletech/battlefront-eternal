//! connection.js's protected packet, ACK/loss and timer phase. The device
//! supplies a monotonic millisecond clock and borrowed transmit/delivery hooks.
//! No socket, JSON policy or service admission is called by a packet decoder.
use std::net::SocketAddr;
use crate::{Error,Result,crypto::{self,Aead},handshake::Keys,codec::{Reader,Writer,Ranges,Replay},streams::{SendStream,ReceiveStream,Span},state::{FragmentPart,Fragments,Queue}};
use crate::layout::{data as d,frame as f,packet as p,limits as l,close_code as close,stream as stream_id};
const PLAIN:usize=l::MAX_DATAGRAM-d::BYTES-l::TAG;
// Recent loss is tracked in 21 rolling 50 ms buckets covering a 1000 ms window,
// matching connection.js LOSS_WINDOW / LOSS_BUCKET_MS / LOSS_BUCKETS.
const LOSS_WINDOW_MS: f64 = 1000.0;
const LOSS_BUCKET_MS: f64 = 50.0;
const LOSS_BUCKETS: usize = (LOSS_WINDOW_MS as usize) / (LOSS_BUCKET_MS as usize) + 1;
// Three independent lanes prevent head-of-line blocking between unreliable state
// snapshots, ordered game events, and bulk transfers.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Lane{State,Events,Bulk}
// Both callbacks are synchronous loans. A device may copy into its own
// preallocated mailbox, but the protocol never owns a host callback object.
pub trait Sink{
    fn packet(&mut self,bytes:&[u8],path:SocketAddr);
    fn message(&mut self,lane:Lane,text:bool,bytes:&[u8]);
}
// Fixed-size close record storing up to 200 UTF-8 reason bytes without heap allocation.
#[derive(Clone,Copy)]
pub struct Closed{pub code:u64,pub reason:[u8;200],pub length:usize,pub remote:bool}
impl Closed{pub fn reason(&self)->&[u8]{&self.reason[..self.length]}}
// Cumulative packet, byte, loss, and authentication counters matching connection.js counters.
#[derive(Clone,Copy,Default)]
pub struct Stats{pub sent:u64,pub received:u64,pub sent_bytes:u64,pub received_bytes:u64,pub datagrams_sent:u64,pub datagrams_received:u64,pub datagram_packets_lost:u64,pub datagram_packets_acked:u64,pub lost:u64,pub spurious:u64,pub retransmitted:u64,pub duplicates:u64,pub auth_failures:u64,pub probes:u64}
// Associates a transmitted STREAM frame span with its lane index (0 = Events, 1 = Bulk).
#[derive(Clone,Copy,Default)]
struct SentSpan{lane:usize,span:Span}
// Per-packet in-flight record in the fixed MAX_SENT_PACKETS ring buffer.
#[derive(Clone,Copy,Default)]
struct Meta{active:bool,number:u32,at:f64,reliable:bool,state:usize,streams:[SentSpan;2],count:usize}
// Active RFC 9000 path challenge awaiting an 8-byte PATH_RESPONSE from `path`.
#[derive(Clone,Copy)]
struct Challenge{path:SocketAddr,bytes:[u8;8],sent:f64}
// ACK ranges descend on the wire and stay half-open internally. Decoding
// one packet needs only this stack value, even at the maximum range count.
struct Ack{largest:u64,delay:f64,rows:[[u64;2];l::MAX_DECODED_ACK_RANGES],count:usize}
impl Ack{
    fn read(r:&mut Reader<'_>)->Result<Self>{
        let largest=r.varint()?;let delay=r.varint()?as f64*8./1000.;let extra=r.varint()?;let first=r.varint()?;
        if first>largest||extra>=l::MAX_DECODED_ACK_RANGES as u64{return Err(Error("invalid ack"));}
        let mut a=Self{largest,delay,rows:[[0;2];l::MAX_DECODED_ACK_RANGES],count:extra as usize+1};a.rows[0]=[largest-first,largest+1];let mut smallest=largest-first;
        for i in 1..a.count{let gap=r.varint()?;let n=r.varint()?;let high=smallest.checked_sub(gap).and_then(|v|v.checked_sub(2)).ok_or(Error("invalid ack"))?;
            let low=high.checked_sub(n).ok_or(Error("invalid ack"))?;a.rows[i]=[low,high+1];smallest=low;}Ok(a)
    }
    // Lost metadata is unordered after bounded ring eviction. Binary search
    // avoids scanning every ACK range for every remembered packet.
    fn has(&self,n:u64)->bool{
        let mut lo=0;let mut hi=self.count;while lo<hi{let mid=(lo+hi)/2;let [a,b]=self.rows[mid];if n>=b{hi=mid;}else if n<a{lo=mid+1;}else{return true;}}false
    }
}
// Established BFE-UDP/1 session owner with preallocated stream rings, fragment bank, and sent-packet table.
pub struct Connection{
    pub local:u32,pub remote:u32,send:Aead,receive:Aead,previous:Option<Aead>,next:Option<Aead>,send_phase:u8,receive_phase:u8,phase_start:u32,key_interval:u32,
    number:u32,floor:u32,sent:Vec<Meta>,lost_history:[Option<(u32,f64)>;256],lost_head:usize,reliable:usize,stream_flight:usize,last_reliable:f64,
    tx:[SendStream;2],rx:[ReceiveStream;2],state:Queue,fragments:Fragments,received:Ranges,replay:Replay,
    largest_at:f64,ack_needed:bool,ack_deadline:f64,largest_acked:Option<u32>,loss_time:f64,pto_count:u32,
    pub rtt:f64,pub rtt_var:f64,pub latest_rtt:f64,pub min_rtt:f64,has_rtt:bool,pub window:usize,threshold:usize,recovery:f64,reorder:u32,time_threshold:f64,
    control:[[u8;9];l::MAX_CONTROL_FRAMES],control_count:usize,ping:bool,handshake_done:bool,pub handshake_confirmed:bool,
    pub path:SocketAddr,challenge:Option<Challenge>,last_challenge:f64,probe_tokens:f64,probe_at:f64,
    last_received:f64,last_sent:f64,pending_close:Option<Closed>,pub closed:Option<Closed>,close_at:f64,last_echo:f64,
    loss_bucket_id: [i64; LOSS_BUCKETS],
    loss_bucket_lost: [u32; LOSS_BUCKETS],
    loss_bucket_total: [u32; LOSS_BUCKETS],
    pub stats:Stats,
}
impl Connection{
    // Reserve each stream, state queue, fragment bank and packet table now.
    // The first encrypted packet must not trigger a lazy native reservation.
    pub fn new(keys:Keys,path:SocketAddr,now:f64,rtt:f64)->Self{
        let rtt=if rtt.is_finite(){rtt.max(1.)}else{100.};
        Self{local:keys.local,remote:keys.remote,send:keys.send,receive:keys.receive,previous:None,next:None,send_phase:0,receive_phase:0,phase_start:0,key_interval:l::KEY_INTERVAL as u32,
            number:0,floor:0,sent:vec![Meta::default();l::MAX_SENT_PACKETS],lost_history:[None;256],lost_head:0,reliable:0,stream_flight:0,last_reliable:f64::NEG_INFINITY,
            tx:[SendStream::new(),SendStream::new()],rx:[ReceiveStream::new(),ReceiveStream::new()],state:Queue::new(),fragments:Fragments::new(),received:Ranges::new(l::MAX_RECEIVED_RANGES+1),replay:Replay::default(),
            largest_at:f64::NEG_INFINITY,ack_needed:false,ack_deadline:f64::INFINITY,largest_acked:None,loss_time:f64::INFINITY,pto_count:0,
            rtt,rtt_var:rtt/2.,latest_rtt:rtt,min_rtt:f64::INFINITY,has_rtt:false,window:32*l::MAX_DATAGRAM,threshold:usize::MAX,recovery:f64::NEG_INFINITY,reorder:3,time_threshold:9./8.,
            control:[[0;9];l::MAX_CONTROL_FRAMES],control_count:0,ping:false,handshake_done:false,handshake_confirmed:false,path,challenge:None,last_challenge:f64::NEG_INFINITY,probe_tokens:64.,probe_at:now,
            last_received:now,last_sent:now,pending_close:None,closed:None,close_at:f64::NEG_INFINITY,last_echo:f64::NEG_INFINITY,
            loss_bucket_id: [-1; LOSS_BUCKETS],
            loss_bucket_lost: [0; LOSS_BUCKETS],
            loss_bucket_total: [0; LOSS_BUCKETS],
            stats:Stats::default()}
    }
    // Replaceable state evicts older state. A reliable byte-budget violation
    // instead closes explicitly; silently losing an ordered byte is invalid.
    pub fn queue(&mut self,lane:Lane,text:bool,bytes:&[u8])->Result<bool>{
        if self.closed.is_some()||self.pending_close.is_some(){return Ok(false);}
        let result=match lane{Lane::State=>self.state.push(bytes,text),Lane::Events=>self.tx[0].write(bytes,text),Lane::Bulk=>self.tx[1].write(bytes,text)};
        if let Err(error)=result{self.close(close::PROTOCOL as u64,error.0.as_bytes());return Err(error);}Ok(true)
    }
    // Total unacknowledged or unsent reliable bytes queued across Events and Bulk streams.
    pub fn backlog(&self)->usize{self.tx.iter().map(SendStream::pending).sum()}
    // Request an ACK-eliciting PING frame on the next flush.
    pub fn ping(&mut self){self.ping=true;}
    /// `src/net/connection.js` `Connection.confirmHandshake`: queues a
    /// `HANDSHAKE_DONE` frame to confirm session establishment to the client.
    pub fn confirm_handshake(&mut self) {
        self.handshake_done = true;
    }
    /// `src/net/connection.js` `Connection.recentLoss`: fraction of
    /// state-carrying packets lost over the 1000 ms sliding bucket window.
    pub fn recent_loss(&self, now: f64) -> f64 {
        let oldest = ((now - LOSS_WINDOW_MS) / LOSS_BUCKET_MS).floor() as i64;
        let mut lost_count = 0u64;
        let mut count = 0u64;
        for slot in 0..LOSS_BUCKETS {
            if self.loss_bucket_id[slot] >= oldest {
                lost_count += self.loss_bucket_lost[slot] as u64;
                count += self.loss_bucket_total[slot] as u64;
            }
        }
        if count > 0 {
            lost_count as f64 / count as f64
        } else {
            0.0
        }
    }
    // Record one state-carrying packet outcome into the current 50 ms loss bucket,
    // resetting the slot whenever the monotonic clock advances into a new bucket id.
    fn loss_sample(&mut self, now: f64, lost_packet: bool) {
        let bucket = (now / LOSS_BUCKET_MS).floor() as i64;
        let slot = bucket.rem_euclid(LOSS_BUCKETS as i64) as usize;
        if self.loss_bucket_id[slot] != bucket {
            self.loss_bucket_id[slot] = bucket;
            self.loss_bucket_lost[slot] = 0;
            self.loss_bucket_total[slot] = 0;
        }
        self.loss_bucket_total[slot] += 1;
        if lost_packet {
            self.loss_bucket_lost[slot] += 1;
        }
    }
    // Store a bounded reason until the next flush can send it. Device code
    // can request a close without borrowing a socket inside an input callback.
    pub fn close(&mut self,code:u64,reason:&[u8]){
        if self.closed.is_some()||self.pending_close.is_some(){return;}let mut why=[0;200];let n=reason.len().min(why.len());why[..n].copy_from_slice(&reason[..n]);
        self.pending_close=Some(Closed{code:code.min(l::MAX_SAFE_INTEGER),reason:why,length:n,remote:false});
    }
    // Encode a CLOSE frame carrying the varint close code and bounded UTF-8 reason slice.
    fn write_close(info:Closed,out:&mut Writer<'_>)->Result<()>{out.byte(f::CLOSE as u8)?;out.varint(info.code)?;out.varint(info.length as u64)?;out.span(info.reason())}
    // Authenticate before committing either the replay horizon or key phase.
    // The caller may immediately reuse its UDP read buffer after this returns.
    pub fn receive(&mut self,packet:&[u8],path:SocketAddr,now:f64,sink:&mut impl Sink)->bool{
        // Re-echo a local CLOSE frame at most every 100 ms while draining stray peer packets.
        if let Some(info)=self.closed{
            if !info.remote&&now<self.close_at+1000_f64.max(3.*self.pto())&&now-self.last_echo>100.{
                self.last_echo=now;let mut bytes=[0;PLAIN];let mut w=Writer::new(&mut bytes);if Self::write_close(info,&mut w).is_ok(){let n=w.at;let _=self.emit(&bytes[..n],Meta::default(),self.path,now,sink);}
            }return false;
        }
        if packet.len()<d::BYTES+l::TAG+1||packet.len()>l::MAX_DATAGRAM||!matches!(packet[d::TYPE]as usize,p::DATA|p::DATA_PHASE){return false;}
        let Ok(recv_bytes) = packet[d::RECEIVER..d::RECEIVER+4].try_into() else { return false; };
        if u32::from_le_bytes(recv_bytes)!=self.local{return false;}
        let Ok(num_bytes) = packet[d::NUMBER..d::NUMBER+4].try_into() else { return false; };
        let number=u32::from_le_bytes(num_bytes);if self.replay.seen(number){self.stats.duplicates+=1;return false;}
        let phase=if packet[d::TYPE]as usize==p::DATA_PHASE{1}else{0};let n=packet.len()-d::BYTES-l::TAG;
        let mut clear=[0;PLAIN];clear[..n].copy_from_slice(&packet[d::BYTES..d::BYTES+n]);let tag=&packet[d::BYTES+n..];let header=&packet[..d::BYTES];let mut rotated=false;
        // Previous-phase stragglers are legitimate below phase_start. Recopy
        // ciphertext between trials so a failed decrypt cannot affect another key.
        let authentic=if phase==self.receive_phase{self.receive.open(header,number,&mut clear[..n],tag)}else{
            let old=self.previous.as_ref().filter(|_|number<self.phase_start).is_some_and(|a|a.open(header,number,&mut clear[..n],tag));
            if old{true}else{
                clear[..n].copy_from_slice(&packet[d::BYTES..d::BYTES+n]);if self.next.is_none(){self.next=self.receive.next().ok();}
                rotated=self.next.as_ref().is_some_and(|a|a.open(header,number,&mut clear[..n],tag));rotated
            }
        };
        if !authentic{self.stats.auth_failures+=1;return false;}
        if rotated{if let Some(next)=self.next.take(){self.previous=Some(std::mem::replace(&mut self.receive,next));self.receive_phase=phase;self.phase_start=number;}}
        // ACK history may be shorter than replay history. Trimming its old
        // ranges never permits a replay that the bitmap has already consumed.
        let newest=self.replay.largest.is_none_or(|largest|number>largest);self.replay.commit(number);
        self.received.add(number as u64,number as u64+1);
        if newest{self.largest_at=now;}
        if self.received.len()>l::MAX_RECEIVED_RANGES{let before=self.received.rows()[self.received.len()-l::MAX_ACK_RANGES][0];self.received.trim(before);}
        self.received.trim(self.replay.largest.unwrap_or(0).saturating_sub(l::REPLAY_WINDOW as u32-1)as u64);
        self.stats.received+=1;self.stats.received_bytes+=packet.len()as u64;self.last_received=now;
        match self.frames(&clear[..n],path,now,sink){
            Ok(probing)=>if newest&&!probing&&path!=self.path{self.challenge_path(path,now,sink);},
            Err(error)=>self.close(close::PROTOCOL as u64,error.0.as_bytes()),
        }
        if self.closed.is_none()&&((self.ack_needed&&now>=self.ack_deadline)||self.control_count>0||self.handshake_done||self.pending_close.is_some()){let _=self.flush(now,sink);}true
    }
    // ACK-only packets never elicit another ACK. Reliable control and stream
    // data use the shorter deadline; state can piggyback its acknowledgement.
    fn frames(&mut self,bytes:&[u8],path:SocketAddr,now:f64,sink:&mut impl Sink)->Result<bool>{
        let mut r=Reader::new(bytes);let mut probing=true;let mut acked=false;let mut answered=false;let mut elicit=0;
        while r.remaining()>0{
            let kind=r.byte()?as usize;
            if kind==f::PADDING{continue;}
            if kind!=f::PATH_CHALLENGE&&kind!=f::PATH_RESPONSE{probing=false;}
            match kind{
                f::PING=>elicit=2,
                f::HANDSHAKE_DONE=>{elicit=2;self.handshake_confirmed=true;},
                // Reject duplicate or future ACKs before their table walk. An
                // authenticated peer still cannot make receive work unbounded.
                f::ACK=>{if acked{return Err(Error("repeated ack frame"));}let ack=Ack::read(&mut r)?;if ack.largest>=self.number as u64{return Err(Error("ack of an unsent packet"));}acked=true;self.ack(&ack,now)?;},
                f::DATAGRAM=>{let text=r.byte()?&1!=0;let n=r.varint()?;let bytes=r.wide_span(n)?;sink.message(Lane::State,text,bytes);self.stats.datagrams_received+=1;elicit=elicit.max(1);},
                f::FRAGMENT=>{let text=r.byte()?&1!=0;let id=r.varint()?;let index=r.byte()?as usize;let count=r.byte()?as usize;let n=r.varint()?;let bytes=r.wide_span(n)?;
                    if self.fragments.receive(FragmentPart{id,index,count,text,bytes},now,&mut|text,bytes|sink.message(Lane::State,text,bytes))?{self.stats.datagrams_received+=1;}elicit=elicit.max(1);},
                // Each ordered lane has its own delivery cursor. Loss on bulk
                // cannot hold an event that fits the shared congestion window.
                f::STREAM=>{let id=r.byte()?;let at=r.varint()?;let n=r.varint()?;let bytes=r.wide_span(n)?;
                    let (index,lane)=match id as usize{stream_id::EVENTS=>(0,Lane::Events),stream_id::BULK=>(1,Lane::Bulk),_=>return Err(Error("unknown stream"))};
                    self.rx[index].receive(at,bytes,&mut|text,bytes|sink.message(lane,text,bytes))?;elicit=2;},
                f::PATH_CHALLENGE=>{let bytes=r.span(8)?;if !answered{answered=true;self.answer(bytes,path,now,sink)?;}elicit=2;},
                f::PATH_RESPONSE=>{let bytes=r.span(8)?;if self.challenge.is_some_and(|c|c.path==path&&c.bytes==bytes){self.path=path;self.challenge=None;}},
                f::CLOSE=>{let code=r.varint()?;let n=r.varint()?;let bytes=r.wide_span(n)?;let mut reason=[0;200];let length=bytes.len().min(reason.len());reason[..length].copy_from_slice(&bytes[..length]);
                    self.closed=Some(Closed{code,reason,length,remote:true});self.close_at=now;return Ok(true);},
                _=>return Err(Error("unknown frame")),
            }
            if self.closed.is_some(){return Ok(probing);}
        }
        if elicit>0{self.ack_needed=true;self.ack_deadline=self.ack_deadline.min(now+if elicit>1{25.}else{40.});}Ok(probing)
    }
    // Answers are source-bound, even while the connection is validating a new
    // remote address. A spoofed authenticated peer cannot amplify unlimited
    // responses toward another address by filling the control queue.
    fn answer(&mut self,bytes:&[u8],path:SocketAddr,now:f64,sink:&mut impl Sink)->Result<()>{
        let mut frame=[0;9];frame[0]=f::PATH_RESPONSE as u8;frame[1..].copy_from_slice(bytes);
        if path==self.path{if self.control_count<self.control.len(){self.control[self.control_count]=frame;self.control_count+=1;}}
        else{self.probe_tokens=64_f64.min(self.probe_tokens+(now-self.probe_at).max(0.)*128./1000.);self.probe_at=now;
            if self.probe_tokens>=1.{self.probe_tokens-=1.;self.emit(&frame,Meta::default(),path,now,sink)?;}}
        Ok(())
    }
    // Keep one challenge for the newest active candidate, rate limited and
    // retried only while that address continues to send authentic traffic.
    fn challenge_path(&mut self,path:SocketAddr,now:f64,sink:&mut impl Sink){
        if self.challenge.is_none_or(|c|c.path!=path){
            if now-self.last_challenge<50.{return;}let Ok(bytes)=crypto::random()else{return;};self.challenge=Some(Challenge{path,bytes,sent:f64::NEG_INFINITY});
        }else if self.challenge.is_some_and(|c|now-c.sent<self.pto().max(50.)){return;}
        if let Some(c)=&mut self.challenge{c.sent=now;self.last_challenge=now;let mut bytes=[0;9];bytes[0]=f::PATH_CHALLENGE as u8;bytes[1..].copy_from_slice(&c.bytes);let _=self.emit(&bytes,Meta::default(),path,now,sink);}
    }
    // Remove in-flight accounting exactly once, whether an ACK or declared
    // loss reaches the entry first. A newer ring occupant is never released.
    fn forget(&mut self,number:u32)->Option<Meta>{
        // A fixed packet ring also bounds peers that keep sending but never
        // ACK. Evicting an old entry declares its reliable ranges lost first.
        let slot=number as usize%self.sent.len();let meta=self.sent[slot];if !meta.active||meta.number!=number{return None;}
        self.sent[slot].active=false;if meta.reliable{self.reliable-=1;}for row in &meta.streams[..meta.count]{self.stream_flight-=row.span.length;}Some(meta)
    }
    // Remember a bounded loss history for spurious-loss adaptation. Stream
    // ranges, rather than packet ciphertext, are what recovery retransmits.
    fn loss(&mut self,number:u32,now:f64)->Result<Option<Meta>>{
        let Some(meta)=self.forget(number)else{return Ok(None);};self.lost_history[self.lost_head]=Some((number,now));self.lost_head=(self.lost_head+1)%self.lost_history.len();self.stats.lost+=1;
        if meta.state > 0 {
            self.stats.datagram_packets_lost += 1;
            self.loss_sample(now, true);
        }
        for row in &meta.streams[..meta.count]{self.tx[row.lane].loss(row.span)?;}Ok(Some(meta))
    }
    // Advance the lower bound of unacknowledged packet numbers past inactive ring slots.
    fn trim_sent(&mut self){while self.floor<self.number{let row=self.sent[self.floor as usize%self.sent.len()];if row.active&&row.number==self.floor{break;}self.floor+=1;}}
    // An ACK releases delivered byte ranges independently from packet order.
    // The largest newly ACKed eliciting packet supplies the RTT sample.
    fn ack(&mut self,ack:&Ack,now:f64)->Result<()>{
        for row in &mut self.lost_history{if let Some((n,at))=*row{if now-at>2000.{*row=None;}else if ack.has(n as u64){*row=None;self.stats.spurious+=1;self.reorder=(self.reorder+1).min(16);self.time_threshold=(self.time_threshold+0.125).min(2.);}}}
        let mut newly=false;let mut largest_time=None;let mut stream_bytes=0;
        for number in self.floor..self.number{
            if number as u64>ack.largest{break;}
            if !ack.has(number as u64){continue;}
            let Some(meta)=self.forget(number)else{continue;};newly=true;
            if number as u64==ack.largest&&(meta.reliable||meta.state>0){largest_time=Some(meta.at);}
            for row in &meta.streams[..meta.count]{self.tx[row.lane].ack(row.span)?;stream_bytes+=row.span.length;}
            if meta.state > 0 {
                self.stats.datagram_packets_acked += 1;
                self.loss_sample(now, false);
            }
        }
        self.trim_sent();if !newly{return Ok(());}self.largest_acked=Some(self.largest_acked.map_or(ack.largest as u32,|n|n.max(ack.largest as u32)));
        if let Some(time)=largest_time{self.rtt_sample(now-time,ack.delay.min(40.));}
        if stream_bytes>0{let gain=if self.window<self.threshold{stream_bytes}else{(l::MAX_DATAGRAM*stream_bytes/self.window).max(1)};self.window=(self.window+gain).min(l::MAX_STREAM_BUFFER);}
        self.pto_count=0;self.detect_loss(now)
    }
    // Remove the peer's bounded ACK delay before smoothing. Otherwise frame
    // cadence would become part of the network lead and inflate input latency.
    fn rtt_sample(&mut self,sample:f64,delay:f64){
        if sample<0.||!sample.is_finite(){return;}let adjusted=(sample-delay).max(1.);self.latest_rtt=adjusted;self.min_rtt=self.min_rtt.min(adjusted);
        if !self.has_rtt{self.rtt=adjusted;self.rtt_var=adjusted/2.;self.has_rtt=true;}
        else{self.rtt_var=0.75*self.rtt_var+0.25*(self.rtt-adjusted).abs();self.rtt=0.875*self.rtt+0.125*adjusted;}
    }
    // RFC 9002 probe timeout (PTO) duration including 25 ms maximum ACK delay.
    fn pto(&self)->f64{self.rtt+(4.*self.rtt_var).max(1.)+25.}
    // Exponentially backed-off PTO deadline when reliable frames remain in flight.
    fn pto_deadline(&self)->f64{if self.reliable>0{self.last_reliable+self.pto()*2_f64.powi(self.pto_count.min(6)as i32)}else{f64::INFINITY}}
    // Packet and time thresholds both follow the peer's newest ACK. NewReno
    // halves the window only once per recovery interval, not once per packet.
    fn detect_loss(&mut self,now:f64)->Result<()>{
        self.loss_time=f64::INFINITY;let Some(largest)=self.largest_acked else{return Ok(());};let delay=(self.time_threshold*self.latest_rtt.max(self.rtt)).max(1.);let mut newest=f64::NEG_INFINITY;
        for n in self.floor..self.number{if n>largest{break;}let meta=self.sent[n as usize%self.sent.len()];if !meta.active||meta.number!=n{continue;}
            if n as u64+self.reorder as u64<=largest as u64||meta.at<=now-delay{if let Some(lost)=self.loss(n,now)?{if lost.count>0{newest=newest.max(lost.at);}}}
            else{self.loss_time=self.loss_time.min(meta.at+delay);}}
        self.trim_sent();if newest>self.recovery{self.recovery=now;self.window=(self.window/2).max(8*l::MAX_DATAGRAM);self.threshold=self.window;}Ok(())
    }
    // The device owns timer scheduling. Poll computes only due work, keeping
    // a paused display or long receive burst from creating timer allocations.
    pub fn poll(&mut self,now:f64,sink:&mut impl Sink)->Result<()>{
        if self.closed.is_some(){return Ok(());}
        if now-self.last_received>=l::IDLE_MS as f64{self.close(close::IDLE as u64,b"idle timeout");}
        if now>=self.loss_time{self.detect_loss(now)?;}
        if now>=self.pto_deadline(){self.pto_count+=1;self.stats.probes+=1;let mut found=false;
            for n in self.floor..self.number{let meta=self.sent[n as usize%self.sent.len()];if meta.active&&meta.number==n&&meta.count>0{for row in &meta.streams[..meta.count]{self.tx[row.lane].loss(row.span)?;}found=true;break;}}
            if !found{self.ping=true;}}
        if now-self.last_sent>=l::KEEPALIVE_MS as f64{self.ping=true;}self.flush(now,sink)
    }
    // Report only the newest bounded ranges. Older packets remain protected
    // by Replay even when their acknowledgements have left this small history.
    fn ack_frame(&self,now:f64,out:&mut Writer<'_>)->Result<()>{
        let rows=self.received.rows();if rows.is_empty(){return Ok(());}let top=rows.len()-1;let count=rows.len().min(l::MAX_ACK_RANGES);let largest=rows[top][1]-1;
        out.byte(f::ACK as u8)?;out.varint(largest)?;out.varint(((now-self.largest_at).max(0.)*1000./8.).round().min(l::MAX_SAFE_INTEGER as f64)as u64)?;
        out.varint(count as u64-1)?;out.varint(largest-rows[top][0])?;
        for i in (top+1-count..top).rev(){out.varint(rows[i+1][0]-rows[i][1]-1)?;out.varint(rows[i][1]-rows[i][0]-1)?;}Ok(())
    }
    // Clear pending ACK deadline state once an outgoing packet includes an ACK frame.
    fn ack_sent(&mut self){self.ack_needed=false;self.ack_deadline=f64::INFINITY;}
    // One bounded batch gathers state, control and the two ordered lanes.
    // Reliable congestion limits never hold a replaceable state datagram.
    pub fn flush(&mut self,now:f64,sink:&mut impl Sink)->Result<()>{
        if self.closed.is_some(){return Ok(());}
        if let Some(info)=self.pending_close.take(){let mut bytes=[0;PLAIN];let mut w=Writer::new(&mut bytes);Self::write_close(info,&mut w)?;let n=w.at;self.emit(&bytes[..n],Meta::default(),self.path,now,sink)?;self.closed=Some(info);self.close_at=now;return Ok(());}
        for guard in 0..256{
            let mut bytes=[0;PLAIN];let mut w=Writer::new(&mut bytes);let mut meta=Meta::default();let ack=guard==0&&!self.received.is_empty()&&(self.ack_needed||now-self.largest_at<2000.);
            if ack{self.ack_frame(now,&mut w)?;}
            while self.control_count>0&&w.remaining()>=9{w.span(&self.control[0])?;self.control.copy_within(1..self.control_count,0);self.control_count-=1;meta.reliable=true;}
            if self.handshake_done && w.remaining() > 0 {
                w.byte(f::HANDSHAKE_DONE as u8)?;
                self.handshake_done = false;
                meta.reliable = true;
            }
            if self.ping&&w.remaining()>0{w.byte(f::PING as u8)?;self.ping=false;meta.reliable=true;}
            while w.remaining()>4{let Some(done)=self.state.frame(&mut w)?else{break;};meta.state+=1;if done{self.stats.datagrams_sent+=1;}}
            let mut pending=0;
            for lane in 0..2{if w.remaining()<=16{break;}let available=self.window.saturating_sub(self.stream_flight+pending);
                if let Some(span)=self.tx[lane].take(w.remaining()-12,available)?{w.byte(f::STREAM as u8)?;w.byte([stream_id::EVENTS,stream_id::BULK][lane]as u8)?;w.varint(span.offset)?;w.varint(span.length as u64)?;self.tx[lane].copy(span,&mut w)?;
                    meta.streams[meta.count]=SentSpan{lane,span};meta.count+=1;meta.reliable=true;pending+=span.length;if span.retransmit{self.stats.retransmitted+=span.length as u64;}}}
            let n=w.at;
            // A full-sized fragment may need an ACK-only packet first. Never
            // truncate its bytes or change its advertised fragment count to fit.
            if !meta.reliable&&meta.state==0{
                if ack&&((self.ack_needed&&now>=self.ack_deadline)||!self.state.is_empty()){self.emit(&bytes[..n],meta,self.path,now,sink)?;self.ack_sent();if !self.state.is_empty(){continue;}}break;
            }
            self.emit(&bytes[..n],meta,self.path,now,sink)?;if ack{self.ack_sent();}
            if self.state.is_empty()&&self.control_count==0&&!self.handshake_done&&!self.tx.iter().any(|t|t.lost_bytes>0||(t.sent<t.written&&self.stream_flight<self.window)){break;}
        }Ok(())
    }
    // Packet numbers never wrap under a key. Sealing writes into one stack
    // datagram; sent metadata retains ranges, not copies of payload ciphertext.
    fn emit(&mut self,bytes:&[u8],mut meta:Meta,path:SocketAddr,now:f64,sink:&mut impl Sink)->Result<()>{
        if bytes.len()>PLAIN{return Err(Error("packet exceeds bounds"));}
        if self.number>=l::MAX_PACKET_NUMBER as u32{self.close(close::PROTOCOL as u64,b"packet numbers exhausted");return Err(Error("packet numbers exhausted"));}
        if self.number>0&&self.number.is_multiple_of(self.key_interval){self.send=self.send.next()?;self.send_phase^=1;}
        let n=self.number;self.number+=1;let mut packet=[0;l::MAX_DATAGRAM];packet[d::TYPE]=if self.send_phase==0{p::DATA}else{p::DATA_PHASE}as u8;
        packet[d::RECEIVER..d::RECEIVER+4].copy_from_slice(&self.remote.to_le_bytes());packet[d::NUMBER..d::NUMBER+4].copy_from_slice(&n.to_le_bytes());
        packet[d::BYTES..d::BYTES+bytes.len()].copy_from_slice(bytes);let (header,tail)=packet.split_at_mut(d::BYTES);let tag=self.send.seal(header,n,&mut tail[..bytes.len()])?;tail[bytes.len()..bytes.len()+l::TAG].copy_from_slice(&tag);
        let slot=n as usize%self.sent.len();if self.sent[slot].active{self.loss(self.sent[slot].number,now)?;}
        if meta.reliable||meta.state>0{meta.active=true;meta.number=n;meta.at=now;self.sent[slot]=meta;if meta.reliable{self.reliable+=1;self.last_reliable=now;}for row in &meta.streams[..meta.count]{self.stream_flight+=row.span.length;}}
        self.floor=self.floor.max(self.number.saturating_sub(l::MAX_SENT_PACKETS as u32));self.trim_sent();self.last_sent=now;self.stats.sent+=1;self.stats.sent_bytes+=(d::BYTES+bytes.len()+l::TAG)as u64;
        sink.packet(&packet[..d::BYTES+bytes.len()+l::TAG],path);Ok(())
    }
    // Short counters exercise key changes in synthetic interoperability
    // fixtures; the device build always uses the generated protocol interval.
    #[cfg(test)]
    pub(crate) fn fixture_key_interval(&mut self,n:u32){self.key_interval=n.max(1);}
}
