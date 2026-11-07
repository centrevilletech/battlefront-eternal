//! Page receive and outbound phases share the display memory. Baselines and
//! sampled records remain native through the body and hull phases.
use crate::network_layout::net_binding as link;
pub mod input;
pub mod mounts;
pub mod preview;
pub mod outbound;
pub mod read;
pub type Result<T> = std::result::Result<T, &'static str>;
pub const EXCHANGE: &str = "invalid page networking exchange";
pub const ROOM: usize = crate::network_layout::net_limits::EXCHANGE;

// Wire sections mix counted scalars with word-aligned packed byte spans. The
// surrounding header, operations and pose rows come from page-net.mjs.
// Counted streams borrow only the reserved input range. Every crossing count
// is checked before it can become a slice length or a native table index.
pub struct Words<'a> { pub values: &'a [f64], pub at: usize, }
impl<'a> Words<'a> {
    pub fn new(values: &'a [f64]) -> Self { Self { values, at: 0 } }
    pub fn number(&mut self) -> Result<f64> { let n = *self.values.get(self.at).ok_or(EXCHANGE)?;
        self.at += 1;
        Ok(n) }
    pub fn uint(&mut self, maximum: usize) -> Result<usize> { let n = self.number()?;
        if !n.is_finite() || n < 0. || n.fract() != 0. || n > maximum as f64 { return Err(EXCHANGE); }
        Ok(n as usize) }
    // Checked addition covers overflow independently of the window bound.
    // An invalid span never advances the reader into another region.
    pub fn span(&mut self, count: usize) -> Result<&'a [f64]> { let end = self.at.checked_add(count).ok_or(EXCHANGE)?;
        let out = self.values.get(self.at..end).ok_or(EXCHANGE)?;
        self.at = end;
        Ok(out) }
    // Bytes are packed in aligned whole words, not widened to f64 values.
    // Check the logical byte count and physical span before borrowing a byte
    // view. All f64 bit patterns are valid; padding never reaches a reader.
    pub fn bytes(&mut self) -> Result<&'a [u8]> { let n = self.uint(65535)?;
        let words = self.span(n.div_ceil(std::mem::size_of::<f64>()))?;
        Ok(unsafe { std::slice::from_raw_parts(words.as_ptr().cast::<u8>(), n) }) }
    pub fn optional_bytes(&mut self) -> Result<Option<&'a [u8]>> { if self.uint(1)? == 0 { Ok(None) } else {
            Ok(Some(self.bytes()?)) } }
    pub fn finish(&self) -> Result<()> { if self.at == self.values.len() { Ok(()) } else { Err(EXCHANGE) } } }
// Output has no growth operation. The largest protocol-valid phase fits the
// window reserved during setup; malformed expansion returns an ordinary fault.
pub struct Output<'a> { pub values: &'a mut [f64], pub at: usize, }
impl<'a> Output<'a> {
    pub fn push(&mut self, n: f64) -> Result<()> {
        let dest = self.values.get_mut(self.at).ok_or("page networking output exceeds bounds")?;
        *dest = n;
        self.at += 1;
        Ok(()) }
    // Copy a complete result span atomically with respect to its bound.
    // No page code runs while these words are borrowed.
    pub fn extend(&mut self, values: &[f64]) -> Result<()> {
        let end = self.at.checked_add(values.len()).ok_or(EXCHANGE)?;
        self.values.get_mut(self.at..end).ok_or("page networking output exceeds bounds")?.copy_from_slice(values);
        self.at = end;
        Ok(()) }
    // Packed owned payloads keep opaque bytes out of the scalar read loop.
    // Reserve complete words and return only the checked logical byte range.
    pub fn packed(&mut self, n: usize) -> Result<&mut [u8]> { let words = n.div_ceil(8);
        let end = self.at.checked_add(words).ok_or(EXCHANGE)?;
        let span = self.values.get_mut(self.at..end).ok_or("page networking output exceeds bounds")?;
        self.at = end;
        Ok(unsafe { std::slice::from_raw_parts_mut(span.as_mut_ptr().cast::<u8>(), n) }) } }

pub use read::session::Session;
use std::{cell::RefCell, rc::Rc};
/// A cold link keeps display consumers beside the network ring. It never
/// points into a caller-owned buffer or invokes a JavaScript sample reader.
pub type Shared = Rc<RefCell<Session>>;

