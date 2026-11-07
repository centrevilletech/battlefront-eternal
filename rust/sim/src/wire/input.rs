//! BNI1 input and BNA1 acknowledgement encoding. Registry/profile lookup
//! and JSON context identity remain at the adapter boundary.
use crate::gen::{WiAck, WiPacket, WiView};
type Result<T> = std::result::Result<T, &'static str>;

struct Fields { bytes: Vec<u8>, limit: usize }
impl Fields {
    fn new(limit: usize) -> Self { Self { bytes: Vec::new(), limit } }
    fn reserve(&self, count: usize) -> Result<()> {
        if self.bytes.len() + count > self.limit { Err("input packet exceeds bounds") } else { Ok(()) }
    }
    fn uint(&mut self, value: f64, size: usize, max: f64) -> Result<()> {
        self.reserve(size)?;
        if !value.is_finite() || value.fract() != 0. || value < 0. || value > max { return Err("invalid input scalar"); }
        self.bytes.extend_from_slice(&(value as u32).to_le_bytes()[..size]); Ok(())
    }
    fn u8(&mut self, value: f64) -> Result<()> { self.uint(value, 1, 255.) }
    fn u16(&mut self, value: f64) -> Result<()> { self.uint(value, 2, 65535.) }
    fn u32(&mut self, value: f64) -> Result<()> { self.uint(value, 4, 4294967295.) }
    fn f64(&mut self, value: f64) -> Result<()> {
        self.reserve(8)?; if !value.is_finite() { return Err("invalid input scalar"); }
        self.bytes.extend_from_slice(&value.to_le_bytes()); Ok(())
    }
}
fn view(w: &mut Fields, v: &WiView) -> Result<()> {
    if !v.profile_found { return Err("unknown input binding profile"); }
    w.u16(v.flags)?; w.u16(v.profile)?;
    let flags = jsrt::to_uint32(v.flags);
    for (bit, value) in [(32, v.mouse_scale), (64, v.aspect), (128, v.view_tick), (256, v.sensitivity), (512, v.curve)] {
        if flags & bit != 0 { w.f64(value)?; }
    }
    if flags & 16 != 0 { for n in v.axes { w.f64(n)?; } w.u16(v.buttons)?; }
    Ok(())
}
fn validate_view(v: &WiView) -> Result<()> {
    let f = jsrt::to_uint32(v.flags);
    if f > 1023 { return Err("invalid input view"); }
    if f & (256 | 512) != 0 && f & 4 == 0 { return Err("invalid flight view"); }
    if f & 128 != 0 && (v.view_tick < 0. || v.view_tick > 4294967295.)
        || f & 32 != 0 && (v.mouse_scale <= 0. || v.mouse_scale > 2.)
        || f & 64 != 0 && (v.aspect <= 0. || v.aspect > 10.)
        || f & 256 != 0 && (v.sensitivity <= 0. || v.sensitivity > 10.)
        || f & 512 != 0 && (v.curve < 0.1 || v.curve > 4.)
        || f & 16 != 0 && v.axes.iter().any(|n| n.abs() > 1.) { return Err("invalid input view range"); }
    Ok(())
}
pub fn encode(packet: &WiPacket) -> Result<Vec<u8>> {
    if packet.commands.len() > 8 || packet.edges.len() > 32 { return Err("input count limit"); }
    if let Some(ack) = &packet.ack {
        if !ack.epoch.is_finite() || ack.epoch.fract() != 0. || ack.epoch < 0. || ack.epoch > 65535.
            || !ack.tick.is_finite() || ack.tick.fract() != 0. || ack.tick < 0. || ack.tick > 4294967295. { return Err("invalid snapshot ack"); }
    }
    let mut w = Fields::new(1100);
    w.u32(0x31494e42u32 as f64)?; w.u32(packet.epoch)?; w.u32(packet.generation)?;
    w.u8(packet.views.len() as f64)?; w.u8(packet.commands.len() as f64)?; w.u8(packet.edges.len() as f64)?; w.u8(packet.ack.is_some() as u8 as f64)?;
    for v in &packet.views { view(&mut w, v)?; }
    for c in &packet.commands {
        if !c.life_matches { return Err("mixed input life"); }
        w.u32(c.sequence)?; w.u32(c.tick)?; w.u8(c.view)?;
        let mut bits = [0u8; 16];
        for code in &c.codes {
            if !code.is_finite() || code.fract() != 0. || *code < 0. || *code >= 128. { return Err("unknown input code"); }
            let i = jsrt::to_uint32(*code) as usize; bits[i >> 3] |= 1 << (i & 7);
        }
        for byte in bits { w.u8(byte as f64)?; }
        w.f64(c.look[0])?; w.f64(c.look[1])?;
    }
    for e in &packet.edges {
        if !e.life_matches { return Err("mixed edge life"); }
        w.u32(e.id)?; w.u32(e.tick)?;
        if e.kind < 0. { return Err("unknown input edge"); } w.u8(e.kind)?;
        match jsrt::to_uint32(e.kind) {
            0 => { if e.action < 0. { return Err("unknown input action"); } w.u8(e.action)?; },
            1 => {
                if !(e.button == 0. || e.button == 1.) || !(e.pressed == 0. || e.pressed == 1.) { return Err("invalid fire edge"); }
                w.u8((jsrt::to_uint32(e.button) | jsrt::to_uint32(e.pressed) << 1) as f64)?;
                w.f64(e.view_tick)?; w.f64(e.aim_yaw)?; w.f64(e.aim_pitch)?;
            },
            2 => { if !e.team_range_valid { return Err("invalid team"); } w.u8(e.team)?; },
            3 => { w.u16(e.class_id)?; w.u16(e.post_id)?; },
            _ => {},
        }
    }
    if let Some(ack) = &packet.ack { w.u16(ack.epoch)?; w.u32(ack.tick)?; }
    // The JS encoder reads its own completed packet to validate ranges.
    // Keep this after all writes so bound/scalar failures keep precedence.
    if packet.views.len() > 8 { return Err("input count limit"); }
    for v in &packet.views { validate_view(v)?; }
    for c in &packet.commands {
        if jsrt::to_uint32(c.view) as usize >= packet.views.len() { return Err("invalid view reference"); }
        for code in &c.codes { if !packet.valid_codes.get(jsrt::to_uint32(*code) as usize).copied().unwrap_or(false) { return Err("unknown held input"); } }
    }
    for e in &packet.edges {
        if e.id == 0. || e.kind > 5. { return Err("invalid edge"); }
        if e.kind == 0. && e.action >= 21. { return Err("unknown action"); }
        if e.kind == 1. && (e.view_tick < 0. || e.view_tick > 4294967295.) { return Err("invalid fire aim"); }
    }
    Ok(w.bytes)
}
pub fn keepalive(ack: &WiAck) -> Result<Vec<u8>> {
    let mut w = Fields::new(10); w.u32(0x31414e42u32 as f64)?; w.u16(ack.epoch)?; w.u32(ack.tick)?; Ok(w.bytes)
}
pub fn wire_input_words(op: u32) -> u32 { if op == 0 { WiPacket::WORDS as u32 } else { WiAck::WORDS as u32 } }

