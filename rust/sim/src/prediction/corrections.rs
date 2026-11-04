//! Correction readers shared by private replay and native restore callers.
//! These functions validate owned results before the caller publishes state.
//! Prerequisite bits represent plain objects; observable JavaScript reads stay
//! in the caller's original reader.

pub type Result<T> = std::result::Result<T, &'static str>;
const EXCHANGE: &str = "invalid correction exchange";
const SAFE: f64 = 9007199254740991.0;

pub const SOLDIER_VECTORS: [&str; 27] = [
    "position",
    "previous",
    "settled",
    "velocity",
    "normal",
    "contactMean",
    "forwardAxis",
    "rightAxis",
    "left",
    "facing",
    "pull",
    "motion",
    "center",
    "closest",
    "point",
    "contactNormal",
    "surfaceNormal",
    "pushDir",
    "clipDir",
    "push",
    "before",
    "origin",
    "avoidance",
    "stepStart",
    "stepTarget",
    "stepVelocity",
    "stepResolved",
];
pub const SOLDIER_REPLAY_VECTORS: [&str; 7] = [
    "position",
    "previous",
    "settled",
    "velocity",
    "normal",
    "contactMean",
    "avoidance",
];
const SOLDIER_MISSING: [&str; 27] = [
    "missing soldier vector position",
    "missing soldier vector previous",
    "missing soldier vector settled",
    "missing soldier vector velocity",
    "missing soldier vector normal",
    "missing soldier vector contactMean",
    "missing soldier vector forwardAxis",
    "missing soldier vector rightAxis",
    "missing soldier vector left",
    "missing soldier vector facing",
    "missing soldier vector pull",
    "missing soldier vector motion",
    "missing soldier vector center",
    "missing soldier vector closest",
    "missing soldier vector point",
    "missing soldier vector contactNormal",
    "missing soldier vector surfaceNormal",
    "missing soldier vector pushDir",
    "missing soldier vector clipDir",
    "missing soldier vector push",
    "missing soldier vector before",
    "missing soldier vector origin",
    "missing soldier vector avoidance",
    "missing soldier vector stepStart",
    "missing soldier vector stepTarget",
    "missing soldier vector stepVelocity",
    "missing soldier vector stepResolved",
];
pub const COMPACT_FLIGHT_OBJECTS: [&str; 9] = [
    "position",
    "velocity",
    "flightStart",
    "flightEnd",
    "flightTrickRight",
    "quaternion",
    "flightStartQ",
    "flightTrickLoopQ",
    "flightTrickEndQ",
];
pub const FLIGHT_OBJECTS: [&str; 13] = [
    "position",
    "previous",
    "velocity",
    "tickFrom",
    "flightStart",
    "flightEnd",
    "flightTrickRight",
    "quaternion",
    "flightPreviousQ",
    "flightTickQ",
    "flightStartQ",
    "flightTrickLoopQ",
    "flightTrickEndQ",
];
const FLIGHT_MISSING: [&str; 13] = [
    "missing flight vector position",
    "missing flight vector previous",
    "missing flight vector velocity",
    "missing flight vector tickFrom",
    "missing flight vector flightStart",
    "missing flight vector flightEnd",
    "missing flight vector flightTrickRight",
    "missing flight vector quaternion",
    "missing flight vector flightPreviousQ",
    "missing flight vector flightTickQ",
    "missing flight vector flightStartQ",
    "missing flight vector flightTrickLoopQ",
    "missing flight vector flightTrickEndQ",
];
const FLIGHT_ENUM_LENGTHS: [u8; 6] = [5, 4, 3, 8, 8, 8];

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }
    fn byte(&mut self) -> u8 {
        let n = self.bytes[self.at];
        self.at += 1;
        n
    }
    fn u16(&mut self) -> usize {
        let n = u16::from_le_bytes(self.bytes[self.at..self.at + 2].try_into().unwrap());
        self.at += 2;
        n as usize
    }
    fn u32(&mut self) -> u32 {
        let n = u32::from_le_bytes(self.bytes[self.at..self.at + 4].try_into().unwrap());
        self.at += 4;
        n
    }
    fn raw(&mut self) -> f64 {
        let n = f64::from_le_bytes(self.bytes[self.at..self.at + 8].try_into().unwrap());
        self.at += 8;
        n
    }
    fn number(&mut self, error: &'static str, nan: bool) -> Result<f64> {
        let n = self.raw();
        if n.is_finite() || nan && n.is_nan() {
            Ok(n)
        } else {
            Err(error)
        }
    }
    fn numbers(
        &mut self,
        count: usize,
        error: &'static str,
        nan_index: Option<usize>,
    ) -> Result<Vec<f64>> {
        (0..count)
            .map(|i| self.number(error, nan_index == Some(i)))
            .collect()
    }
    fn vector<const N: usize>(&mut self, error: &'static str) -> Result<[f64; N]> {
        let mut values = [0.0; N];
        for n in &mut values {
            *n = self.number(error, false)?;
        }
        Ok(values)
    }
}
fn present(flags: &[bool], index: usize) -> bool {
    flags.get(index).copied().unwrap_or(false)
}
fn safe_integer(n: f64) -> bool {
    n.is_finite() && n.fract() == 0.0 && n >= 0.0 && n <= SAFE
}
fn orientation(q: &[f64; 4], error: &'static str) -> Result<()> {
    let sum = q.iter().fold(0.0, |sum, n| sum + n * n);
    if (sum - 1.0).abs() > 1e-6 {
        Err(error)
    } else {
        Ok(())
    }
}