#[derive(Clone, Copy)]
struct Binding { id:f64, generation:f64, kind:f64, slot:Option<usize>, lease:u64 }
// SnapshotPresentation.update used to retire a binding on the first sampled
// life/class mismatch. Keep that decision native, but publish one cold-read
// liveness flag so a subsequent semantic packet can admit the binding again.
pub(crate) struct LinkTable { rows:Vec<Option<Binding>>, states:Vec<f64> }
impl LinkTable {
    pub(crate) fn update(&mut self,owner:&Session){
        use bfe_pose::timeline_layout::timeline_output as o;
        for key in 0..self.rows.len(){let Some(b)=&mut self.rows[key]else{continue;};
            if b.lease!=owner.lease{self.states[key*link::STRIDE+link::ACTIVE]=0.;continue;}
            if self.states[key*link::STRIDE+link::ACTIVE]==0.||owner.excluded.contains(&b.id){continue;}
            // A newly admitted resource can precede the first committed pose.
            // Resolve that stable slot once; absence does not retire a link.
            if b.slot.is_some_and(|slot|owner.timeline.output[slot*o::STRIDE+o::ID]!=b.id){b.slot=None;}
            if b.slot.is_none(){b.slot=owner.timeline.slot(b.id);}
            let Some(slot)=b.slot else{continue;};let row=&owner.timeline.output[slot*o::STRIDE..][..o::STRIDE];
            if row[o::VALID]!=0.&&row[o::ID]==b.id&&(row[o::GENERATION]!=b.generation||row[o::KIND]!=b.kind){self.states[key*link::STRIDE+link::ACTIVE]=0.;}
        }
    }
}
pub struct Bindings { owner:Option<Shared>, table:Rc<RefCell<LinkTable>> }
impl Default for Bindings {
    fn default()->Self{let capacity=crate::layout::unit_limits::ACTORS;Self{owner:None,table:Rc::new(RefCell::new(LinkTable{rows:Vec::with_capacity(capacity),states:vec![0.;capacity*link::STRIDE]}))}}
}
impl Bindings {
    // Both vectors reserve their full resource capacity at owner creation.
    // Link admission therefore remains allocation-free even on a new packet;
    // weak owner registration never keeps a discarded display kernel alive.
    pub fn bind(&mut self,key:usize,id:f64,generation:f64,kind:f64,owner:&Shared){
        if key>=crate::layout::unit_limits::ACTORS{return;}
        if self.owner.as_ref().is_none_or(|o|!Rc::ptr_eq(o,owner)){
            // A resource owner can move to another receiver. Stop the old
            // receiver validating this table before admitting its new links.
            if let Some(previous)=self.owner.take(){previous.borrow_mut().unlink(&self.table);}
            {let mut table=self.table.borrow_mut();table.rows.fill(None);table.states.fill(0.);}
            if !owner.borrow_mut().link(&self.table){return;}
            self.owner=Some(owner.clone());
        }
        let mut table=self.table.borrow_mut();if table.rows.len()<=key{table.rows.resize(key+1,None);}
        table.rows[key]=Some(Binding{id,generation,kind,slot:owner.borrow().timeline.slot(id),lease:owner.borrow().lease});table.states[key*link::STRIDE+link::ACTIVE]=1.;
    }
    // Resource removal clears only this key. The native frame never walks a
    // JavaScript binding map or constructs a per-entity sampled-pose facade.
    pub fn unbind(&mut self,key:usize){let mut table=self.table.borrow_mut();if let Some(row)=table.rows.get_mut(key){*row=None;table.states[key*link::STRIDE+link::ACTIVE]=0.;}}
    pub fn states_ptr(&self)->*const f64{self.table.borrow().states.as_ptr()}
    pub fn states_len(&self)->usize{self.table.borrow().states.len()}
    fn sample<'a>(table:&LinkTable,key:usize,owner:&'a Session)->Option<&'a[f64]>{
        use bfe_pose::timeline_layout::timeline_output as o;
        let b=table.rows.get(key)?.as_ref()?;
        if table.states[key*link::STRIDE+link::ACTIVE]==0.||b.lease!=owner.lease||owner.excluded.contains(&b.id){return None;}
        let slot=b.slot?;let row=&owner.timeline.output[slot*o::STRIDE..][..o::STRIDE];
        if row[o::VALID]==0.||row[o::ID]!=b.id||row[o::GENERATION]!=b.generation||row[o::KIND]!=b.kind{return None;}Some(row)
    }
    // PageUnits.write supplies current collision facts and resource handles.
    // Replace historical fields inside the native phase, after that transport
    // and before pose sampling. No sampled value is written back through JS.
    pub fn units(&mut self, input: &mut [f64]) { use crate::layout::{unit_frame as f, unit_input as i};
        use bfe_pose::timeline_layout::timeline_output as o;
        let Some(owner) = self.owner.as_ref() else { return; };
        let owner = owner.borrow();
        let table=self.table.borrow();
        let count = (input[f::ACTORS] as usize).min((input.len().saturating_sub(f::STRIDE)) / i::STRIDE);
        for at in 0..count { let row = &mut input[f::STRIDE + at * i::STRIDE..][..i::STRIDE];
            let Some(p) = Self::sample(&table, row[i::ACTOR] as usize, &owner) else { continue; };
            if row[i::ID] != p[o::ID] || row[i::GENERATION] != p[o::GENERATION] || row[i::SEAT] != 0. { continue; }
            row[i::NETWORK] = 1.;
            row[i::POSITION..i::POSITION + 3].copy_from_slice(&p[o::POSITION..o::POSITION + 3]);
            row[i::VELOCITY..i::VELOCITY + 3].copy_from_slice(&p[o::VELOCITY..o::VELOCITY + 3]);
            row[i::YAW] = p[o::YAW];
            row[i::PITCH] = p[o::PITCH];
            row[i::NETWORK_EPOCH] = owner.header[crate::network_layout::net_header::EPOCH];
            row[i::NETWORK_GENERATION] = p[o::GENERATION];
            row[i::NETWORK_TICK] = p[o::FROM_TICK];
            row[i::FIRE_COUNTER] = p[o::FIRE_COUNTER];
            if p[o::KIND] == 2. { for (a, b) in [ (i::WALKER_MODE, o::MODE), (i::WALKER_TIME, o::TIME),
                    (i::WALKER_LEGS, o::LEGS), (i::WALKER_ROLL, o::ROLL), (i::WALKER_LEAN, o::LEAN),
                    (i::WALKER_REVERSE, o::REVERSE), (i::DEAD, o::DEAD), ] { row[a] = p[b]; } }
            if p[o::KIND] == 0. { row[i::POSTURE] = p[o::POSTURE];
                let state = p[o::STATE] as u32;
                row[i::AIR] = match state { 1 => 1., 2 => 2., 3 | 8 => 3., 4 => 4., _ => 0., };
                row[i::DIVE] = match state { 5 => 1., 6 => -1., _ => 0., };
                row[i::DEAD] = if state == 7 || state == 8 { 1. } else { 0. }; } } }
    // PageVehicles.write keeps prediction, resource and nonhistorical facts.
    // Remote motion comes from this owner's already-sampled native output;
    // death and local prediction retain their original phase inputs.
    pub fn vehicles(&mut self, input: &mut [f64]) {
        use crate::layout::{vehicle_entry as e, vehicle_frame as f, vehicle_mode as m};
        use bfe_pose::timeline_layout::timeline_output as o;
        let Some(owner) = self.owner.as_ref() else { return; };
        let owner = owner.borrow();
        let table=self.table.borrow();
        let count = (input[f::ENTRIES] as usize).min(input.len().saturating_sub(f::STRIDE) / e::STRIDE);
        for at in 0..count { let row = &mut input[f::STRIDE + at * e::STRIDE..][..e::STRIDE];
            if row[e::MODE] != m::ALIVE as f64 { continue; }
            let key = row[e::RIG] as usize;
            let bound = table.rows.get(key) .and_then(Option::as_ref)
                .is_some_and(|b| b.lease == owner.lease && !owner.excluded.contains(&b.id));
            let Some(p) = Self::sample(&table, key, &owner) else { if bound && row[e::LOCAL_PRESENT] == 0. {
                    row[e::DEFERRED] = 1.; }
                continue; };
            if p[o::KIND] != 1. { continue; }
            row[e::NETWORK] = 1.;
            row[e::ALPHA] = 1.;
            row[e::DT] = owner.timeline.input[bfe_pose::timeline_layout::timeline_frame::DT];
            row[e::POSITION..e::POSITION + 3].copy_from_slice(&p[o::POSITION..o::POSITION + 3]);
            row[e::QUATERNION..e::QUATERNION + 4].copy_from_slice(&p[o::QUATERNION..o::QUATERNION + 4]); } } }
