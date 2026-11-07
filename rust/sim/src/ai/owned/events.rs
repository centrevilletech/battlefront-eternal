//! Combat reads the simulation journal at its original ingest point. Other
//! event kinds still occupy ring slots, so overflow cannot grant AI a longer
//! hearing history than presentation and the source game's other readers.
use crate::ai::combat::{binding::{CombatAi,Event},world::World};
use std::{cell::RefCell,rc::Rc};

#[derive(Clone,Copy,Debug,PartialEq)]
pub enum Record {Combat(Event),Other}
struct State {ring:Vec<Option<Record>>,seq:u64}
#[derive(Clone)]
pub struct Journal(Rc<RefCell<State>>);
#[derive(Default)]
pub struct Reader {pub cursor:u64,pub dropped:u64}
impl Journal {
    pub fn new(capacity:usize)->Self {
        assert!(capacity>0,"nonempty simulation journal");
        Self(Rc::new(RefCell::new(State {ring:vec![None;capacity],seq:0})))
    }
    pub fn reader(&self)->Reader {Reader {cursor:self.0.borrow().seq,dropped:0}}
    pub fn emit(&self,record:Record) {
        let mut s=self.0.borrow_mut();let index=s.seq%s.ring.len() as u64;
        s.ring[index as usize]=Some(record);s.seq+=1;
    }
    /// Import the sparse semantic records for a completed outer boundary.
    /// Unrelated records still replace their ring slots and spend capacity;
    /// a delayed AI reader cannot recover an overwritten shot from them.
    pub fn import(&self,head:u64,records:impl IntoIterator<Item=(u64,Record)>) {
        let mut s=self.0.borrow_mut();let capacity=s.ring.len() as u64;
        if head<s.seq {s.ring.fill(None);}
        let start=if head<s.seq {0}else{s.seq};
        for sequence in start.max(head.saturating_sub(capacity))..head {
            s.ring[(sequence%capacity) as usize]=Some(Record::Other);
        }
        s.seq=head;
        for (sequence,record) in records {
            assert!(sequence<head&&head-sequence<=capacity,"journal record inside retained sequence");
            s.ring[(sequence%capacity) as usize]=Some(record);
        }
    }
    pub fn sequence(&self)->u64 {self.0.borrow().seq}
    pub fn capacity(&self)->usize {self.0.borrow().ring.len()}
    /// SimEvents.reset keeps each existing reader's cursor. It only resets
    /// the journal; the next battle installs its own reader at construction.
    pub fn reset(&self) {let mut s=self.0.borrow_mut();s.ring.fill(None);s.seq=0;}
}
impl Reader {
    pub fn read(&mut self,journal:&Journal,mut visit:impl FnMut(Record)) {
        {
            let s=journal.0.borrow();let capacity=s.ring.len() as u64;
            if s.seq.saturating_sub(self.cursor)>capacity {
                self.dropped+=s.seq-self.cursor-capacity;self.cursor=s.seq-capacity;
            }
        }
        loop {
            let record={
                let s=journal.0.borrow();
                if self.cursor>=s.seq {break;}
                s.ring[(self.cursor%s.ring.len() as u64) as usize]
            };
            self.cursor+=1;
            // Release the journal borrow before delivery. A synchronous
            // callback can emit enough records to replace this ring slot.
            if let Some(record)=record {visit(record);}
        }
    }
    pub fn ingest(&mut self,journal:&Journal,combat:&mut CombatAi,world:&mut impl World) {
        self.read(journal,|record|if let Record::Combat(event)=record {
            combat.ingest(world,std::slice::from_ref(&event));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::combat::binding::EventRef;
    fn shot(id:u64)->Record {Record::Combat(Event::Shot {source:EventRef::Unit(id),target:EventRef::None})}
    #[test]
    fn unrelated_events_consume_capacity_and_readers_keep_independent_cursors() {
        let j=Journal::new(3);let mut a=j.reader();j.emit(shot(1));let mut b=j.reader();
        j.emit(Record::Other);j.emit(shot(2));j.emit(Record::Other);j.emit(shot(3));
        let mut seen=Vec::new();a.read(&j,|e|seen.push(e));
        assert_eq!(seen,vec![shot(2),Record::Other,shot(3)]);assert_eq!(a.dropped,2);
        seen.clear();b.read(&j,|e|seen.push(e));assert_eq!(seen,vec![shot(2),Record::Other,shot(3)]);
        assert_eq!(b.dropped,1);assert_eq!(a.cursor,5);assert_eq!(b.cursor,5);
    }
    #[test]
    fn nested_emissions_extend_the_read_and_overflow_is_checked_only_on_entry() {
        let j=Journal::new(2);let mut r=j.reader();j.emit(shot(1));
        let nested=j.clone();let mut seen=Vec::new();
        r.read(&j,|e|{
            seen.push(e);
            if seen.len()==1 {nested.emit(shot(2));nested.emit(shot(3));nested.emit(shot(4));}
        });
        // The overwritten slot is read as it now stands, just like the JS
        // ring. Rechecking overflow after delivery would skip this record.
        assert_eq!(seen,vec![shot(1),shot(4),shot(3),shot(4)]);
        assert_eq!(r.cursor,4);assert_eq!(r.dropped,0);
        j.reset();j.emit(shot(5));let mut count=0;r.read(&j,|_|count+=1);
        assert_eq!(count,0);assert_eq!(r.cursor,4);assert_eq!(r.dropped,0);
        let mut fresh=j.reader();j.emit(shot(6));fresh.read(&j,|_|count+=1);assert_eq!(count,1);
    }
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Invalid {Sequence,Order,Window}
fn sequence(value:f64)->Result<u64,Invalid> {
    if value.is_finite()&&value>=0.&&value<=9007199254740991.&&value.fract()==0. {Ok(value as u64)}else{Err(Invalid::Sequence)}
}
impl super::state::State {
    /// Resolve event identities once at the phase boundary. Payloads never
    /// carry body facts or decide whether a current target is eligible; the
    /// combat reader makes those observations at its source ingest slot.
    pub fn import_journal(&self,input:&crate::gen::AiJournalInput)->Result<(),Invalid> {
        let head=sequence(input.sequence)?;let capacity=self.journal.capacity() as u64;
        let mut previous=None;
        for event in &input.records {
            let at=sequence(event.sequence)?;
            if previous.is_some_and(|last|last>=at) {return Err(Invalid::Order);}
            if at>=head||head-at>capacity {return Err(Invalid::Window);}
            previous=Some(at);
        }
        use crate::ai::combat::{binding::{EventRef,Event},world::Ref};
        let reference=|value:&crate::gen::AiJournalRef| {
            let object=value.member.and_then(|member|self.queries.object(&self.entities,member));
            match object {
                None=>EventRef::None,
                Some(object) if value.direct=>EventRef::Unit(object),
                Some(object)=>EventRef::Ref(Ref {obj:object,generation:value.generation.unwrap_or(f64::NAN)}),
            }
        };
        self.journal.import(head,input.records.iter().map(|event| {
            let record=match event.kind {
                crate::gen::AiJournalKind::Shot=>Event::Shot {source:reference(&event.source),target:reference(&event.target)},
                crate::gen::AiJournalKind::Damage=>Event::Damage {target:reference(&event.target),attacker:reference(&event.attacker),
                    attacker_character:event.attacker_character.and_then(|character|self.entities.find(super::entities::Entity::Character(character))),
                    amount:event.amount,generic:event.generic,typed:event.typed,known:event.known,
                    point:event.point.map(|point|three::Vector3::new(point[0],point[1],point[2]))},
            };
            (event.sequence as u64,Record::Combat(record))
        }));
        Ok(())
    }
}