pub(crate) fn wire_input_encode_in(w: &mut crate::world::owner::World, op: u32) -> Option<&[f64]> {
    let (row, heap) = w.sim.calls.args();
    let result = if op == 0 { encode(&WiPacket::take(row, heap)) } else { keepalive(&WiAck::take(row, heap)) };
    super::module::output(&mut w.sim, result.map(|bytes| vec![bytes]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gen::{WiCommand, WiEdge};
    #[test]
    fn established_input_vector_and_ack_tail() {
        let packet = WiPacket {
            epoch: 0x01020304u32 as f64, generation: 0x05060708u32 as f64,
            views: vec![WiView { profile_found: true, ..WiView::default() }],
            commands: vec![WiCommand { life_matches: true, sequence: 0x090a0b0cu32 as f64, tick: 0x0d0e0f10u32 as f64,
                view: 0., codes: vec![22.], look: [1.25, -2.5] }],
            edges: vec![WiEdge { life_matches: true, id: 1., tick: 2., kind: 3., class_id: 0x1122 as f64, post_id: 0x3344 as f64, ..WiEdge::default() }],
            ack: Some(WiAck { epoch: 65534., tick: 4294967295. }), valid_codes: vec![true; 128],
        };
        let expected = "424e4931040302010807060501010101000000000c0b0a09100f0e0d0000004000000000000000000000000000000000000000f43f00000000000004c001000000020000000322114433feffffffffff";
        let actual = encode(&packet).unwrap().iter().map(|b| format!("{b:02x}")).collect::<String>();
        assert_eq!(actual, expected);
        assert_eq!(keepalive(&WiAck { epoch: 65535., tick: 4294967295. }).unwrap(), [66, 78, 65, 49, 255, 255, 255, 255, 255, 255]);
    }
    #[test]
    fn post_encode_validation_rejects_ranges_and_zero_edge_id() {
        let mut packet = WiPacket { epoch: 1., generation: 1., views: vec![WiView { profile_found: true, flags: 32., mouse_scale: 0., ..WiView::default() }], ..WiPacket::default() };
        assert_eq!(encode(&packet), Err("invalid input view range"));
        packet.epoch = f64::NAN;
        assert_eq!(encode(&packet), Err("invalid input scalar"));
        packet.epoch = 1.; packet.views.clear();
        packet.edges.push(WiEdge { life_matches: true, kind: 5., ..WiEdge::default() });
        assert_eq!(encode(&packet), Err("invalid edge"));
    }
}

// The WASM boundary owns addresses; shared services borrow their owners.
#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;
    #[no_mangle]
    pub extern "C" fn wire_input_words(op: u32) -> u32 { super::wire_input_words(op) }
    #[no_mangle]
    pub extern "C" fn wire_input_encode(op: u32) -> *const f64 { wire_input_encode_in(crate::abi::facade::world(), op).map_or(std::ptr::null(), |b| b.as_ptr()) }
}
#[cfg(target_arch = "wasm32")]
pub use exports::{wire_input_encode};
