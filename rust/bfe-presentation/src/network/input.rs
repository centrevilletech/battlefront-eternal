//! BNI1/BNA1 encoding from the existing page receive owner. Commands and
//! views cross as generated rows; completed bytes leave as one packed span.
type Result<T> = super::Result<T>;
#[derive(Default, Clone)]
// captureWireInput resolves names and binding profiles on the page once.
// This generated row carries only numeric context; optional fields retain
// their flag bits rather than inferring presence from zero-valued settings.
struct WiView { flags: f64, profile: f64, profile_found: bool, mouse_scale: f64, aspect: f64, view_tick: f64,
    sensitivity: f64, curve: f64, axes: [f64; 4], buttons: f64, }
impl WiView { const WORDS: usize = crate::network_layout::net_view::STRIDE;
    fn take(row: &[f64], _: &[f64]) -> Self { use crate::network_layout::net_view as f;
        Self { flags: row[f::FLAGS], profile: row[f::PROFILE], profile_found: row[f::PROFILE_FOUND] != 0.,
            mouse_scale: row[f::MOUSE_SCALE], aspect: row[f::ASPECT], view_tick: row[f::VIEW_TICK],
            sensitivity: row[f::SENSITIVITY], curve: row[f::CURVE], axes: row[f::AXES..f::AXES + 4].try_into().unwrap(),
            buttons: row[f::BUTTONS], } } }
#[derive(Default, Clone)]
// Reliable edges keep their original execution tick and life checks. Team,
// spawn and fire validations happen in wire order before bytes are returned.
// Registry strings never enter the native per-command encoder.
struct WiEdge { life_matches: bool, id: f64, tick: f64, kind: f64, action: f64, button: f64, pressed: f64,
    view_tick: f64, aim_yaw: f64, aim_pitch: f64, team: f64, team_range_valid: bool, class_id: f64, post_id: f64, }
impl WiEdge { const WORDS: usize = crate::network_layout::net_edge::STRIDE;
    fn take(row: &[f64], _: &[f64]) -> Self { use crate::network_layout::net_edge as f;
        Self { life_matches: row[f::LIFE_MATCHES] != 0., id: row[f::ID], tick: row[f::TICK], kind: row[f::KIND],
            action: row[f::ACTION], button: row[f::BUTTON], pressed: row[f::PRESSED], view_tick: row[f::VIEW_TICK],
            aim_yaw: row[f::AIM_YAW], aim_pitch: row[f::AIM_PITCH], team: row[f::TEAM],
            team_range_valid: row[f::TEAM_RANGE_VALID] != 0., class_id: row[f::CLASS_ID], post_id: row[f::POST_ID], } }
}
// wire-input.js Writer used the same datagram bound. Stack storage permits
// complete validation before publication, with no packet-time reservation.
// A failing scalar never leaves a partially accepted outbound datagram.
struct Fields { bytes: [u8; 1100], at: usize, limit: usize, }
impl Fields {
    fn new(limit: usize) -> Self { Self { bytes: [0; 1100], at: 0, limit } }
    fn reserve(&self, count: usize) -> Result<()> { if self.at + count > self.limit { Err("input packet exceeds bounds")
        } else { Ok(()) } }
    // Writer.u8/u16/u32 reject fractions and nonfinite values before casts.
    // Narrow little-endian prefixes preserve each protocol scalar width.
    fn uint(&mut self, value: f64, size: usize, max: f64) -> Result<()> { self.reserve(size)?;
        if !value.is_finite() || value.fract() != 0. || value < 0. || value > max { return Err("invalid input scalar");
        }
        self.bytes[self.at..self.at + size].copy_from_slice(&(value as u32).to_le_bytes()[..size]);
        self.at += size;
        Ok(()) }
    fn u8(&mut self, value: f64) -> Result<()> { self.uint(value, 1, 255.) }
    fn u16(&mut self, value: f64) -> Result<()> { self.uint(value, 2, 65535.) }
    fn u32(&mut self, value: f64) -> Result<()> { self.uint(value, 4, 4294967295.) }
    // Look and view scalars preserve f64 bits, including signed zero. They
    // must remain finite, but must not round through an integer or f32 lane.
    fn f64(&mut self, value: f64) -> Result<()> { self.reserve(8)?;
        if !value.is_finite() { return Err("invalid input scalar"); }
        self.bytes[self.at..self.at + 8].copy_from_slice(&value.to_le_bytes());
        self.at += 8;
        Ok(()) } }
