//! One generated boundary carries camera and journal facts for the whole AI
//! phase. Generated list decoding would allocate fresh Vecs each tick; these
//! fixed-stride buffers instead retain both encoded and decoded storage.
use crate::gen::{AiJournalEvent,AiJournalInput,AiPhaseBoundary,AiPhaseCamera,AiPhaseCounters,AiPhaseHeader};

pub struct Exchange {
    pub header:[f64;AiPhaseHeader::WORDS],
    pub camera_words:Vec<f64>,
    pub journal_words:Vec<f64>,
    pub cameras:Vec<AiPhaseCamera>,
    pub journal:AiJournalInput,
    pub control_output:[u32;4],
    descriptor:[f64;AiPhaseBoundary::WORDS],
    counters:[f64;AiPhaseCounters::WORDS],
}
impl Default for Exchange {
    /// Moving the exchange out during a native loan must not reserve another
    /// buffer. Construction alone calls new with the real battle capacities.
    fn default()->Self {Self {header:[0.;AiPhaseHeader::WORDS],camera_words:Vec::new(),journal_words:Vec::new(),
        control_output:[0;4],cameras:Vec::new(),journal:AiJournalInput::default(),descriptor:[0.;AiPhaseBoundary::WORDS],counters:[0.;AiPhaseCounters::WORDS]}}
}
impl Exchange {
    /// The counters bank the descriptor names, for a native host reading by name.
    pub fn counters(&self)->&[f64] {&self.counters}
    /// The banks a host writes before the AI phase, by name, for a host
    /// without the module's memory.
    pub fn bank_mut(&mut self,which:&str)->Option<&mut [f64]> {
        Some(match which {"header"=>&mut self.header[..],"cameras"=>&mut self.camera_words[..],"journal"=>&mut self.journal_words[..],_=>return None})
    }
    pub fn new(cameras:usize,journal:usize)->Self {
        Self {camera_words:vec![0.;cameras*AiPhaseCamera::WORDS],journal_words:vec![0.;journal*AiJournalEvent::WORDS],
            cameras:Vec::with_capacity(cameras),journal:AiJournalInput {sequence:0.,records:Vec::with_capacity(journal)},..Self::default()}
    }
    /// Classic scheduler observes one camera boundary. Other journal kinds
    /// retain sequence positions, even though their payloads never cross it.
    pub fn decode(&mut self)->AiPhaseHeader {
        let mut header=AiPhaseHeader::take(&self.header,&[]);
        // A delayed input source may advertise more rows than it reserved.
        // Clamp on the owner side before any indexing or retained Vec write.
        let cameras=(header.cameras as usize).min(self.camera_words.len()/AiPhaseCamera::WORDS);
        let events=(header.events as usize).min(self.journal_words.len()/AiJournalEvent::WORDS);
        header.cameras=cameras as u32;header.events=events as u32;
        self.cameras.clear();self.journal.records.clear();self.journal.sequence=header.sequence;
        for index in 0..cameras {self.cameras.push(AiPhaseCamera::take(&self.camera_words[index*AiPhaseCamera::WORDS..],&[]));}
        for index in 0..events {self.journal.records.push(AiJournalEvent::take(&self.journal_words[index*AiJournalEvent::WORDS..],&[]));}
        header
    }
    /// Pointers are rebuilt after construction or a native loan, never cached
    /// across moves of the fixed header. Publishing counters copies no bodies.
    pub fn descriptor(&mut self,counters:&AiPhaseCounters)->&[f64] {
        let mut descriptor=AiPhaseBoundary {camera_capacity:(self.camera_words.len()/AiPhaseCamera::WORDS) as u32,
            journal_capacity:(self.journal_words.len()/AiJournalEvent::WORDS) as u32,
            control_count:self.control_output[1],control_version:self.control_output[2],..Default::default()};
        #[cfg(target_arch = "wasm32")]
        self.bind_descriptor(&mut descriptor);
        descriptor.put(&mut self.descriptor,&mut Vec::new());counters.put(&mut self.counters,&mut Vec::new());&self.descriptor
    }
}

#[cfg(target_arch = "wasm32")]
impl Exchange {
    fn bind_descriptor(&self,descriptor:&mut AiPhaseBoundary) {
        descriptor.header=self.header.as_ptr() as usize as u32;descriptor.cameras=self.camera_words.as_ptr() as usize as u32;
        descriptor.journal=self.journal_words.as_ptr() as usize as u32;descriptor.counters=self.counters.as_ptr() as usize as u32;
        descriptor.controls=self.control_output[0];descriptor.control_ack=self.control_output[3];
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    #[test]
    fn native_descriptor_retains_capacity_and_counter_banks_without_addresses() {
        let mut exchange=Exchange::new(3,7);
        exchange.control_output=[11,3,9,17];
        let descriptor=AiPhaseBoundary::take(exchange.descriptor(&AiPhaseCounters::default()),&[]);
        assert_eq!((descriptor.camera_capacity,descriptor.journal_capacity),(3,7));
        assert_eq!((descriptor.control_count,descriptor.control_version),(3,9));
        assert_eq!([descriptor.header,descriptor.cameras,descriptor.journal,descriptor.counters,descriptor.controls,descriptor.control_ack],[0;6]);
    }
}
