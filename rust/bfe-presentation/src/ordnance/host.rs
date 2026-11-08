//! Raw host facts become canonical ordnance rows in the native frame. The
//! browser retains resource identity only; flags and keys never come from JS.
use crate::ordnance_layout::{ordnance_host as h,ordnance_record as r,
    ordnance_frame as f,ordnance_limits as limits,ordnance_kind as k,ordnance_flag as flag};
use super::Ordnance;

pub struct Host {
    pub input:Vec<f64>,
    pub records:usize,
    pub net_count:usize,
    serial:f64,
    pub classes:Vec<Class>,
    batches:i32,
    pub dropped:u64,
    reported:bool,
    slots:Vec<bool>,
    free:Vec<usize>,
}
pub struct Class {
    content:super::style::Content,
    pub row:[f64;crate::ordnance_layout::ordnance_style::STRIDE],
    loaded:[bool;4],look:bool,
}
impl Host {
    pub fn new()->Self{Self{input:vec![0.0;f::STRIDE+limits::RECORDS*h::STRIDE],records:0,net_count:0,serial:0.0,classes:Vec::with_capacity(limits::STYLES),batches:0,dropped:0,reported:false,
        slots:vec![false;limits::NET_SLOTS],free:(0..limits::NET_SLOTS).rev().collect()}}
    // Source identity is assigned at resource admission, never in a frame.
    pub fn token(&mut self,slot:f64)->f64{if slot>=0.0{0.0}else{self.serial+=1.0;(1u64<<40)as f64+self.serial}}
    pub fn batch(&mut self)->i32{if self.batches<limits::HANDLES as i32{let id=self.batches;self.batches+=1;id}else{-1}}
    pub fn allocate(&mut self)->i32{if let Some(slot)=self.free.pop(){self.slots[slot]=true;slot as i32}else{-1}}
    pub fn release(&mut self,slot:usize){if let Some(used)=self.slots.get_mut(slot){if *used{*used=false;self.free.push(slot);}}}
}
fn present(n:f64)->bool{!n.is_nan()}
fn absent(n:f64,default:f64)->f64{if n.is_nan(){default}else{n}}
impl Ordnance {
    // PageOrdnance.bolt/round/grenade/beam record assembly. Preserve the old
    // optional-field semantics while keeping all policy out of the host packer.
    pub fn host_begin(&mut self,bolts:bool,sniper:bool){
        self.host.records=0;self.host.input[f::VERSION]=crate::ordnance_layout::VERSION as f64;
        self.host.input[f::ENHANCED_BOLTS]=bolts as u8 as f64;self.host.input[f::ENHANCED_SNIPER]=sniper as u8 as f64;
    }
    pub fn host_record(&mut self)->i32{if self.host.records>=limits::RECORDS{self.host.dropped+=1;-1}else{let index=self.host.records;self.host.records+=1;index as i32}}
    pub fn host_net_row(&mut self)->usize{
        if self.host.net_count>=limits::NET_COMMANDS{self.net_flush(self.host.net_count);self.host.net_count=0;}
        let at=self.host.net_count*crate::ordnance_layout::net_command::STRIDE;self.host.net_count+=1;at
    }
    pub fn prepare_host(&mut self){
        let raw=&self.host.input;
        self.input[..f::STRIDE].copy_from_slice(&raw[..f::STRIDE]);
        let count=super::clamp_count(raw[f::RECORDS],limits::RECORDS);
        self.input[f::RECORDS]=count as f64;
        let mut kept=0;
        for index in 0..count {
            let a=&raw[f::STRIDE+index*h::STRIDE..f::STRIDE+(index+1)*h::STRIDE];
            let kind=a[h::KIND] as u32;
            if a[h::STYLE]<0.0&&!matches!(kind,k::GRENADE|k::NET){self.host.dropped+=1;continue;}
            let b=&mut self.input[f::STRIDE+kept*r::STRIDE..f::STRIDE+(kept+1)*r::STRIDE];kept+=1;b.fill(0.0);
            let mut bits=0u32;
            for (raw,flag) in [(h::PREVIOUS_FLOWN,flag::HAS_PREVIOUS_FLOWN),(h::PREVIOUS_DRAWN,flag::HAS_PREVIOUS_DRAWN),
                (h::OFFSET,flag::HAS_OFFSET),(h::FLIGHT_TICK,flag::LINEAR),(h::CLOCK_TICK,flag::HAS_CLOCK),
                (h::LAUNCH,flag::HAS_LAUNCH),(h::MUZZLE_OFFSET,flag::HAS_MUZZLE_OFFSET),
                (h::QUATERNION,flag::HAS_QUATERNION),(h::VELOCITY,flag::HAS_VELOCITY)] {
                if present(a[raw]){bits|=1<<flag;}
            }
            if a[h::HAS_MESH]!=0.0{bits|=1<<flag::HAS_MESH;}if a[h::HAS_TRAIL]!=0.0{bits|=1<<flag::HAS_TRAIL;}
            if present(a[h::MUZZLE]) {
                if kind==k::BOLT{bits|=1<<(a[h::RESOLVE] as u32).min(31);}
                else if kind==k::ROUND&&present(a[h::LAUNCH]){bits|=1<<flag::RESOLVE_ROUND;}
            }
            b[r::KIND]=kind as f64;b[r::STYLE]=a[h::STYLE];b[r::SOURCE]=a[h::SOURCE];b[r::FLAGS]=bits as f64;
            b[r::KEY]=if a[h::SIM_SLOT]>=0.0{a[h::SIM_SLOT]+(a[h::SIM_GENERATION]+1.0)*(1u64<<20)as f64}else{a[h::KEY]};
            for (raw,out) in [(h::ALPHA,r::ALPHA),(h::FLOWN,r::FLOWN),(h::DRAWN,r::DRAWN),(h::SPEED,r::SPEED),(h::LIFE,r::LIFE),(h::MERGE,r::MERGE)]{b[out]=a[raw];}
            for (raw,out) in [(h::CLOCK_TICK,r::CLOCK_TICK),(h::PREVIOUS_FLOWN,r::PREVIOUS_FLOWN),(h::PREVIOUS_DRAWN,r::PREVIOUS_DRAWN),(h::FLIGHT_TICK,r::FLIGHT_TICK)]{b[out]=absent(a[raw],0.0);}
            b[r::ENDED]=if present(a[h::ENDED])&&a[h::ENDED]!=0.0{1.0}else{0.0};b[r::FADE]=absent(a[h::FADE],1.0);
            for (raw,out,n) in [(h::POSITION,r::POSITION,3),(h::PREVIOUS,r::PREVIOUS,3),(h::DIRECTION,r::DIRECTION,3),(h::END,r::END,3),
                (h::VELOCITY,r::VELOCITY,3),(h::QUATERNION,r::QUATERNION,4),(h::LAUNCH,r::LAUNCH,3),(h::OFFSET,r::OFFSET,3),(h::MUZZLE,r::MUZZLE,3)] {
                if present(a[raw]){b[out..out+n].copy_from_slice(&a[raw..raw+n]);}
            }
            if kind==k::ROUND&&present(a[h::MUZZLE_OFFSET]){b[r::OFFSET..r::OFFSET+3].copy_from_slice(&a[h::MUZZLE_OFFSET..h::MUZZLE_OFFSET+3]);}
        }
        self.input[f::RECORDS]=kept as f64;
    }
    pub fn present_host(&mut self){if self.host.input[f::VERSION]==crate::ordnance_layout::VERSION as f64{self.host.input[f::RECORDS]=self.host.records as f64;self.host.input[f::NET_COMMANDS]=self.host.net_count as f64;self.prepare_host();self.host.net_count=0;}self.present();}
}

