//! Borrowed input feedback never materializes a JSON object or a growable
//! result array. Its bounded ids are application receipts, distinct from the
//! protected transport's packet and stream acknowledgements.
use serde::{Deserialize,Deserializer,de::{self,Visitor,SeqAccess}};
use std::fmt;
#[derive(Clone,Copy,Default)]
pub struct Ids{pub values:[u32;32],pub count:usize}
impl<'de> Deserialize<'de> for Ids{
    fn deserialize<D:Deserializer<'de>>(d:D)->Result<Self,D::Error>{
        struct Read;
        impl<'de> Visitor<'de> for Read{type Value=Ids;
            fn expecting(&self,f:&mut fmt::Formatter)->fmt::Result{f.write_str("at most 32 input edge ids")}
            fn visit_seq<A:SeqAccess<'de>>(self,mut seq:A)->Result<Ids,A::Error>{let mut out=Ids::default();
                while let Some(id)=seq.next_element::<u32>()?{if id==0||out.count==32{return Err(de::Error::custom("invalid edge receipt"));}out.values[out.count]=id;out.count+=1;}Ok(out)
            }
        }d.deserialize_seq(Read)
    }
}
#[derive(Default)]
pub struct Results(pub Ids);
impl<'de> Deserialize<'de> for Results{
    fn deserialize<D:Deserializer<'de>>(d:D)->Result<Self,D::Error>{
        #[derive(Deserialize)]struct Outcome<'a>{id:u32,tick:u32,status:&'a str}
        struct Read;
        impl<'de> Visitor<'de> for Read{type Value=Results;
            fn expecting(&self,f:&mut fmt::Formatter)->fmt::Result{f.write_str("at most 32 input edge outcomes")}
            fn visit_seq<A:SeqAccess<'de>>(self,mut seq:A)->Result<Results,A::Error>{let mut out=Ids::default();
                while let Some(result)=seq.next_element::<Outcome>()?{
                    if result.id==0||out.count==32||!matches!(result.status,"consumed"|"expired"|"life-cancelled"|"cleared"|"rejected"){return Err(de::Error::custom("invalid edge outcome"));}
                    let _=result.tick;out.values[out.count]=result.id;out.count+=1;
                }Ok(Results(out))
            }
        }d.deserialize_seq(Read)
    }
}
// EntityEdgeSender receipts may repeat during loss. Retaining a set-shaped
// tail avoids both duplicate retirement work and a packet-time allocation.
impl Ids{
    // Feedback repeats until our ACK arrives. Repeated ids use no extra room;
    // overflow retains the newest bounded tail and can be repaired next turn.
    pub fn add(&mut self,id:u32){if self.values[..self.count].contains(&id){return;}
        if self.count==32{self.values.copy_within(1..32,0);self.count-=1;}
        self.values[self.count]=id;self.count+=1;
    }
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn feedback_is_bounded_and_rejects_unknown_outcomes(){
        let ids:Ids=serde_json::from_str("[1,2,3]").unwrap();assert_eq!(ids.count,3);
        assert!(serde_json::from_str::<Ids>("[0]").is_err());
        assert!(serde_json::from_str::<Results>("[{\"id\":1,\"tick\":4,\"status\":\"unknown\"}]").is_err());
        let mut ids=Ids::default();for id in 1..100{ids.add(id);}assert_eq!(ids.count,32);assert_eq!(ids.values[31],99);
    }
}