// writeView emits optional scalars in flag order, followed by joystick axes.
// The final validation follows encoding, preserving the source error order
// when both a scalar and its enclosing context are invalid.
fn view(w: &mut Fields, v: &WiView) -> Result<()> { if !v.profile_found { return Err("unknown input binding profile"); }
    w.u16(v.flags)?;
    w.u16(v.profile)?;
    let flags = jsrt::to_uint32(v.flags);
    for (bit, value) in [(32, v.mouse_scale), (64, v.aspect), (128, v.view_tick), (256, v.sensitivity), (512, v.curve)]
    { if flags & bit != 0 { w.f64(value)?; } }
    if flags & 16 != 0 { for n in v.axes { w.f64(n)?; }
        w.u16(v.buttons)?; }
    Ok(()) }
// validateView keeps flight tuning exclusive to a flight context. Raw flags
// are validated independently from optional scalar ranges; an absent field
// must not fail merely because its reserved record lane contains zero.
fn validate_view(v: &WiView) -> Result<()> { let f = jsrt::to_uint32(v.flags);
    if f > 1023 { return Err("invalid input view"); }
    if f & (256 | 512) != 0 && f & 4 == 0 { return Err("invalid flight view"); }
    if f & 128 != 0 && (v.view_tick < 0. || v.view_tick > 4294967295.)
        || f & 32 != 0 && (v.mouse_scale <= 0. || v.mouse_scale > 2.)
        || f & 64 != 0 && (v.aspect <= 0. || v.aspect > 10.)
        || f & 256 != 0 && (v.sensitivity <= 0. || v.sensitivity > 10.)
        || f & 512 != 0 && (v.curve < 0.1 || v.curve > 4.)
        || f & 16 != 0 && v.axes.iter().any(|n| n.abs() > 1.)
    { return Err("invalid input view range"); }
    Ok(()) }
