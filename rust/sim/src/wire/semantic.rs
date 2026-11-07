//! Lossless semantic checkpoint and summary encoders. Inputs are owned
//! scalar captures; all conditional layouts and wire writes live here.
use crate::store::Strings;

type Result<T> = std::result::Result<T, &'static str>;
const NAN: f64 = f64::from_bits(0x7ff8_0000_0000_0000);
const SAFE: f64 = 9007199254740991.;

struct Input<'a> {
    words: &'a [f64],
    at: usize,
}
impl Input<'_> {
    fn n(&mut self) -> Result<f64> {
        let n = self
            .words
            .get(self.at)
            .copied()
            .ok_or("invalid semantic exchange")?;
        self.at += 1;
        Ok(n)
    }
    fn list(&mut self) -> Result<Vec<f64>> {
        let n = self.count()?;
        (0..n).map(|_| self.n()).collect()
    }
    fn count(&mut self) -> Result<usize> {
        let n = self.n()?;
        if !integer(n, 0., self.words.len() as f64) {
            return Err("invalid semantic exchange");
        }
        Ok(n as usize)
    }
    fn bytes(&mut self) -> Result<Vec<u8>> {
        Ok(self
            .list()?
            .into_iter()
            .map(|n| jsrt::to_uint32(n) as u8)
            .collect())
    }
}
fn integer(n: f64, min: f64, max: f64) -> bool {
    n.is_finite() && n.fract() == 0. && n >= min && n <= max
}
fn object_is(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || a.is_nan() && b.is_nan()
}
fn bool_value(n: f64, error: &'static str) -> Result<u8> {
    if n == 0. {
        Ok(0)
    } else if n == 1. {
        Ok(1)
    } else {
        Err(error)
    }
}
fn enum_index(strings: &Strings, id: f64, values: &[&str], error: &'static str) -> Result<u8> {
    let units = strings
        .list
        .get(jsrt::to_uint32(id) as usize)
        .filter(|_| id >= 0.)
        .ok_or(error)?;
    values
        .iter()
        .position(|s| s.encode_utf16().eq(units.iter().copied()))
        .map(|i| i as u8)
        .ok_or(error)
}
#[derive(Default)]
struct Writer {
    bytes: Vec<u8>,
    limit: usize,
    overflow: &'static str,
}
impl Writer {
    fn new(limit: usize, overflow: &'static str) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
            overflow,
        }
    }
    fn raw(&mut self, bytes: &[u8]) -> Result<()> {
        if self.bytes.len() + bytes.len() > self.limit {
            return Err(self.overflow);
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
    fn int(&mut self, n: f64, size: usize, error: &'static str) -> Result<()> {
        let max = match size {
            1 => 255.,
            2 => 65535.,
            _ => 4294967295.,
        };
        if !integer(n, 0., max) {
            return Err(error);
        }
        self.raw(&(n as u32).to_le_bytes()[..size])
    }
    fn u8(&mut self, n: f64) -> Result<()> {
        self.int(n, 1, "invalid own-player integer")
    }
    fn u16(&mut self, n: f64) -> Result<()> {
        self.int(n, 2, "invalid own-player integer")
    }
    fn u32(&mut self, n: f64) -> Result<()> {
        self.int(n, 4, "invalid own-player integer")
    }
    fn f64(&mut self, n: f64, error: &'static str) -> Result<()> {
        if !n.is_finite() {
            return Err(error);
        }
        self.raw(&n.to_le_bytes())
    }
    fn unchecked(&mut self, n: f64) -> Result<()> {
        self.raw(&n.to_le_bytes())
    }
}
fn weapon(input: &mut Input<'_>, arsenal: bool) -> Result<Vec<u8>> {
    let level = input.n()?;
    if !integer(level, 0., 255.) {
        return Err("weapon state definition mismatch");
    }
    let mut flags = 0u8;
    for i in 0..7 {
        flags |= bool_value(input.n()?, "invalid weapon state flag")? << i;
    }
    let nullable = input.n()? == 1.;
    let mut values = [0.; 20];
    for (i, v) in values.iter_mut().enumerate() {
        *v = input.n()?;
        if i == 7 && nullable {
            *v = NAN;
            continue;
        }
        if !v.is_finite() && !(i == 2 && *v == f64::INFINITY) {
            return Err("invalid weapon state number");
        }
    }
    let mut w = Writer::new(200, "weapon state exceeds bounds");
    w.u8(level)?;
    w.u8(flags as f64)?;
    if arsenal {
        let mut base = [0.; 20];
        for n in &mut base {
            *n = input.n()?;
        }
        let melee = input.n()? == 1.;
        let mut mask = 0u32;
        for i in 0..20 {
            if !object_is(values[i], base[i]) {
                mask |= 1 << i;
            }
        }
        w.u32(mask as f64)?;
        for i in 0..20 {
            if mask & (1 << i) != 0 {
                w.unchecked(values[i])?;
            }
        }
        if melee {
            w.unchecked(input.n()?)?;
            w.raw(&[
                jsrt::to_uint32(input.n()?) as u8,
                jsrt::to_uint32(input.n()?) as u8,
            ])?;
        }
    } else {
        for n in values {
            w.unchecked(n)?;
        }
    }
    Ok(w.bytes)
}
const SOLDIER_NUM: &str = "nonfinite Soldier correction";
fn soldier(input: &mut Input<'_>, strings: &Strings) -> Result<Vec<u8>> {
    if input.n()? != 0. || input.n()? != 0. {
        return Err("Soldier correction requires completed collision pass");
    }
    let posture = enum_index(
        strings,
        input.n()?,
        &["stand", "crouch", "prone"],
        "invalid Soldier state",
    )?;
    let air_id = input.n()?;
    let air = if air_id == -1. {
        0
    } else {
        enum_index(
            strings,
            air_id,
            &["jump", "fall", "thrown", "jet"],
            "invalid Soldier state",
        )? + 1
    };
    let grounded = bool_value(input.n()?, "invalid Soldier state")?;
    let dive_from = input.n()?;
    let mut numbers = [0.; 6];
    for n in &mut numbers {
        *n = input.n()?;
    }
    let mut vectors = Vec::new();
    for _ in 0..4 {
        let valid = input.n()?;
        let mut v = [0.; 3];
        for n in &mut v {
            *n = input.n()?;
        }
        vectors.push((valid, v));
    }
    let settled = !vectors[0]
        .1
        .iter()
        .zip(vectors[3].1)
        .all(|(a, b)| object_is(*a, b));
    let diving = numbers[3] != 0.;
    let mut w = Writer::new(149, "Soldier correction exceeds bounds");
    w.raw(&[
        settled as u8 | ((diving as u8) << 1),
        posture,
        air,
        grounded,
    ])?;
    for n in numbers {
        w.f64(n, SOLDIER_NUM)?;
    }
    for (i, name) in [
        "missing Soldier vector position",
        "missing Soldier vector velocity",
        "missing Soldier vector normal",
        "missing Soldier vector settled",
    ]
    .iter()
    .enumerate()
    {
        if i == 3 && !settled {
            break;
        }
        let (valid, v) = vectors[i];
        if valid != 1. {
            return Err(name);
        }
        for n in v {
            w.f64(n, SOLDIER_NUM)?;
        }
    }
    if diving {
        w.u8(enum_index(
            strings,
            dive_from,
            &["stand", "crouch", "prone"],
            "invalid dive posture",
        )? as f64)?;
    }
    Ok(w.bytes)
}
fn droideka(input: &mut Input<'_>) -> Result<Vec<u8>> {
    if input.n()? != 1. {
        return Err("droideka state prerequisite missing");
    }
    let mode = input.n()?;
    let gun = input.n()?;
    if !integer(mode, 0., 13.) || !integer(gun, 0., SAFE) {
        return Err("invalid droideka mode or barrel");
    }
    let mut flags = 0;
    for i in 0..3 {
        flags |= bool_value(input.n()?, "invalid droideka flag")? << i;
    }
    let shield = input.n()? == 1.;
    let mut shield_value = 0.;
    if shield {
        let on = bool_value(input.n()?, "invalid droideka shield")?;
        shield_value = input.n()?;
        let max = input.n()?;
        if shield_value < 0. || shield_value > max {
            return Err("invalid droideka shield");
        }
        flags |= 8 | (on << 4);
    }
    let last = input.n()? == 1.;
    let mut barrel = [0.; 2];
    if last {
        barrel = [input.n()?, input.n()?];
        let count = input.n()?;
        let guns = input.n()?;
        if !integer(barrel[0], 0., guns - 1.) || !integer(barrel[1], 0., count - 1.) {
            return Err("invalid droideka last barrel");
        }
        flags |= 32;
    }
    let mut w = Writer::new(194, "droideka state exceeds bounds");
    w.u8(mode)?;
    w.u8(flags as f64)?;
    for _ in 0..21 {
        w.f64(input.n()?, "nonfinite droideka state")?;
    }
    if shield {
        w.f64(shield_value, "nonfinite droideka state")?;
    }
    if last {
        for n in barrel {
            w.f64(n, "nonfinite droideka state")?;
        }
    }
    Ok(w.bytes)
}
fn seat(input: &mut Input<'_>, w: &mut Writer, error: &'static str) -> Result<()> {
    for _ in 0..6 {
        let tag = input.n()?;
        w.u8(tag)?;
        w.f64(input.n()?, error)?;
    }
    Ok(())
}
fn ground(input: &mut Input<'_>) -> Result<Vec<u8>> {
    if input.n()? != 1. {
        return Err("unsupported ground correction");
    }
    let mut w = Writer::new(224, "ground correction exceeds bounds");
    for _ in 0..21 {
        w.f64(input.n()?, "nonfinite ground correction")?;
    }
    for error in ["invalid ground alive", "invalid ground sleeping"] {
        w.u8(bool_value(input.n()?, error)? as f64)?;
    }
    seat(input, &mut w, "nonfinite ground correction")?;
    Ok(w.bytes)
}
const FLIGHT_MESSAGES: [&str; 8] = [
    "",
    "Takeoff path blocked",
    "Landing path blocked",
    "Too fast",
    "No landing zone",
    "Press SPACE to land",
    "Flight ceiling",
    "Leaving battlefield",
];
fn flight(input: &mut Input<'_>, strings: &Strings) -> Result<Vec<u8>> {
    if input.n()? != 1. {
        return Err("unsupported compact flight correction");
    }
    let mut enums = [0u8; 6];
    for (i, (values, error)) in [
        (
            &["Landed", "TakingOff", "Flying", "Landing", "Destroyed"][..],
            "invalid flight flightState",
        ),
        (
            &["", "barrel", "loop", "immelmann"][..],
            "invalid flight trick",
        ),
        (
            &["", "Locking", "Locked"][..],
            "invalid flight flightLockState",
        ),
        (&FLIGHT_MESSAGES[..], "invalid flight flightWarning"),
        (&FLIGHT_MESSAGES[..], "invalid flight landingMessage"),
        (&FLIGHT_MESSAGES[..], "invalid flight landingReason"),
    ]
    .into_iter()
    .enumerate()
    {
        enums[i] = enum_index(strings, input.n()?, values, error)?;
    }
    let transition = enums[0] == 1 || enums[0] == 3;
    let trick = enums[1];
    let mut w = Writer::new(800, "flight correction exceeds bounds");
    w.raw(&enums)?;
    for i in 0..31 {
        let n = input.n()?;
        if i >= 29 && trick == 0 {
            continue;
        }
        if i == 13 && n.is_nan() {
            w.unchecked(n)?;
        } else {
            w.f64(n, "nonfinite flight correction")?;
        }
    }
    for (i, error) in [
        "missing flight vector position",
        "missing flight vector velocity",
        "missing flight vector flightStart",
        "missing flight vector flightEnd",
        "missing flight vector flightTrickRight",
    ]
    .into_iter()
    .enumerate()
    {
        let valid = input.n()?;
        let v = [input.n()?, input.n()?, input.n()?];
        if i >= 2 && i < 4 && !transition || i == 4 && trick != 1 {
            continue;
        }
        if valid != 1. {
            return Err(error);
        }
        for n in v {
            w.f64(n, "nonfinite flight correction")?;
        }
    }
    for (i, error) in [
        "missing flight vector quaternion",
        "missing flight vector flightStartQ",
        "missing flight vector flightTrickLoopQ",
        "missing flight vector flightTrickEndQ",
    ]
    .into_iter()
    .enumerate()
    {
        let valid = input.n()?;
        let v = [input.n()?, input.n()?, input.n()?, input.n()?];
        if i == 1 && !transition || i >= 2 && trick != 3 {
            continue;
        }
        if valid != 1. {
            return Err(error);
        }
        for n in v {
            w.f64(n, "nonfinite flight correction")?;
        }
    }
    for _ in 0..10 {
        let n = input.n()?;
        if !integer(n, 0., 2.) {
            return Err("invalid flight boolean");
        }
        w.u8(n)?;
    }
    seat(input, &mut w, "nonfinite flight correction")?;
    Ok(w.bytes)
}

fn mounted(input: &mut Input<'_>) -> Result<Vec<u8>> {
    let mode = input.n()?;
    let id = input.n()?;
    let generation = input.n()?;
    let index = input.n()?;
    let values = input.list()?;
    let count = input.count()?;
    if count > 2 {
        return Err("mounted weapon count exceeds correction budget");
    }
    let mut w = Writer::new(4096, "mounted control exceeds bounds");
    w.u8(mode)?;
    w.u32(id)?;
    w.u32(generation)?;
    w.u8(index)?;
    let expected = if mode == 4. {
        0
    } else if mode == 2. {
        4
    } else {
        6
    };
    if values.len() != expected {
        return Err("invalid semantic exchange");
    }
    let mut flags = 0;
    // The passenger values carry look targets as pairs of present/value.
    let look_yaw = input.n()?;
    let look_pitch = input.n()?;
    if mode == 2. {
        if look_yaw == 1. {
            flags |= 1;
        }
        if look_pitch == 1. {
            flags |= 2;
        }
    }
    w.u8(flags as f64)?;
    for (i, n) in values.into_iter().enumerate() {
        if mode == 2. && (i == 2 && flags & 1 == 0 || i == 3 && flags & 2 == 0) {
            continue;
        }
        w.f64(n, "nonfinite mounted control")?;
    }
    w.u8(count as f64)?;
    for _ in 0..count {
        let barrel = input.n()?;
        let bytes = input.bytes()?;
        if !integer(barrel, 0., SAFE) {
            return Err("invalid mounted barrel");
        }
        w.unchecked(barrel)?;
        w.u16(bytes.len() as f64)?;
        w.raw(&bytes)?;
    }
    Ok(w.bytes)
}
fn own(input: &mut Input<'_>) -> Result<Vec<u8>> {
    let mut w = Writer::new(1100, "own-player envelope exceeds budget");
    w.u32(input.n()?)?;
    w.u32(input.n()?)?;
    w.u16(input.n()?)?;
    let team = input.n()?;
    if !integer(team, 0., 2.) {
        return Err("invalid own-player integer");
    }
    w.u8(team)?;
    w.u8(input.n()?)?;
    w.u32(input.n()?)?;
    w.u32(input.n()?)?;
    let body = input.bytes()?;
    w.u16(body.len() as f64)?;
    w.raw(&body)?;
    w.f64(input.n()?, "invalid own-player number")?;
    w.u8(input.n()?)?;
    w.f64(input.n()?, "invalid own-player number")?;
    w.f64(input.n()?, "invalid own-player number")?;
    w.u8(input.n()?)?;
    for _ in 0..3 {
        w.f64(input.n()?, "invalid own-player number")?;
    }
    w.u8(input.n()?)?;
    w.f64(input.n()?, "invalid own-player number")?;
    let first = input.n()?;
    let water = input.n()?;
    w.u8((jsrt::to_uint32(first) | (jsrt::to_uint32(water) << 1)) as f64)?;
    for _ in 0..2 {
        let selected = input.n()?;
        if selected != 255. && !integer(selected, 0., 254.) {
            return Err("invalid own-player integer");
        }
        w.u8(selected)?;
    }
    let count = input.count()?;
    w.u8(count as f64)?;
    for _ in 0..count {
        w.u16(input.n()?)?;
        let bytes = input.bytes()?;
        w.u16(bytes.len() as f64)?;
        w.raw(&bytes)?;
    }
    let mut bits = [0u8; 16];
    for n in input.list()? {
        if !integer(n, 0., 127.) {
            return Err("held code missing from registry");
        }
        let i = n as usize;
        bits[i >> 3] |= 1 << (i & 7);
    }
    w.raw(&bits)?;
    let mut flags = 0u16;
    for i in 0..5 {
        if input.n()? != 0. {
            flags |= 1 << i;
        }
    }
    let mut view = [None; 5];
    for (i, n) in view.iter_mut().enumerate() {
        let present = input.n()?;
        let value = input.n()?;
        if present != 0. {
            *n = Some(value);
            flags |= 1 << (i + 5);
        }
    }
    if input.n()? != 0. {
        flags |= 1024;
    }
    w.u16(flags as f64)?;
    for n in view[..3].iter().flatten() {
        w.f64(*n, "invalid own-player number")?;
    }
    w.u16(input.n()?)?;
    for n in view[3..].iter().flatten() {
        w.f64(*n, "invalid own-player number")?;
    }
    let axes = [input.n()?, input.n()?, input.n()?, input.n()?];
    let mut buttons = 0;
    for i in 0..16 {
        if input.n()? != 0. {
            buttons |= 1 << i;
        }
    }
    if flags & 16 != 0 {
        for n in axes {
            w.f64(n, "invalid own-player number")?;
        }
        w.u16(buttons as f64)?;
    }
    w.u32(input.n()?)?;
    let mode = input.n()?;
    w.u8(mode)?;
    let mounted = input.bytes()?;
    if !mounted.is_empty() {
        w.u16(mounted.len() as f64)?;
        w.raw(&mounted)?;
    }
    if mode == 1. {
        w.u16(input.n()?)?;
        w.u32(input.n()?)?;
        w.u16(input.n()?)?;
        let bytes = input.bytes()?;
        w.u16(bytes.len() as f64)?;
        w.raw(&bytes)?;
    }
    Ok(w.bytes)
}
fn recon(input: &mut Input<'_>) -> Result<Vec<u8>> {
    let count = input.count()?;
    let inventory = input.count()?;
    let selected = input.list()?;
    if count > 2 || count != inventory || selected.len() != 2 {
        return Err("unsupported recon arsenal");
    }
    let zoom = input.n()?;
    if !integer(zoom, 0., 1.) {
        return Err("invalid recon zoom");
    }
    for n in &selected {
        let channels = input.n()?;
        if !integer(*n, 0., jsrt::js_max(1., channels) - 1.) {
            return Err("invalid recon selected weapon");
        }
    }
    let dead = bool_value(input.n()?, "invalid recon control state")?;
    let piloted = bool_value(input.n()?, "invalid recon control state")?;
    let first = input.n()? != 0.;
    if dead != 0 && piloted != 0 {
        return Err("invalid recon control state");
    }
    let mut w = Writer::new(900, "recon correction overflow/truncation");
    w.u8(2.)?;
    w.u8((dead | (piloted << 1) | ((first as u8) << 2)) as f64)?;
    for _ in 0..6 {
        w.f64(input.n()?, "invalid recon number")?;
    }
    w.u8(zoom)?;
    w.int(input.n()?, 4, "invalid recon integer")?;
    for _ in 0..2 {
        let values = input.list()?;
        if values.len() != 3 {
            return Err("invalid recon vector");
        }
        for n in values {
            w.f64(n, "invalid recon number")?;
        }
    }
    for n in selected {
        w.int(n, 1, "invalid recon byte")?;
    }
    w.u8(count as f64)?;
    for _ in 0..count {
        let bytes = input.bytes()?;
        w.int(bytes.len() as f64, 2, "invalid recon integer")?;
        w.raw(&bytes)?;
    }
    let salvo = input.n()? != 0.;
    w.u8(salvo as u8 as f64)?;
    if salvo {
        w.int(input.n()?, 1, "invalid recon byte")?;
        w.int(input.n()?, 1, "invalid recon byte")?;
        let left = input.n()?;
        let max = input.n()?;
        if left > max {
            return Err("invalid recon salvo count");
        }
        w.int(left, 4, "invalid recon integer")?;
        w.f64(input.n()?, "invalid recon number")?;
        for _ in 0..4 {
            let values = input.list()?;
            if values.len() != 3 {
                return Err("invalid recon vector");
            }
            for n in values {
                w.f64(n, "invalid recon number")?;
            }
        }
    }
    let camera = input.n()? != 0.;
    w.u8(camera as u8 as f64)?;
    if camera {
        let position = input.list()?;
        let q = input.list()?;
        let aspect = input.n()?;
        let fov = input.n()?;
        let reach = input.n()?;
        let fresh = bool_value(input.n()?, "invalid recon camera")?;
        if !(aspect > 0.) || (jsrt::js_hypot(&q) - 1.).abs() > 0.001 {
            return Err("invalid recon camera");
        }
        if position.len() != 3 || q.len() != 4 {
            return Err("invalid recon vector");
        }
        for n in position.into_iter().chain(q).chain([aspect, fov, reach]) {
            w.f64(n, "invalid recon number")?;
        }
        w.u8(fresh as f64)?;
    }
    Ok(w.bytes)
}

fn summary_writer() -> Writer {
    Writer::new(1000, "summary exceeds bounds")
}
fn su8(w: &mut Writer, n: f64) -> Result<()> {
    w.int(n, 1, "invalid summary scalar")
}
fn su16(w: &mut Writer, n: f64) -> Result<()> {
    w.int(n, 2, "invalid summary scalar")
}
fn su32(w: &mut Writer, n: f64) -> Result<()> {
    w.int(n, 4, "invalid summary scalar")
}
fn sf(w: &mut Writer, n: f64) -> Result<()> {
    w.f64(n, "invalid summary scalar")
}
fn sb(w: &mut Writer, n: f64) -> Result<()> {
    su8(w, bool_value(n, "invalid summary boolean")? as f64)
}
fn battle_summary(input: &mut Input<'_>) -> Result<Vec<u8>> {
    let count = input.count()?;
    if count > 7 {
        return Err("summary team limit");
    }
    let mut teams = Vec::new();
    for _ in 0..count {
        let mut row = [0.; 5];
        for n in &mut row {
            *n = input.n()?;
        }
        teams.push(row);
    }
    // Object's team keys sort numerically before writing, with a NaN
    // comparator treated as equality as in Array.sort.
    jsrt::sort::js_sort(&mut teams, &mut |a, b| a[0] - b[0]);
    let mut w = summary_writer();
    su8(&mut w, count as f64)?;
    for row in teams {
        su8(&mut w, row[0])?;
        for n in &row[1..] {
            sf(&mut w, *n)?;
        }
    }
    for _ in 0..4 {
        sb(&mut w, input.n()?)?;
    }
    su8(&mut w, input.n()?)?;
    sf(&mut w, input.n()?)?;
    for _ in 0..2 {
        let present = input.n()?;
        sb(&mut w, present)?;
        let value = input.n()?;
        if present != 0. {
            sf(&mut w, value)?;
        }
    }
    Ok(w.bytes)
}
fn post_summary(input: &mut Input<'_>) -> Result<Vec<u8>> {
    let mut w = summary_writer();
    su16(&mut w, input.n()?)?;
    su8(&mut w, input.n()?)?;
    su8(&mut w, input.n()?)?;
    sf(&mut w, input.n()?)?;
    sf(&mut w, input.n()?)?;
    sb(&mut w, input.n()?)?;
    sb(&mut w, input.n()?)?;
    let count = input.count()?;
    if count > 7 {
        return Err("summary post team limit");
    }
    su8(&mut w, count as f64)?;
    for _ in 0..count {
        su8(&mut w, input.n()?)?;
        su16(&mut w, input.n()?)?;
    }
    Ok(w.bytes)
}
fn spawn_summary(input: &mut Input<'_>, strings: &Strings) -> Result<Vec<u8>> {
    let mut w = summary_writer();
    su32(&mut w, input.n()?)?;
    su32(&mut w, input.n()?)?;
    su8(&mut w, input.n()?)?;
    su8(
        &mut w,
        enum_index(
            strings,
            input.n()?,
            &["unspawned", "alive", "dying", "awaiting"],
            "invalid spawn phase",
        )? as f64,
    )?;
    su16(&mut w, input.n()?)?;
    su16(&mut w, input.n()?)?;
    sb(&mut w, input.n()?)?;
    sb(&mut w, input.n()?)?;
    sf(&mut w, input.n()?)?;
    let pending = input.n()?;
    sb(&mut w, pending)?;
    if pending != 0. {
        su16(&mut w, input.n()?)?;
        su16(&mut w, input.n()?)?;
    }
    let auto = input.n()?;
    sb(&mut w, auto)?;
    if auto != 0. {
        su8(&mut w, input.n()?)?;
        su16(&mut w, input.n()?)?;
        su16(&mut w, input.n()?)?;
    }
    for _ in 0..2 {
        let list = input.list()?;
        if list.len() > 64 {
            return Err("spawn eligibility limit");
        }
        su8(&mut w, list.len() as f64)?;
        for n in list {
            su16(&mut w, n)?;
        }
    }
    let slot = input.n()?;
    su8(&mut w, if integer(slot, 0., 254.) { slot } else { 255. })?;
    Ok(w.bytes)
}
fn utf8(strings: &Strings, id: f64) -> Result<Vec<u8>> {
    strings
        .list
        .get(jsrt::to_uint32(id) as usize)
        .filter(|_| id >= 0.)
        .map(|s| jsrt::text::text_encode(s))
        .ok_or("invalid semantic string")
}
fn roster(input: &mut Input<'_>, strings: &Strings) -> Result<Vec<Vec<u8>>> {
    let total = input.count()?;
    if total > 2400 {
        return Err("roster summary limit");
    }
    let mut out = Vec::new();
    let mut start = 0;
    while start < total {
        let count = (total - start).min(12);
        let mut w = summary_writer();
        su16(&mut w, total as f64)?;
        su16(&mut w, start as f64)?;
        su8(&mut w, count as f64)?;
        for _ in 0..count {
            let human = input.n()? != 0.;
            let character_human = input.n()? != 0.;
            let team = input.n()?;
            su8(
                &mut w,
                if !human && character_human {
                    0.
                } else {
                    jsrt::js_max(0., jsrt::js_min(255., jsrt::to_int32(team) as f64))
                },
            )?;
            su8(&mut w, human as u8 as f64)?;
            for _ in 0..4 {
                su16(
                    &mut w,
                    jsrt::js_max(0., jsrt::js_min(65535., jsrt::to_int32(input.n()?) as f64)),
                )?;
            }
            let rtt = input.n()?;
            su16(
                &mut w,
                if rtt.is_finite() {
                    jsrt::js_min(65534., jsrt::js_max(0., jsrt::js_round(rtt)))
                } else {
                    65535.
                },
            )?;
            let kind = input.n()?;
            let family = input.n()?;
            let link = if kind < 0. || family < 0. {
                0
            } else {
                let k = utf8(strings, kind)?;
                let f = utf8(strings, family)?;
                if k.is_empty() || f.is_empty() {
                    0
                } else {
                    (if k == b"relay" { 3 } else { 1 }) + u8::from(f.contains(&b'6'))
                }
            };
            su8(&mut w, link as f64)?;
            let mut name = utf8(strings, input.n()?)?;
            name.truncate(32);
            su8(&mut w, name.len() as f64)?;
            w.raw(&name)?;
        }
        out.push(w.bytes);
        start += count;
    }
    Ok(out)
}
fn ms(seconds: f64, quantum: f64) -> f64 {
    jsrt::js_min(
        4294967295.,
        jsrt::js_max(0., (seconds * 1000. / quantum).floor() * quantum),
    )
}
struct Packed {
    tallies: [f64; 10],
    favourites: [Option<(f64, f64)>; 3],
    victims: Vec<(f64, f64)>,
}
fn record_summaries(input: &mut Input<'_>) -> Result<Vec<Vec<u8>>> {
    let quantum = input.n()?;
    let count = input.count()?;
    let mut out = Vec::new();
    let mut chunk = summary_writer();
    let mut size = 0;
    chunk.bytes.push(0);
    for _ in 0..count {
        let slot = input.n()?;
        let slots = input.count()?;
        let mut packed = Vec::new();
        let mut mask = 0u32;
        for i in 0..slots {
            let tally_nonzero = input.n()? != 0.;
            let mut p = Packed {
                tallies: [0.; 10],
                favourites: [None; 3],
                victims: Vec::new(),
            };
            for n in &mut p.tallies {
                *n = input.n()?;
            }
            let victims = input.count()?;
            for _ in 0..victims {
                let id = input.n()?;
                let n = input.n()?;
                if id >= 0. {
                    p.victims.push((id, n));
                }
            }
            for table in &mut p.favourites {
                let entries = input.count()?;
                for _ in 0..entries {
                    let id = input.n()?;
                    let time = input.n()?;
                    if table.is_none_or(|(_, t)| time > t) {
                        *table = Some((id, time));
                    }
                }
            }
            let nonzero =
                tally_nonzero || !p.victims.is_empty() || p.favourites.iter().any(Option::is_some);
            if nonzero {
                mask |= 1u32.wrapping_shl(i as u32);
                packed.push(p);
            }
        }
        if mask == 0 {
            continue;
        }
        let mut entry = Writer::new(4096, "summary exceeds bounds");
        su16(&mut entry, slot)?;
        su8(&mut entry, mask as f64)?;
        for p in packed {
            for (i, n) in p.tallies.into_iter().enumerate() {
                if i >= 8 {
                    su32(&mut entry, ms(n, quantum))?;
                } else if i == 5 {
                    su16(&mut entry, jsrt::js_min(65535., jsrt::to_int32(n) as f64))?;
                } else {
                    su8(&mut entry, jsrt::js_min(255., jsrt::to_int32(n) as f64))?;
                }
            }
            for best in p.favourites {
                let (id, time) = best.unwrap_or((0., 0.));
                su16(&mut entry, id)?;
                if id != 0. {
                    su32(&mut entry, ms(time, quantum))?;
                }
            }
            let victims: Vec<_> = p
                .victims
                .into_iter()
                .filter(|(id, _)| *id < 65535.)
                .take(255)
                .collect();
            su8(&mut entry, victims.len() as f64)?;
            for (id, n) in victims {
                su16(&mut entry, id)?;
                su8(&mut entry, jsrt::js_min(255., jsrt::to_int32(n) as f64))?;
            }
        }
        if entry.bytes.len() > 900 {
            continue;
        }
        if size != 0 && (chunk.bytes.len() + entry.bytes.len() > 900 || size == 255) {
            chunk.bytes[0] = size;
            out.push(chunk.bytes);
            chunk = summary_writer();
            chunk.bytes.push(0);
            size = 0;
        }
        chunk.raw(&entry.bytes)?;
        size += 1;
    }
    if size != 0 {
        chunk.bytes[0] = size;
        out.push(chunk.bytes);
    }
    Ok(out)
}

pub fn encode(op: u32, words: &[f64], strings: &Strings) -> Result<Vec<Vec<u8>>> {
    let mut input = Input { words, at: 0 };
    let parts = match op {
        0 => vec![weapon(&mut input, false)?],
        1 => vec![weapon(&mut input, true)?],
        2 => vec![droideka(&mut input)?],
        3 => vec![soldier(&mut input, strings)?],
        4 => vec![ground(&mut input)?],
        5 => vec![flight(&mut input, strings)?],
        6 => vec![mounted(&mut input)?],
        7 => vec![own(&mut input)?],
        8 => vec![recon(&mut input)?],
        9 => vec![battle_summary(&mut input)?],
        10 => vec![post_summary(&mut input)?],
        11 => vec![spawn_summary(&mut input, strings)?],
        12 => roster(&mut input, strings)?,
        13 => record_summaries(&mut input)?,
        _ => return Err("unknown semantic operation"),
    };
    if input.at != words.len() {
        return Err("semantic exchange trailing values");
    }
    Ok(parts)
}
#[derive(Default)]
pub struct Semantic {
    input: Vec<f64>,
    output: Vec<u8>,
}
pub(crate) fn wire_semantic_create_in(w: &mut crate::world::owner::World) -> usize {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");

    owner.semantic.insert(Semantic::default()).unwrap_or(0)

}
pub(crate) unsafe fn wire_semantic_drop_in(w: &mut crate::world::owner::World, p: usize) {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.semantic.get(p as usize).is_none() {return;}

    if p != 0 {
        owner.semantic.remove(p as usize);
    }

}
pub(crate) unsafe fn wire_semantic_room_in(w: &mut crate::world::owner::World, p: usize, n: u32) -> Option<&mut [f64]> {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.semantic.get(p as usize).is_none() {return None;}

    let p = owner.semantic.get_mut(p as usize).unwrap();
    p.input.clear();
    p.input.resize(n as usize, 0.);
    Some(&mut p.input[..])

}
pub(crate) unsafe fn wire_semantic_run_in(w: &mut crate::world::owner::World, p: usize, op: u32) -> u32 {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.semantic.get(p as usize).is_none() {return 0;}

    let p = owner.semantic.get_mut(p as usize).unwrap();
    let result = encode(op, &p.input, &w.sim.strings);
    p.output.clear();
    match result {
        Ok(parts) => {
            p.output
                .extend_from_slice(&(parts.len() as u32).to_le_bytes());
            for bytes in parts {
                p.output
                    .extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                p.output.extend(bytes);
            }
            0
        }
        Err(error) => w.sim.strings.intern_str(error).0 + 1,
    }

}
pub(crate) unsafe fn wire_semantic_ptr_in(w: &mut crate::world::owner::World, p: usize) -> Option<&[u8]> {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.semantic.get(p as usize).is_none() {return None;}

    Some(&owner.semantic.get_mut(p as usize).unwrap().output[..])

}
pub(crate) unsafe fn wire_semantic_len_in(w: &mut crate::world::owner::World, p: usize) -> u32 {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.semantic.get(p as usize).is_none() {return 0;}

    owner.semantic.get_mut(p as usize).unwrap().output.len() as u32

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weapon_delta_retains_signed_zero_and_canonical_nullable_nan() {
        let strings = Strings::default();
        let mut words = vec![0., 1., 0., 1., 0., 1., 0., 1., 1.];
        let mut values = [0.; 20];
        values[0] = -0.;
        values[2] = f64::INFINITY;
        values[7] = NAN;
        words.extend(values);
        let full = encode(0, &words, &strings).unwrap().remove(0);
        assert_eq!(&full[..2], &[0, 85]);
        assert_eq!(&full[2..10], &(-0_f64).to_le_bytes());
        assert_eq!(&full[58..66], &NAN.to_le_bytes());
        values[0] = 0.;
        words.extend(values);
        words.push(0.);
        let delta = encode(1, &words, &strings).unwrap().remove(0);
        assert_eq!(&delta[..6], &[0, 85, 1, 0, 0, 0]);
        assert_eq!(&delta[6..], &(-0_f64).to_le_bytes());
        words[1] = f64::NAN;
        assert_eq!(
            encode(1, &words, &strings),
            Err("invalid weapon state flag")
        );
    }

    #[test]
    fn record_selection_and_quantization_skip_empty_and_unknown_victims() {
        let strings = Strings::default();
        let mut words = vec![1000., 2., 0., 1., 0.];
        words.extend([0.; 10]);
        words.extend([1., -1., 255., 0., 0., 0.]);
        words.extend([7., 1., 1.]);
        words.extend([0., 0., 0., 0., 0., 65536., 0., 0., 13.9999, 2.0001]);
        words.extend([1., 4., 300.]);
        words.extend([2., 1., 13.9999, 2., 13.9999, 0., 0.]);
        let result = encode(13, &words, &strings).unwrap();
        assert_eq!(result.len(), 1);
        let bytes = &result[0];
        assert_eq!(&bytes[..4], &[1, 7, 0, 1]);
        assert_eq!(&bytes[9..11], &[255, 255]);
        assert_eq!(&bytes[13..17], &13000u32.to_le_bytes());
        assert_eq!(&bytes[17..21], &2000u32.to_le_bytes());
        assert_eq!(&bytes[21..27], &[1, 0, 200, 50, 0, 0]);
        assert_eq!(&bytes[31..], &[1, 4, 0, 255]);
    }
}

// The WASM boundary owns addresses; shared services borrow their owners.
#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;
    #[no_mangle]
    pub extern "C" fn wire_semantic_create() -> *mut Semantic { wire_semantic_create_in(crate::abi::facade::world()) as *mut Semantic }
    #[no_mangle]
    pub unsafe extern "C" fn wire_semantic_drop(p: *mut Semantic) { wire_semantic_drop_in(crate::abi::facade::world(), p as usize) }
    #[no_mangle]
    pub unsafe extern "C" fn wire_semantic_room(p: *mut Semantic, n: u32) -> *mut f64 { wire_semantic_room_in(crate::abi::facade::world(), p as usize, n).map_or(std::ptr::null_mut(), |b| b.as_mut_ptr()) }
    #[no_mangle]
    pub unsafe extern "C" fn wire_semantic_run(p: *mut Semantic, op: u32) -> u32 { wire_semantic_run_in(crate::abi::facade::world(), p as usize, op) }
    #[no_mangle]
    pub unsafe extern "C" fn wire_semantic_ptr(p: *mut Semantic) -> *const u8 { wire_semantic_ptr_in(crate::abi::facade::world(), p as usize).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub unsafe extern "C" fn wire_semantic_len(p: *mut Semantic) -> u32 { wire_semantic_len_in(crate::abi::facade::world(), p as usize) }
}
#[cfg(target_arch = "wasm32")]
pub use exports::{wire_semantic_create, wire_semantic_drop, wire_semantic_room, wire_semantic_run, wire_semantic_ptr, wire_semantic_len};
