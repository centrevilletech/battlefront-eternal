//! World summary refresh cadence, preserving first-offer Map order and
//! identity skips, including fields seen before their first due tick.
use crate::gen::{WireField, WireStaggerTake};
use jsrt::{same_value_zero, to_uint32};
#[derive(Clone)]
pub struct Entry {
    pub schema: f64,
    pub id: f64,
    pub token: f64,
    pub field: Option<WireField>,
    pub state_team: Option<Vec<u16>>,
    pub round: f64,
    pub due: f64,
}
#[derive(Default)]
pub struct Stagger {
    pub entries: Vec<Entry>,
    pub cadence: Vec<(f64, f64)>,
    pub round: f64,
}
impl Stagger {
    pub fn new() -> Self {
        Self {
            cadence: vec![
                (1., 6.),
                (2., 6.),
                (4., 60.),
                (5., 300.),
                (6., 30.),
                (7., 48.),
                (8., 12.),
                (9., 60.),
                (10., 60.),
            ],
            ..Self::default()
        }
    }
    fn find(&self, schema: f64, id: f64) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|e| same_value_zero(e.schema, schema) && same_value_zero(e.id, id))
    }
    pub fn take(&mut self, a: WireStaggerTake) {
        self.round += 1.;
        let round = self.round;
        for record in a.records {
            let f = record.field;
            let schema = f.schema;
            let cadence = self
                .cadence
                .iter()
                .find(|(s, _)| *s == schema)
                .map_or(6., |x| x.1);
            let at = match self
                .entries
                .iter()
                .position(|e| same_value_zero(e.schema, schema) && same_value_zero(e.id, f.id))
            {
                Some(i) => i,
                None => {
                    self.entries.push(Entry {
                        schema,
                        id: f.id,
                        token: 0.,
                        field: None,
                        state_team: None,
                        round,
                        due: if schema == 9. || schema == 10. {
                            a.tick
                        } else {
                            a.tick + (to_uint32(f.id * 2654435761.) as f64) % cadence
                        },
                    });
                    self.entries.len() - 1
                }
            };
            let e = &mut self.entries[at];
            e.round = round;
            if record.token == e.token && a.tick < e.due {
                continue;
            }
            let state_team = if schema == 7. && e.field.is_some() {
                super::json::building_state_team(&f.payload)
            } else {
                None
            };
            let immediate = schema == 7. && e.field.is_some() && state_team != e.state_team
                || (schema == 9. || schema == 10.)
                    && e.field.as_ref().is_some_and(|b| b.payload != f.payload);
            if immediate || a.tick >= e.due {
                e.token = record.token;
                e.due = a.tick + cadence;
                if schema == 7. {
                    e.state_team =
                        state_team.or_else(|| super::json::building_state_team(&f.payload));
                }
                e.field = Some(f);
            }
        }
        self.entries.retain(|e| {
            e.round == round
                || !e
                    .field
                    .as_ref()
                    .is_some_and(|f| a.complete.contains(&f.schema))
        });
    }
    pub fn tokens(&self) -> Vec<f64> {
        self.entries
            .iter()
            .filter(|e| e.field.is_some())
            .map(|e| e.token)
            .collect()
    }
    pub fn world(&self) -> Vec<WireField> {
        self.entries
            .iter()
            .filter_map(|e| e.field.clone())
            .collect()
    }
    /// -1 asks for fresh capture; zero is a held field not taken yet.
    pub fn held(&self, tick: f64, schema: f64, id: f64, state_team: Option<&[u16]>) -> f64 {
        let Some(e) = self.find(schema, id) else {
            return -1.;
        };
        if tick >= e.due
            || e.field.is_some()
                && (if schema == 7. {
                    state_team != e.state_team.as_deref()
                } else {
                    schema == 9. || schema == 10.
                })
        {
            -1.
        } else {
            e.token
        }
    }
    pub fn due(&self, tick: f64, schema: f64, id: f64) -> bool {
        self.find(schema, id).is_none_or(|e| tick >= e.due)
    }
    pub fn any_due(&self, tick: f64, schemas: &[f64]) -> bool {
        self.entries.is_empty()
            || self
                .entries
                .iter()
                .any(|e| schemas.contains(&e.schema) && e.field.is_some() && tick >= e.due)
    }
    pub fn schema_due(&self, tick: f64, schemas: &[f64]) -> bool {
        let mut seen = false;
        for e in &self.entries {
            if schemas.contains(&e.schema) {
                seen = true;
                if tick >= e.due {
                    return true;
                }
            }
        }
        !seen
    }
}
use crate::{
    abi::sim,
    gen::{WireCadences, WireHostList},
    store::StrId,
};
fn list(values: Vec<f64>) -> *const f64 {
    let out = WireHostList { values };
    let s = sim();
    s.calls
        .result(WireHostList::WORDS, |row, heap| out.put(row, heap));
    s.calls.block.ptr()
}
#[no_mangle]
pub extern "C" fn wire_stagger_create() -> *mut Stagger {
    Box::into_raw(Box::new(Stagger::new()))
}
#[no_mangle]
pub unsafe extern "C" fn wire_stagger_drop(p: *mut Stagger) {
    if !p.is_null() {
        drop(Box::from_raw(p));
    }
}
#[no_mangle]
pub extern "C" fn wire_stagger_words(op: u32) -> u32 {
    match op {
        0 => WireStaggerTake::WORDS as u32,
        1 => WireHostList::WORDS as u32,
        2 => WireCadences::WORDS as u32,
        _ => 0,
    }
}
#[no_mangle]
pub unsafe extern "C" fn wire_stagger_take(p: *mut Stagger) -> *const f64 {
    let a = {
        let (row, heap) = sim().calls.args();
        WireStaggerTake::take(row, heap)
    };
    let s = &mut *p;
    s.take(a);
    list(s.tokens())
}
#[no_mangle]
pub unsafe extern "C" fn wire_stagger_cadence(p: *mut Stagger) {
    let a = {
        let (row, heap) = sim().calls.args();
        WireCadences::take(row, heap)
    };
    (&mut *p).cadence = a.values.into_iter().map(|v| (v.schema, v.ticks)).collect();
}
#[no_mangle]
pub unsafe extern "C" fn wire_stagger_held(
    p: *const Stagger,
    tick: f64,
    schema: f64,
    id: f64,
    state: i32,
) -> f64 {
    let state = if state < 0 {
        None
    } else {
        Some(sim().strings.units(StrId(state as u32)))
    };
    (&*p).held(tick, schema, id, state)
}
#[no_mangle]
pub unsafe extern "C" fn wire_stagger_due(
    p: *const Stagger,
    tick: f64,
    schema: f64,
    id: f64,
) -> u32 {
    (&*p).due(tick, schema, id) as u32
}
#[no_mangle]
pub unsafe extern "C" fn wire_stagger_schemas(p: *const Stagger, tick: f64, any: u32) -> u32 {
    let a = {
        let (row, heap) = sim().calls.args();
        WireHostList::take(row, heap)
    };
    if any != 0 {
        (&*p).any_due(tick, &a.values) as u32
    } else {
        (&*p).schema_due(tick, &a.values) as u32
    }
}