// encodeEntityInput and encodeSnapshotAckKeepalive share one bounded writer.
// Counts clamp at the exchange reader before fixed command/edge arrays are
// indexed; a complete byte span is published only after trailing validation.
pub fn encode(op: u32, input: &mut super::Words, bytes: &mut [u8]) -> Result<usize> {
    let mut w = Fields::new(if op == crate::network_layout::net_op::ACK { 10 } else { 1100 });
    if op == crate::network_layout::net_op::ACK { w.u32(0x31414e42u32 as f64)?;
        w.u16(input.number()?)?;
        w.u32(input.number()?)?; } else { let epoch = input.number()?;
        let generation = input.number()?;
        let nv = input.uint(8)?;
        let nc = input.uint(8)?;
        let ne = input.uint(32)?;
        let ack = input.uint(1)? != 0;
        let ack_epoch = input.number()?;
        let ack_tick = input.number()?;
        if ack && (!ack_epoch.is_finite() || ack_epoch.fract() != 0. || ack_epoch < 0. || ack_epoch > 65535.
                || !ack_tick.is_finite()
                || ack_tick.fract() != 0.
                || ack_tick < 0.
                || ack_tick > 4294967295.)
        { return Err("invalid snapshot ack"); }
        w.u32(0x31494e42u32 as f64)?;
        w.u32(epoch)?;
        w.u32(generation)?;
        for n in [nv, nc, ne, ack as usize] { w.u8(n as f64)?; }
        // At most eight distinct contexts serve the counted command prefix.
        // Commands reference these rows by index, never by another entity id.
        let mut views: [WiView; 8] = std::array::from_fn(|_| WiView::default());
        for v in &mut views[..nv] { *v = WiView::take(input.span(WiView::WORDS)?, &[]);
            view(&mut w, v)?; }
        // A command's context index is checked after its scalar fields, as
        // the former encode-then-decode validation did. Error precedence is
        // observable to input admission and the frozen malformed fixtures.
        let mut command_views = [0.; 8];
        // Repeated held codes set one bit, yet every code is still validated.
        // The registry-validity suffix prevents a spare bitmap bit from
        // silently creating an input name the receiving side cannot resolve.
        let mut used = [false; 128];
        for vi in &mut command_views[..nc] { if input.uint(1)? == 0 { return Err("mixed input life"); }
            w.u32(input.number()?)?;
            w.u32(input.number()?)?;
            *vi = input.number()?;
            w.u8(*vi)?;
            let count = input.uint(128)?;
            let mut bits = [0u8; 16];
            for _ in 0..count { let code = input.number()?;
                if !code.is_finite() || code.fract() != 0. || code < 0. || code >= 128. {
                    return Err("unknown input code"); }
                let i = code as usize;
                bits[i >> 3] |= 1 << (i & 7);
                used[i] = true; }
            for b in bits { w.u8(b as f64)?; }
            w.f64(input.number()?)?;
            w.f64(input.number()?)?; }
        let mut edges: [WiEdge; 32] = std::array::from_fn(|_| WiEdge::default());
        // Fire edges include the sampled view and aim, while discrete edges
        // carry only their action code. Neither path invokes gameplay hooks.
        for e in &mut edges[..ne] { *e = WiEdge::take(input.span(WiEdge::WORDS)?, &[]);
            if !e.life_matches { return Err("mixed edge life"); }
            w.u32(e.id)?;
            w.u32(e.tick)?;
            if e.kind < 0. { return Err("unknown input edge"); }
            w.u8(e.kind)?;
            match jsrt::to_uint32(e.kind) { 0 => { if e.action < 0. { return Err("unknown input action"); }
                    w.u8(e.action)?; }
                1 => { if !(e.button == 0. || e.button == 1.) || !(e.pressed == 0. || e.pressed == 1.) {
                        return Err("invalid fire edge"); }
                    w.u8((jsrt::to_uint32(e.button) | jsrt::to_uint32(e.pressed) << 1) as f64)?;
                    w.f64(e.view_tick)?;
                    w.f64(e.aim_yaw)?;
                    w.f64(e.aim_pitch)?; }
                2 => { if !e.team_range_valid { return Err("invalid team"); }
                    w.u8(e.team)?; }
                3 => { w.u16(e.class_id)?;
                    w.u16(e.post_id)?; }
                _ => {} } }
        if ack { w.u16(ack_epoch)?;
            w.u32(ack_tick)?; }
        for v in &views[..nv] { validate_view(v)?; }
        for vi in &command_views[..nc] { if jsrt::to_uint32(*vi) as usize >= nv { return Err("invalid view reference");
            } }
        for i in 0..128 { let valid = input.uint(1)? != 0;
            if used[i] && !valid { return Err("unknown held input"); } }
        for e in &edges[..ne] { if e.id == 0. || e.kind > 5. { return Err("invalid edge"); }
            if e.kind == 0. && e.action >= 21. { return Err("unknown action"); }
            if e.kind == 1. && (e.view_tick < 0. || e.view_tick > 4294967295.) { return Err("invalid fire aim"); } } }
    input.finish()?;
    // Publication is one packed span after all validations succeed. Callers
    // can synchronously borrow it without constructing a scalar result array.
    bytes[..w.at].copy_from_slice(&w.bytes[..w.at]);
    Ok(w.at) }