impl Ordnance {
    // The host only supplies authored class facts and resource readiness.
    // Native admission owns the bound, role transitions and compiled row.
    pub fn host_class(&mut self,text:&str)->Result<i32,String>{
        if self.host.classes.len()>=limits::STYLES {self.host.dropped+=1;return Ok(-1);}
        let content:super::style::Content=serde_json::from_str(text).map_err(|e|e.to_string())?;
        let row=super::style::row(&content,false);let id=self.host.classes.len();
        self.host.classes.push(Class{content,row,loaded:[false;4],look:false});self.sync_host_style(id);Ok(id as i32)
    }
    pub fn sync_host_style(&mut self,index:usize){
        if let Some(class)=self.host.classes.get(index){self.style_input.copy_from_slice(&class.row);self.set_style(index);}
    }
    pub fn host_style_request(&mut self,index:usize,role:u32,enhanced:bool)->String{
        let Some(class)=self.host.classes.get_mut(index) else{return String::from("null")};
        let tag=if enhanced{3}else{(role as usize).min(2)};
        if role==0||class.loaded[tag]{return String::from("null");}class.loaded[tag]=true;
        if enhanced&&!class.look {
            class.look=true;let previous=class.row;class.row=super::style::row(&class.content,true);
            class.row[..3].copy_from_slice(&previous[..3]);
        }
        let plan=super::style::resources(&class.content,role,enhanced).to_string();self.sync_host_style(index);plan
    }
    pub fn host_style_batches(&mut self,index:usize,core:f64,glow:f64,beam:f64){
        if let Some(class)=self.host.classes.get_mut(index){class.row[..3].copy_from_slice(&[core,glow,beam]);self.sync_host_style(index);}
    }
    pub fn host_style_pixels(&mut self,index:usize,pixels:&[u8]){
        let hue=super::style::texture_hue(pixels);
        if let Some(class)=self.host.classes.get_mut(index){
            if !hue.is_empty(){let color=super::style::vivid(&hue,1.6);let at=crate::ordnance_layout::ordnance_style::LOOK_COLOR;class.row[at..at+3].copy_from_slice(&color);self.sync_host_style(index);}
        }
    }
    pub fn host_report(&mut self)->bool{
        use crate::ordnance_layout::ordnance_header as h;
        if self.host.reported{return false;}
        let lost=self.host.dropped as f64+[h::DROPPED_QUADS,h::DROPPED_BLASTERS,h::DROPPED_CALLS,h::REFUSED_VIEWS,h::REFUSED_NET,h::SKIPPED_NET].iter().map(|i|self.header[*i]).sum::<f64>();
        if lost==0.0{return false;}self.host.reported=true;true
    }
}

