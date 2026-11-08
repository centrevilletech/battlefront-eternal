//! Combat record decoding, validation and retention metadata. Payloads remain
//! owned transport bytes; no host decoder repeats the numeric policy.
use serde::{Deserialize, Deserializer};
use serde_json::{json, Value};
type Result<T> = std::result::Result<T,String>;
struct Reader<'a>{bytes:&'a [u8],at:usize}
impl<'a> Reader<'a>{
    fn bytes<const N:usize>(&mut self)->Result<[u8;N]>{
        let end=self.at+N;let slice=self.bytes.get(self.at..end).ok_or("invalid combat payload")?;
        self.at=end;Ok(slice.try_into().unwrap())
    }
    fn u(&mut self)->Result<f64>{Ok(u32::from_le_bytes(self.bytes()?) as f64)}
    fn i(&mut self)->Result<f64>{Ok(i32::from_le_bytes(self.bytes()?) as f64)}
    fn byte(&mut self)->Result<u8>{Ok(self.bytes::<1>()?[0])}
    fn f(&mut self,error:&str)->Result<f64>{let n=f64::from_le_bytes(self.bytes()?);if !n.is_finite(){Err(error.into())}else{Ok(n)}}
    fn small(&mut self)->Result<f64>{let n=f32::from_le_bytes(self.bytes()?) as f64;if !n.is_finite()||n.abs()>1e8{Err("invalid projectile sample".into())}else{Ok(n)}}
    fn vector(&mut self,error:&str)->Result<[f64;3]>{Ok([self.f(error)?,self.f(error)?,self.f(error)?])}
}
// Root field order controls validation errors. Keep it independent of the
// map representation used for nested JSON values in other owners.
struct Fields(Vec<(String,Value)>);
impl<'de> Deserialize<'de> for Fields {
    fn deserialize<D:Deserializer<'de>>(d:D)->std::result::Result<Self,D::Error>{
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value=Fields;
            fn expecting(&self,f:&mut std::fmt::Formatter)->std::fmt::Result{f.write_str("presentation event")}
            fn visit_map<M:serde::de::MapAccess<'de>>(self,mut m:M)->std::result::Result<Fields,M::Error>{
                let mut fields=Vec::new();while let Some(pair)=m.next_entry()?{fields.push(pair);}Ok(Fields(fields))
            }
        }
        d.deserialize_map(V)
    }
}
fn vector(v:&Value)->bool{v.as_array().is_some_and(|v|v.len()==3&&v.iter().all(|n|n.as_f64().is_some_and(f64::is_finite)))}
const KINDS:&[&str]=&["weaponAction","endWeaponAction","shot","remoteShot","shieldHit","deathBlast","blowApart","remoteBlast","effect","lightning","vo","bounce","pickup","serve","towAttach","towRelease","squad"];
// JSON text remains opaque on output. Replace escaped surrogate units only
// in the validation copy, preserving their UTF-16 lengths without requiring
// an unpaired surrogate to become a Rust UTF-8 scalar.
fn validation_text(bytes:&[u8])->Vec<u8>{
    let mut out=bytes.to_vec();let mut at=0;
    while at+1<out.len(){
        if out[at]!=b'\\'{at+=1;continue;}
        if out[at+1]==b'u'&&at+6<=out.len(){
            if let Ok(text)=std::str::from_utf8(&out[at+2..at+6]){
                if u16::from_str_radix(text,16).is_ok_and(|n|(0xd800..=0xdfff).contains(&n)){out[at+2..at+6].copy_from_slice(b"e000");}
            }
            at+=6;
        }else{at+=2;}
    }out
}
pub fn validate_event(bytes:&[u8])->Result<()> {
    let fields:Fields=serde_json::from_slice(&validation_text(bytes)).map_err(|e|e.to_string())?;
    let kind=fields.0.iter().find(|(k,_)|k=="kind").and_then(|(_,v)|v.as_str());
    if !kind.is_some_and(|k|KINDS.contains(&k)){return Err("Invalid presentation kind".into());}
    for(key,v)in fields.0 {
        let error=match key.as_str(){
            "kind"|"name"|"action"|"bank"|"posture"|"hitKind"|"line"|"reason"|"label"|"property"=>
                (!v.is_null()&&!v.as_str().is_some_and(|s|s.encode_utf16().count()<=128)).then_some("Invalid presentation text"),
            "position"|"normal"|"from"|"to"|"source"=>(!v.is_null()&&!vector(&v)).then_some("Invalid presentation vector"),
            "channel"=>(v.as_f64()!=Some(0.0)&&v.as_f64()!=Some(1.0)).then_some("Invalid presentation channel"),
            "ownerId"|"ownerGeneration"|"mountId"|"mountGeneration"|"seat"|"fireId"|"definitionId"|"weaponId"|"building"|"team"=>
                (!v.as_f64().is_some_and(|n|n.is_finite()&&n.fract()==0.0&&(0.0..=4294967295.0).contains(&n))).then_some("Invalid presentation identity"),
            "amount"|"charge"|"swing"=>(!v.as_f64().is_some_and(f64::is_finite)).then_some("Invalid presentation number"),
            "end"|"prepare"|"repair"|"human"|"melee"|"positional"=>(!v.is_boolean()).then_some("Invalid presentation flag"),
            _=>return Err(format!("Unknown presentation field: {key}")),
        };if let Some(e)=error{return Err(e.into());}
    }Ok(())
}
pub fn decode(kind:&str,schema:f64,tick:f64,bytes:&[u8])->Result<String>{
    if schema==8.0 {
        if kind!="combat"||bytes.len()>900{return Err("Invalid presentation payload".into());}
        validate_event(bytes)?;return Ok(json!({"expiryTick":tick+120.0}).to_string());
    }
    if !schema.is_finite()||schema.fract()!=0.0{return Err("invalid combat payload".into());}
    let schema=schema as u32;
    let expected=match schema{1=>130,2=>82,3|4=>78,5=>33,6=>82,7=>4,_=>0};
    let error=match schema{3|4=>"invalid feedback payload",5=>"invalid explosion record",6|7=>"invalid projectile payload",_=>"invalid combat payload"};
    if expected==0||bytes.len()!=expected||(!matches!(schema,3|4)&&kind!="combat"){return Err(error.into());}
    let mut r=Reader{bytes,at:0};let expiry=r.u()?;
    let mut out=json!({"expiryTick":expiry});
    match schema {
        1|2=>{
            out["definitionId"]=json!(r.u()?);out["ownerId"]=json!(r.u()?);out["ownerGeneration"]=json!(r.u()?);out["fireId"]=json!(r.u()?);
            let launch=r.f("nonfinite combat payload")?;out["launchTick"]=json!(launch);
            out["position"]=json!(r.vector("nonfinite combat payload")?);out["direction"]=json!(r.vector("nonfinite combat payload")?);
            let surface=r.byte()?;out["surface"]=json!(surface);
            let(mut speed,mut life,mut merge)=(0.0,0.0,0.0);
            if schema==1 {out["offset"]=json!(r.vector("nonfinite combat payload")?);speed=r.f("nonfinite combat payload")?;life=r.f("nonfinite combat payload")?;merge=r.f("nonfinite combat payload")?;out["speed"]=json!(speed);out["life"]=json!(life);out["merge"]=json!(merge);}
            let execution=r.u()?;let channel=r.byte()?;out["executionTick"]=json!(execution);out["channel"]=json!(channel);
            if channel>1||launch<0.0||execution<launch||execution>tick{return Err("invalid combat execution tick".into());}
            if launch>tick||expiry<tick||expiry-tick>7320.0||surface>5||schema==1&&(life<0.0||life>120.0||speed<0.0||merge<=0.0){return Err("invalid combat lifetime".into());}
        }
        3|4=>{
            for key in ["targetId","targetGeneration","attackerId","attackerGeneration"]{out[key]=json!(r.u()?);}
            out["position"]=json!(r.vector("invalid feedback position")?);out["team"]=json!(r.i()?);out["attackerTeam"]=json!(r.i()?);
            let flags=u16::from_le_bytes(r.bytes()?);if flags>1{return Err("invalid feedback flags".into());}
            let source=r.vector("invalid feedback source")?;out["source"]=if flags==1{json!(source)}else{Value::Null};
            if expiry!=tick+18.0{return Err("invalid feedback expiry".into());}
        }
        5=>{
            out["definitionId"]=json!(r.u()?);out["position"]=json!(r.vector("invalid explosion position")?);let wet=r.byte()?;
            if expiry!=tick+18.0||wet>1{return Err("invalid explosion expiry or water flag".into());}out["wet"]=json!(wet!=0);
        }
        6|7=>{
            if expiry!=tick+18.0{return Err("invalid projectile expiry".into());}
            if schema==6 {
                out["definitionId"]=json!(r.u()?);let typ=r.byte()?;
                for key in ["ownerId","ownerGeneration","fireId","launchTick"]{out[key]=json!(r.u()?);}
                for(key,n)in [("position",3),("velocity",3),("quaternion",4),("end",3)]{
                    let mut values=Vec::with_capacity(n);for _ in 0..n{values.push(r.small()?);}out[key]=json!(values);
                }
                let life=r.small()?;let channel=r.byte()?;out["life"]=json!(life);out["channel"]=json!(channel);
                let q=out["quaternion"].as_array().unwrap();let length=v8math::hypot_n(&q.iter().map(|n|n.as_f64().unwrap()).collect::<Vec<_>>());
                if channel>1||out["launchTick"].as_f64().unwrap()>tick||typ>2||life<0.0||(length-1.0).abs()>0.01{return Err("invalid projectile type/orientation".into());}
                out["type"]=json!(["grenade","round","beam"][typ as usize]);
            }
        }
        _=>{}
    }Ok(out.to_string())
}
