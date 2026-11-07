// Source-owned interface records use the same context and recovery journal as
// battle resources. Frame packets are transient and never journalled as draws.
use super::BfeRenderer;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl BfeRenderer {
    pub fn interface_input_ptr(&mut self) -> u32 { self.r.interface.input.as_mut_ptr() as u32 }
    pub fn interface_input_len(&self) -> u32 { self.r.interface.input.len() as u32 }
    pub fn interface_buffer_init(&mut self, geometry: u32, buffer: u32, data: &[u8]) -> u32 {
        self.remember_interface_buffer_init(geometry, buffer, data); // recovery
 self.r.interface.add_stream(geometry, buffer, data)
    }
    pub fn interface_buffer_ptr(&mut self, stream: u32) -> u32 { self.r.interface.streams[stream as usize].data.as_mut_ptr() as u32 }
    pub fn interface_buffer_len(&self, stream: u32) -> u32 { self.r.interface.streams[stream as usize].data.len() as u32 }
    // Scalar commands reference persistent bytes rather than marshalling a new
    // byte slice for every changed attribute. The mirror owns recovery state.
    pub fn interface_upload_frame(&mut self, stream: u32, offset: usize, length: usize) {
        if self.r.frames.collecting {self.r.frame_interface_upload(stream as usize,offset,length);return;}
        let stream = &self.r.interface.streams[stream as usize];
        let end = offset.saturating_add(length).min(stream.data.len());
        if end > offset {
            self.r.core.geometry_buffer_sub_data(stream.geometry, stream.buffer, offset as i32, &stream.data[offset..end]);
        }
    }
    pub fn interface_frame(&mut self, draws: usize) {
        self.r.core.begin_phase();
        self.r.interface.draw(&mut self.r.core, draws);
        // The interface pass has no faults of its own to report.
        let _ = self.r.core.end_phase();
    }
}