impl Ordnance {
    pub fn host_net_sample(&mut self,at:usize,slot:usize,style:i32,flags:u32,text:&str)->Result<(),String>{
        use crate::ordnance_layout::{net_command as n,net_kind,net_type};
        if at%n::STRIDE!=0||at.checked_add(n::STRIDE).is_none_or(|end|end>self.net_input.len()){self.host.dropped+=1;return Ok(());}
        let e:serde_json::Value=serde_json::from_str(text).map_err(|e|e.to_string())?;
        let row=&mut self.net_input[at..at+n::STRIDE];row.fill(0.0);
        row[n::KIND]=net_kind::SAMPLE as f64;row[n::SLOT]=slot.min(limits::NET_SLOTS) as f64;
        row[n::STYLE]=style as f64;row[n::FLAGS]=flags as f64;
        row[n::TYPE]=match e["type"].as_str(){Some("beam")=>net_type::BEAM,Some("round")=>net_type::ROUND,_=>net_type::GRENADE} as f64;
        for(key,out)in [("generation",n::GENERATION),("tick",n::TICK),("expiryTick",n::EXPIRY_TICK),("life",n::LIFE)]{row[out]=e[key].as_f64().unwrap_or(0.0);}
        for(key,out,count)in [("position",n::POSITION,3),("velocity",n::VELOCITY,3),("quaternion",n::QUATERNION,4),("end",n::END,3)]{
            for i in 0..count{row[out+i]=e[key][i].as_f64().unwrap_or(0.0);}
        }Ok(())
    }
}