/// Wire tags preserve null and undefined independently of the numeric payload.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeatField {
    pub kind: u8,
    pub value: f64,
}
fn seats(
    r: &mut Reader<'_>,
    number_error: &'static str,
    tag_error: &'static str,
) -> Result<[SeatField; 6]> {
    let mut fields = [SeatField {
        kind: 0,
        value: 0.0,
    }; 6];
    for field in &mut fields {
        let kind = r.byte();
        let value = r.number(number_error, false)?;
        if kind > 2 {
            return Err(tag_error);
        }
        *field = SeatField { kind, value };
    }
    Ok(fields)
}
fn seat_words(fields: &[SeatField; 6], out: &mut Vec<f64>) {
    for field in fields {
        out.extend([field.kind as f64, field.value]);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WeaponState {
    pub level: u8,
    pub flags: u8,
    pub numbers: [f64; 20],
}
impl WeaponState {
    pub fn words(&self) -> Vec<f64> {
        let mut out = vec![self.level as f64, self.flags as f64];
        out.extend(self.numbers);
        out
    }
}
pub fn decode_weapon(bytes: Option<&[u8]>) -> Result<WeaponState> {
    let bytes = bytes
        .filter(|b| b.len() == 162 && b[1] <= 127)
        .ok_or("invalid weapon state payload")?;
    let mut r = Reader::new(bytes);
    let level = r.byte();
    let flags = r.byte();
    let mut numbers = [0.0; 20];
    for (i, n) in numbers.iter_mut().enumerate() {
        *n = r.raw();
        if !n.is_finite() && !(i == 2 && *n == f64::INFINITY) && !(i == 7 && n.is_nan()) {
            return Err("invalid weapon state number");
        }
    }
    Ok(WeaponState {
        level,
        flags,
        numbers,
    })
}

/// The vectors are in schema order. Compact state adds settled only when its
/// flag is present; a caller applies its local history reset after validation.
#[derive(Clone, Debug, PartialEq)]
pub struct SoldierState {
    pub schema: u8,
    pub flags: u8,
    pub numbers: Vec<f64>,
    pub vectors: Vec<[f64; 3]>,
    pub posture: u8,
    pub air: u8,
    pub dive_from: u8,
    pub grounded: bool,
    pub step_blocked: bool,
}
impl SoldierState {
    pub fn words(&self) -> Vec<f64> {
        let mut out = Vec::new();
        if self.schema == 6 {
            out.extend([
                self.flags as f64,
                self.posture as f64,
                self.air as f64,
                self.grounded as u8 as f64,
            ]);
        }
        out.extend(&self.numbers);
        for v in &self.vectors {
            out.extend(v);
        }
        if self.schema == 6 {
            out.push(self.dive_from as f64);
        } else {
            out.extend([
                self.posture as f64,
                self.air as f64,
                self.dive_from as f64,
                self.grounded as u8 as f64,
                self.step_blocked as u8 as f64,
            ]);
        }
        out
    }
}
/// `present` is position, velocity, normal, settled, previous, contactMean.
pub fn decode_compact_soldier(bytes: Option<&[u8]>, present: &[bool; 6]) -> Result<SoldierState> {
    let bytes = bytes
        .filter(|b| b.len() >= 124 && b[0] <= 3 && b[1] <= 2 && b[2] <= 4 && b[3] <= 1)
        .ok_or("invalid compact Soldier correction")?;
    let flags = bytes[0];
    let settled = flags & 1 != 0;
    let diving = flags & 2 != 0;
    if bytes.len() != 124 + usize::from(settled) * 24 + usize::from(diving) {
        return Err("invalid compact Soldier length");
    }
    let mut r = Reader { bytes, at: 4 };
    let numbers = r.numbers(6, "nonfinite Soldier correction", None)?;
    if (numbers[3] != 0.0) != diving {
        return Err("inconsistent dive state");
    }
    let mut vectors = Vec::new();
    for (i, error) in [
        "missing Soldier vector position",
        "missing Soldier vector velocity",
        "missing Soldier vector normal",
        "missing Soldier vector settled",
    ]
    .into_iter()
    .enumerate()
    {
        if i == 3 && !settled {
            break;
        }
        if !present[i] {
            return Err(error);
        }
        vectors.push(r.vector("nonfinite Soldier correction")?);
    }
    let dive_from = if diving { r.byte() } else { bytes[1] };
    if dive_from > 2 || !present[4] || !present[3] || !present[5] {
        return Err("invalid Soldier prerequisite");
    }
    Ok(SoldierState {
        schema: 6,
        flags,
        numbers,
        vectors,
        posture: bytes[1],
        air: bytes[2],
        dive_from,
        grounded: bytes[3] != 0,
        step_blocked: false,
    })
}
/// Schemas 1 and 4 retain the full and solver-only vector sets respectively.
pub fn decode_soldier(
    bytes: Option<&[u8]>,
    schema: f64,
    vector_present: &[bool],
) -> Result<SoldierState> {
    let names: Vec<usize> = if schema == 1.0 {
        (0..27).collect()
    } else if schema == 4.0 {
        vec![0, 1, 2, 3, 4, 5, 22]
    } else {
        return Err("unsupported soldier correction");
    };
    let bytes = bytes
        .filter(|b| b.len() == (20 + names.len() * 3) * 8 + 5)
        .ok_or("unsupported soldier correction")?;
    let mut r = Reader::new(bytes);
    let numbers = r.numbers(20, "nonfinite soldier correction", None)?;
    let mut vectors = Vec::new();
    for (i, &name) in names.iter().enumerate() {
        if !present(vector_present, i) {
            return Err(SOLDIER_MISSING[name]);
        }
        vectors.push(r.vector("nonfinite soldier correction")?);
    }
    let mut enums = [0; 3];
    for (n, count) in enums.iter_mut().zip([3, 5, 3]) {
        *n = r.byte();
        if *n >= count {
            return Err("invalid correction enum");
        }
    }
    let grounded = r.byte();
    if grounded > 1 {
        return Err("invalid correction boolean");
    }
    let step = r.byte();
    if step > 1 {
        return Err("invalid correction boolean");
    }
    Ok(SoldierState {
        schema: schema as u8,
        flags: 0,
        numbers,
        vectors,
        posture: enums[0],
        air: enums[1],
        dive_from: enums[2],
        grounded: grounded != 0,
        step_blocked: step != 0,
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct DroidekaState {
    pub mode: u8,
    pub flags: u8,
    pub numbers: [f64; 12],
    pub vectors: [[f64; 3]; 3],
    pub shield: Option<f64>,
    pub last_gun: Option<[f64; 2]>,
}
impl DroidekaState {
    pub fn words(&self) -> Vec<f64> {
        let mut out = vec![self.mode as f64, self.flags as f64];
        out.extend(self.numbers);
        for v in self.vectors {
            out.extend(v);
        }
        if let Some(value) = self.shield {
            out.push(value);
        }
        if let Some(last) = self.last_gun {
            out.extend(last);
        }
        out
    }
}
pub fn decode_droideka(bytes: Option<&[u8]>) -> Result<DroidekaState> {
    let bytes = bytes
        .filter(|b| b.len() >= 170)
        .ok_or("invalid droideka state length")?;
    let mode = bytes[0];
    let flags = bytes[1];
    if mode > 13
        || flags > 63
        || flags & 16 != 0 && flags & 8 == 0
        || bytes.len() != 170 + usize::from(flags & 8 != 0) * 8 + usize::from(flags & 32 != 0) * 16
    {
        return Err("invalid droideka state");
    }
    let mut r = Reader { bytes, at: 2 };
    let numbers = r.vector("nonfinite droideka state")?;
    let vectors = [
        r.vector("nonfinite droideka state")?,
        r.vector("nonfinite droideka state")?,
        r.vector("nonfinite droideka state")?,
    ];
    let shield = if flags & 8 != 0 {
        Some(r.number("nonfinite droideka state", false)?)
    } else {
        None
    };
    let last_gun = if flags & 32 != 0 {
        Some(r.vector::<2>("nonfinite droideka state")?)
    } else {
        None
    };
    if !safe_integer(numbers[11])
        || last_gun.is_some_and(|v| !safe_integer(v[0]) || !safe_integer(v[1]))
        || shield.is_some_and(|v| v < 0.0)
    {
        return Err("invalid droideka state");
    }
    Ok(DroidekaState {
        mode,
        flags,
        numbers,
        vectors,
        shield,
        last_gun,
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct GroundState {
    pub numbers: [f64; 11],
    pub position: [f64; 3],
    pub velocity: [f64; 3],
    pub quaternion: [f64; 4],
    pub alive: bool,
    pub sleeping: bool,
    pub seat: [SeatField; 6],
}
impl GroundState {
    pub fn words(&self) -> Vec<f64> {
        let mut out = Vec::new();
        out.extend(self.numbers);
        out.extend(self.position);
        out.extend(self.velocity);
        out.extend(self.quaternion);
        out.extend([self.alive as u8 as f64, self.sleeping as u8 as f64]);
        seat_words(&self.seat, &mut out);
        out
    }
}
pub fn decode_ground(
    bytes: Option<&[u8]>,
    supported: bool,
    history: [bool; 2],
) -> Result<GroundState> {
    if !supported {
        return Err("unsupported ground correction");
    }
    let bytes = bytes
        .filter(|b| b.len() == 224)
        .ok_or("invalid ground correction length")?;
    let mut r = Reader::new(bytes);
    let numbers = r.vector("nonfinite ground correction")?;
    let position = r.vector("nonfinite ground correction")?;
    let velocity = r.vector("nonfinite ground correction")?;
    let quaternion = r.vector("nonfinite ground correction")?;
    orientation(&quaternion, "invalid ground orientation")?;
    let alive = r.byte();
    if alive > 1 {
        return Err("invalid ground flags");
    }
    let sleeping = r.byte();
    if sleeping > 1 {
        return Err("invalid ground flags");
    }
    let seat = seats(&mut r, "nonfinite ground correction", "invalid ground seat")?;
    if !history[0] || !history[1] {
        return Err("missing ground presentation history");
    }
    Ok(GroundState {
        numbers,
        position,
        velocity,
        quaternion,
        alive: alive != 0,
        sleeping: sleeping != 0,
        seat,
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct FlightState {
    pub compact: bool,
    pub numbers: Vec<f64>,
    pub vectors: Vec<[f64; 3]>,
    pub quaternions: Vec<[f64; 4]>,
    pub booleans: [u8; 10],
    pub enums: [u8; 6],
    pub seat: [SeatField; 6],
}
impl FlightState {
    pub fn words(&self) -> Vec<f64> {
        let mut out = Vec::new();
        if self.compact {
            out.extend(self.enums.map(f64::from));
        }
        out.extend(&self.numbers);
        for v in &self.vectors {
            out.extend(v);
        }
        for q in &self.quaternions {
            out.extend(q);
        }
        out.extend(self.booleans.map(f64::from));
        if !self.compact {
            out.extend(self.enums.map(f64::from));
        }
        seat_words(&self.seat, &mut out);
        out
    }
}
fn flight_enums(r: &mut Reader<'_>) -> Result<[u8; 6]> {
    let mut values = [0; 6];
    for (value, count) in values.iter_mut().zip(FLIGHT_ENUM_LENGTHS) {
        *value = r.byte();
        if *value >= count {
            return Err("invalid flight enum");
        }
    }
    Ok(values)
}
fn flight_booleans(r: &mut Reader<'_>) -> Result<[u8; 10]> {
    let mut values = [0; 10];
    for value in &mut values {
        *value = r.byte();
        if *value > 2 {
            return Err("invalid flight boolean");
        }
    }
    Ok(values)
}
/// Object presence follows COMPACT_FLIGHT_OBJECTS; history is previous,
/// tickFrom, flightPreviousQ and flightTickQ. Inactive objects are not checked.
pub fn decode_compact_flight(
    bytes: Option<&[u8]>,
    supported: bool,
    objects: [bool; 9],
    history: [bool; 4],
) -> Result<FlightState> {
    if !supported {
        return Err("unsupported compact flight correction");
    }
    let bytes = bytes
        .filter(|b| b.len() >= 6)
        .ok_or("invalid compact flight payload")?;
    let mut r = Reader::new(bytes);
    let enums = flight_enums(&mut r)?;
    let transition = enums[0] == 1 || enums[0] == 3;
    let trick = enums[1];
    let number_count = 29 + usize::from(trick != 0) * 2;
    let mut vectors = vec![(0, 0), (1, 2)];
    if transition {
        vectors.extend([(2, 4), (3, 5)]);
    }
    if trick == 1 {
        vectors.push((4, 6));
    }
    let mut quaternions = vec![(5, 7)];
    if transition {
        quaternions.push((6, 10));
    }
    if trick == 3 {
        quaternions.extend([(7, 11), (8, 12)]);
    }
    if bytes.len() != 6 + number_count * 8 + vectors.len() * 24 + quaternions.len() * 32 + 10 + 54 {
        return Err("invalid compact flight length");
    }
    let numbers = r.numbers(number_count, "nonfinite flight correction", Some(13))?;
    let mut vector_values = Vec::new();
    let mut quaternion_values = Vec::new();
    for (i, error) in vectors {
        if !objects[i] {
            return Err(FLIGHT_MISSING[error]);
        }
        vector_values.push(r.vector("nonfinite flight correction")?);
    }
    for (i, error) in quaternions {
        if !objects[i] {
            return Err(FLIGHT_MISSING[error]);
        }
        let q = r.vector("nonfinite flight correction")?;
        orientation(&q, "invalid flight orientation")?;
        quaternion_values.push(q);
    }
    let booleans = flight_booleans(&mut r)?;
    let seat = seats(&mut r, "nonfinite flight correction", "invalid flight seat")?;
    for (valid, error) in history.into_iter().zip([
        "missing flight history previous",
        "missing flight history tickFrom",
        "missing flight history flightPreviousQ",
        "missing flight history flightTickQ",
    ]) {
        if !valid {
            return Err(error);
        }
    }
    Ok(FlightState {
        compact: true,
        numbers,
        vectors: vector_values,
        quaternions: quaternion_values,
        booleans,
        enums,
        seat,
    })
}
/// Schema 2 accepts non-unit quaternion values, matching its original reader.
pub fn decode_flight(
    bytes: Option<&[u8]>,
    schema: f64,
    supported: bool,
    objects: [bool; 13],
    pilot_seat: bool,
) -> Result<FlightState> {
    if schema != 2.0 || !supported {
        return Err("unsupported flight correction");
    }
    let bytes = bytes
        .filter(|b| b.len() == 726)
        .ok_or("unsupported flight correction")?;
    let mut r = Reader::new(bytes);
    let numbers = r.numbers(37, "nonfinite flight correction", Some(16))?;
    let mut vectors = Vec::new();
    let mut quaternions = Vec::new();
    for i in 0..13 {
        if !objects[i] {
            return Err(FLIGHT_MISSING[i]);
        }
        if i < 7 {
            vectors.push(r.vector("nonfinite flight correction")?);
        } else {
            quaternions.push(r.vector("nonfinite flight correction")?);
        }
    }
    let booleans = flight_booleans(&mut r)?;
    let enums = flight_enums(&mut r)?;
    let seat = seats(
        &mut r,
        "nonfinite flight correction",
        "invalid flight seat field",
    )?;
    if !pilot_seat {
        return Err("missing pilot seat");
    }
    Ok(FlightState {
        compact: false,
        numbers,
        vectors,
        quaternions,
        booleans,
        enums,
        seat,
    })
}

/// Payload ranges retain the caller's slice semantics, including Buffer views.
#[derive(Clone, Debug, PartialEq)]
pub struct MountedWeapon {
    pub barrel: f64,
    pub offset: usize,
    pub length: usize,
}
#[derive(Clone, Debug, PartialEq)]
pub struct MountedState {
    pub mode: u8,
    pub id: u32,
    pub generation: u32,
    pub index: u8,
    pub flags: u8,
    pub values: Vec<f64>,
    pub weapons: Vec<MountedWeapon>,
}
impl MountedState {
    pub fn words(&self) -> Vec<f64> {
        let mut out = vec![
            self.mode as f64,
            self.id as f64,
            self.generation as f64,
            self.index as f64,
            self.flags as f64,
            self.values.len() as f64,
        ];
        out.extend(&self.values);
        out.push(self.weapons.len() as f64);
        for w in &self.weapons {
            out.extend([w.barrel, w.offset as f64, w.length as f64]);
        }
        out
    }
}
pub fn decode_mounted(bytes: Option<&[u8]>) -> Result<MountedState> {
    let bytes = bytes
        .filter(|b| b.len() >= 11)
        .ok_or("truncated mounted control")?;
    let mut r = Reader::new(bytes);
    let mode = r.byte();
    let id = r.u32();
    let generation = r.u32();
    let index = r.byte();
    let flags = r.byte();
    if ![2, 3, 4].contains(&mode)
        || id == 0
        || generation == 0
        || flags > 3
        || mode != 2 && (flags != 0 || index != 0)
        || mode == 2 && index == 0
    {
        return Err("invalid mounted control");
    }
    let value_count = if mode == 4 {
        0
    } else if mode == 3 {
        6
    } else {
        2 + usize::from(flags & 1 != 0) + usize::from(flags & 2 != 0)
    };
    let count_at = 11 + value_count * 8;
    if bytes.len() < count_at + 1 {
        return Err("invalid mounted control length");
    }
    let count = bytes[count_at];
    if count > 2 || mode == 3 && count > 1 {
        return Err("invalid mounted control length");
    }
    let values = r.numbers(value_count, "nonfinite mounted control", None)?;
    r.at = count_at + 1;
    let mut weapons = Vec::new();
    for _ in 0..count {
        if r.at + 10 > bytes.len() {
            return Err("invalid mounted control length");
        }
        let barrel = r.raw();
        if !safe_integer(barrel) {
            return Err("invalid mounted barrel");
        }
        let length = r.u16();
        if r.at + length > bytes.len() {
            return Err("invalid mounted control length");
        }
        weapons.push(MountedWeapon {
            barrel,
            offset: r.at,
            length,
        });
        r.at += length;
    }
    if r.at != bytes.len() {
        return Err("invalid mounted control length");
    }
    Ok(MountedState {
        mode,
        id,
        generation,
        index,
        flags,
        values,
        weapons,
    })
}

/// A span in the original correction payload. It retains no borrowed storage;
/// callers can preserve their own copy or view semantics when resolving it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PayloadRange {
    pub offset: usize,
    pub length: usize,
}
impl PayloadRange {
    pub fn get<'a>(&self, bytes: &'a [u8]) -> Option<&'a [u8]> {
        bytes.get(self.offset..self.offset.checked_add(self.length)?)
    }
    fn words(&self, out: &mut Vec<f64>) {
        out.extend([self.offset as f64, self.length as f64]);
    }
}

const OWN_FIELD: &str = "own-player envelope exceeds budget";
const OWN_SLICE: &str = "truncated own-player payload";
// The envelope has no fixed accepted size. Each field checks its own bounds,
// while counted slices use the original reader's distinct truncation error.
struct OwnReader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> OwnReader<'a> {
    fn fixed<const N: usize>(&mut self) -> Result<[u8; N]> {
        let end = self.at.checked_add(N).ok_or(OWN_FIELD)?;
        let source = self.bytes.get(self.at..end).ok_or(OWN_FIELD)?;
        let mut bytes = [0; N];
        bytes.copy_from_slice(source);
        self.at = end;
        Ok(bytes)
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.fixed::<1>()?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.fixed()?))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.fixed()?))
    }
    fn number(&mut self) -> Result<f64> {
        let n = f64::from_le_bytes(self.fixed()?);
        if n.is_finite() {
            Ok(n)
        } else {
            Err("invalid own-player number")
        }
    }
    fn take(&mut self, length: usize) -> Result<PayloadRange> {
        let end = self.at.checked_add(length).ok_or(OWN_SLICE)?;
        if end > self.bytes.len() {
            return Err(OWN_SLICE);
        }
        let range = PayloadRange {
            offset: self.at,
            length,
        };
        self.at = end;
        Ok(range)
    }
    fn counted(&mut self) -> Result<PayloadRange> {
        let length = self.u16()? as usize;
        self.take(length)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OwnWeapon {
    pub id: u16,
    pub payload: PayloadRange,
}
#[derive(Clone, Debug, PartialEq)]
pub struct OwnMounted {
    pub payload: PayloadRange,
    /// Nested weapon offsets are relative to the complete envelope, matching
    /// every other PayloadRange in OwnPlayerState.
    pub state: MountedState,
}
#[derive(Clone, Debug, PartialEq)]
pub struct OwnRemote {
    pub id: u16,
    pub generation: u32,
    pub definition_id: u16,
    pub payload: PayloadRange,
}
#[derive(Clone, Debug, PartialEq)]
pub struct OwnFlightSettings {
    pub invert: bool,
    pub sensitivity: Option<f64>,
    pub curve: Option<f64>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct OwnGamepad {
    pub axes: [f64; 4],
    pub buttons: u16,
}
#[derive(Clone, Debug, PartialEq)]
pub struct OwnView {
    pub flags: u16,
    pub invert_mouse: bool,
    pub first_person: bool,
    pub mouse_scale: Option<f64>,
    pub aspect: Option<f64>,
    pub view_tick: Option<f64>,
    pub flight_settings: Option<OwnFlightSettings>,
    pub gamepad: Option<OwnGamepad>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct OwnPlayerState {
    pub epoch: u32,
    pub content_epoch: u32,
    pub class_id: u16,
    pub team: u8,
    pub body_schema: u8,
    pub vehicle_id: u32,
    pub vehicle_generation: u32,
    pub body: PayloadRange,
    pub health: f64,
    pub dead: bool,
    pub death_time: f64,
    pub unit_dead_time: f64,
    pub removed: bool,
    /// Recoil rate, hold and offset, in that order.
    pub kick: [f64; 3],
    pub zoom_step: u8,
    pub zoom: f64,
    pub first_person: bool,
    pub gun_underwater: bool,
    pub selected: [u8; 2],
    pub weapons: Vec<OwnWeapon>,
    pub held_bits: PayloadRange,
    pub binding_profile_id: u16,
    pub has_bindings: bool,
    pub discarded_input: u32,
    pub control_mode: u8,
    pub mounted: Option<OwnMounted>,
    pub remote: Option<OwnRemote>,
    pub view: OwnView,
}
impl OwnPlayerState {
    /// Scalar transport follows wire order, replacing every source slice with
    /// offset and length. View flags select the same optional scalar fields.
    /// Nested mounted state is already validated; its raw span is sufficient
    /// for the JavaScript caller, which returns that span without decoding it.
    pub fn words(&self) -> Vec<f64> {
        let mut out = vec![
            self.epoch as f64,
            self.content_epoch as f64,
            self.class_id as f64,
            self.team as f64,
            self.body_schema as f64,
            self.vehicle_id as f64,
            self.vehicle_generation as f64,
        ];
        self.body.words(&mut out);
        out.extend([
            self.health,
            self.dead as u8 as f64,
            self.death_time,
            self.unit_dead_time,
            self.removed as u8 as f64,
        ]);
        out.extend(self.kick);
        out.extend([
            self.zoom_step as f64,
            self.zoom,
            (self.first_person as u8 | (self.gun_underwater as u8) << 1) as f64,
            self.selected[0] as f64,
            self.selected[1] as f64,
            self.weapons.len() as f64,
        ]);
        for weapon in &self.weapons {
            out.push(weapon.id as f64);
            weapon.payload.words(&mut out);
        }
        self.held_bits.words(&mut out);
        out.push(self.view.flags as f64);
        out.extend(self.view.mouse_scale);
        out.extend(self.view.aspect);
        out.extend(self.view.view_tick);
        out.push(self.binding_profile_id as f64);
        if let Some(flight) = &self.view.flight_settings {
            out.extend(flight.sensitivity);
            out.extend(flight.curve);
        }
        if let Some(gamepad) = &self.view.gamepad {
            out.extend(gamepad.axes);
            out.push(gamepad.buttons as f64);
        }
        out.extend([self.discarded_input as f64, self.control_mode as f64]);
        if let Some(mounted) = &self.mounted {
            mounted.payload.words(&mut out);
        }
        if let Some(remote) = &self.remote {
            out.extend([
                remote.id as f64,
                remote.generation as f64,
                remote.definition_id as f64,
            ]);
            remote.payload.words(&mut out);
        }
        out
    }
}

/// Decode the own-player envelope after a plain record's kind/schema check.
/// Body, arsenal and recon bytes remain opaque. Only mounted structure is
/// validated here, at its original position before trailing and view checks.
/// The writer's allocation size does not limit accepted envelope lengths.
pub fn decode_own_player(bytes: Option<&[u8]>, supported_record: bool) -> Result<OwnPlayerState> {
    if !supported_record {
        return Err("unsupported own-player correction");
    }
    let bytes = bytes.ok_or("unsupported own-player correction")?;
    let mut r = OwnReader { bytes, at: 0 };
    let epoch = r.u32()?;
    let content_epoch = r.u32()?;
    let class_id = r.u16()?;
    let team = r.byte()?;
    let body_schema = r.byte()?;
    let vehicle_id = r.u32()?;
    let vehicle_generation = r.u32()?;
    if !(1..=2).contains(&team) {
        return Err("invalid own team");
    }
    let body = r.counted()?;
    let health = r.number()?;
    let dead = r.byte()?;
    let death_time = r.number()?;
    let unit_dead_time = r.number()?;
    let removed = r.byte()?;
    if dead > 1 || removed > 1 {
        return Err("invalid life flags");
    }
    let kick = [r.number()?, r.number()?, r.number()?];
    let zoom_step = r.byte()?;
    let zoom = r.number()?;
    let first = r.byte()?;
    if first > 3 {
        return Err("invalid weapon view flags");
    }
    let selected = [r.byte()?, r.byte()?];
    let count = r.byte()?;
    let mut weapons = Vec::with_capacity(count as usize);
    for _ in 0..count {
        weapons.push(OwnWeapon {
            id: r.u16()?,
            payload: r.counted()?,
        });
    }
    let held_bits = r.take(16)?;
    let flags = r.u16()?;
    if flags > 2047 {
        return Err("unknown own-player flags");
    }
    let mouse_scale = if flags & 32 != 0 {
        Some(r.number()?)
    } else {
        None
    };
    let aspect = if flags & 64 != 0 {
        Some(r.number()?)
    } else {
        None
    };
    let view_tick = if flags & 128 != 0 {
        Some(r.number()?)
    } else {
        None
    };
    let binding_profile_id = r.u16()?;
    let mut flight_settings = (flags & 4 != 0).then_some(OwnFlightSettings {
        invert: flags & 8 != 0,
        sensitivity: None,
        curve: None,
    });
    if flags & 256 != 0 {
        let flight = flight_settings.as_mut().ok_or("invalid flight context")?;
        flight.sensitivity = Some(r.number()?);
    }
    if flags & 512 != 0 {
        let flight = flight_settings.as_mut().ok_or("invalid flight context")?;
        flight.curve = Some(r.number()?);
    }
    let has_bindings = flags & 1024 != 0;
    if has_bindings && flight_settings.is_none() {
        return Err("invalid flight bindings context");
    }
    let gamepad = if flags & 16 != 0 {
        Some(OwnGamepad {
            axes: [r.number()?, r.number()?, r.number()?, r.number()?],
            buttons: r.u16()?,
        })
    } else {
        None
    };
    let discarded_input = r.u32()?;
    let control_mode = r.byte()?;
    if control_mode > 4 {
        return Err("invalid own control mode");
    }
    let mounted = if control_mode >= 2 {
        let payload = r.counted()?;
        let mut state = decode_mounted(payload.get(bytes))?;
        if state.mode != control_mode {
            return Err("mounted control mode mismatch");
        }
        // The nested decoder validates every end against its source length,
        // so translating those offsets stays inside the complete envelope.
        for weapon in &mut state.weapons {
            weapon.offset += payload.offset;
        }
        Some(OwnMounted { payload, state })
    } else {
        None
    };
    let remote = if control_mode == 1 {
        let id = r.u16()?;
        let generation = r.u32()?;
        let definition_id = r.u16()?;
        if id >= 4096 || generation == 0 {
            return Err("invalid recon identity");
        }
        Some(OwnRemote {
            id,
            generation,
            definition_id,
            payload: r.counted()?,
        })
    } else {
        None
    };
    if r.at != bytes.len() {
        return Err("trailing own-player payload");
    }
    if view_tick.is_some_and(|n| n < 0.0 || n > 4294967295.0) {
        return Err("invalid replay view tick");
    }
    if mouse_scale.is_some_and(|n| n <= 0.0)
        || aspect.is_some_and(|n| n <= 0.0)
        || flight_settings
            .as_ref()
            .and_then(|flight| flight.sensitivity)
            .is_some_and(|n| n <= 0.0 || n > 10.0)
        || flight_settings
            .as_ref()
            .and_then(|flight| flight.curve)
            .is_some_and(|n| n < 0.1 || n > 4.0)
        || gamepad
            .as_ref()
            .is_some_and(|pad| pad.axes.iter().any(|n| n.abs() > 1.0))
    {
        return Err("invalid replay view");
    }
    Ok(OwnPlayerState {
        epoch,
        content_epoch,
        class_id,
        team,
        body_schema,
        vehicle_id,
        vehicle_generation,
        body,
        health,
        dead: dead != 0,
        death_time,
        unit_dead_time,
        removed: removed != 0,
        kick,
        zoom_step,
        zoom,
        first_person: first & 1 != 0,
        gun_underwater: first & 2 != 0,
        selected,
        weapons,
        held_bits,
        binding_profile_id,
        has_bindings,
        discarded_input,
        control_mode,
        mounted,
        remote,
        view: OwnView {
            flags,
            invert_mouse: flags & 1 != 0,
            first_person: flags & 2 != 0,
            mouse_scale,
            aspect,
            view_tick,
            flight_settings,
            gamepad,
        },
    })
}

/// Scalar transport for the optional shared-bridge adapter. Input is metadata
/// length/values, Uint8Array flag, and byte length/values. All output is owned.
#[derive(Default)]
pub struct Session {
    pub input: Vec<f64>,
    pub output: Vec<f64>,
    error: Option<&'static str>,
}
impl Session {
    pub fn run(&mut self, op: u32) -> Result<()> {
        self.output.clear();
        let input = &self.input;
        let count = |n: f64| -> Result<usize> {
            if n.is_finite() && n >= 0.0 && n.fract() == 0.0 && n <= input.len() as f64 {
                Ok(n as usize)
            } else {
                Err(EXCHANGE)
            }
        };
        let n = count(*input.first().ok_or(EXCHANGE)?)?;
        let meta = input.get(1..n + 1).ok_or(EXCHANGE)?;
        let valid = *input.get(n + 1).ok_or(EXCHANGE)? == 1.0;
        let len = count(*input.get(n + 2).ok_or(EXCHANGE)?)?;
        if input.len() != n + 3 + len {
            return Err(EXCHANGE);
        }
        let bytes = input[n + 3..]
            .iter()
            .map(|&n| jsrt::to_uint32(n) as u8)
            .collect::<Vec<_>>();
        let bytes = if valid { Some(bytes.as_slice()) } else { None };
        let number = |at: usize| meta.get(at).copied().ok_or(EXCHANGE);
        let flag = |at: usize| number(at).map(|x| x == 1.0);
        fn flags<const N: usize>(meta: &[f64], at: usize) -> Result<[bool; N]> {
            let values = meta.get(at..at + N).ok_or(EXCHANGE)?;
            Ok(std::array::from_fn(|i| values[i] == 1.0))
        }
        self.output = match op {
            0 => decode_weapon(bytes)?.words(),
            1 => decode_compact_soldier(bytes, &flags(meta, 0)?)?.words(),
            2 => decode_soldier(
                bytes,
                number(0)?,
                &meta[1..].iter().map(|&x| x == 1.0).collect::<Vec<_>>(),
            )?
            .words(),
            3 => decode_droideka(bytes)?.words(),
            4 => decode_ground(bytes, flag(0)?, flags(meta, 1)?)?.words(),
            5 => decode_compact_flight(bytes, flag(0)?, flags(meta, 1)?, flags(meta, 10)?)?.words(),
            6 => decode_flight(bytes, number(0)?, flag(1)?, flags(meta, 2)?, flag(15)?)?.words(),
            7 => decode_mounted(bytes)?.words(),
            8 => decode_own_player(bytes, flag(0)?)?.words(),
            _ => return Err(EXCHANGE),
        };
        Ok(())
    }
}
#[no_mangle]
pub extern "C" fn prediction_corrections_create() -> *mut Session {
    Box::into_raw(Box::new(Session::default()))
}
#[no_mangle]
pub unsafe extern "C" fn prediction_corrections_drop(p: *mut Session) {
    if !p.is_null() {
        drop(Box::from_raw(p));
    }
}
#[no_mangle]
pub unsafe extern "C" fn prediction_corrections_room(p: *mut Session, n: usize) -> *mut f64 {
    let s = &mut *p;
    s.input.resize(n, 0.0);
    s.input.as_mut_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn prediction_corrections_run(p: *mut Session, op: u32) -> u32 {
    let s = &mut *p;
    s.error = s.run(op).err();
    s.error.is_none() as u32
}
#[no_mangle]
pub unsafe extern "C" fn prediction_corrections_output(p: *const Session) -> *const f64 {
    (&*p).output.as_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn prediction_corrections_output_len(p: *const Session) -> usize {
    (&*p).output.len()
}
#[no_mangle]
pub unsafe extern "C" fn prediction_corrections_error(p: *const Session) -> *const u8 {
    (&*p).error.unwrap_or("").as_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn prediction_corrections_error_len(p: *const Session) -> usize {
    (&*p).error.unwrap_or("").len()
}
