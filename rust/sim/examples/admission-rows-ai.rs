//! JSON-lines cold AI row dump. Each line supplies authored definitions
//! and the earlier owners' canonical identity receipts, never expected row
//! words. It emits the recorder's export/args/intern fields as f64 hex bits.
//! Accepts kind=content, setup or roster; a preceding kind=strings line
//! seeds the common UTF-16 interner with its earlier admission identities.
//! --check consumes the same source fixtures with an `expected` recording
//! call on each line. Expected words reach only the comparator; source facts
//! and the preceding string prefix drive the generated record writers.
use bfe_content::{json, value::Value};
use bfe_sim::gen::*;
use bfe_sim::store::Strings;
use bfe_sim::world::admit::{
    rows::{ai::*, Call},
    Options,
};
use serde_json::json;
use std::io::BufRead;

fn required<'a>(v: &'a Value, key: &str) -> Result<&'a Value, String> {
    v.get(key).ok_or_else(|| format!("missing {key}"))
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    required(v, key)?
        .as_str()
        .ok_or_else(|| format!("{key} must be text"))
}
fn number(v: &Value, key: &str) -> Result<f64, String> {
    let value = required(v, key)?;
    if value.as_str() == Some("__NaN") {
        return Ok(f64::NAN);
    }
    value
        .as_f64()
        .ok_or_else(|| format!("{key} must be numeric"))
}
fn uint(v: &Value, key: &str) -> Result<u32, String> {
    let n = number(v, key)?;
    if n.is_finite() && n >= 0.0 && n <= u32::MAX as f64 && n.fract() == 0.0 {
        Ok(n as u32)
    } else {
        Err(format!("{key} must be an unsigned integer"))
    }
}
fn entries<'a>(v: &'a Value, key: &str) -> Result<&'a [Value], String> {
    v.get(key)
        .map(|v| {
            v.as_array()
                .ok_or_else(|| format!("{key} must be an array"))
        })
        .unwrap_or(Ok(&[]))
}
fn numeric<const N: usize>(v: &Value, key: &str) -> Result<[f64; N], String> {
    let a = required(v, key)?
        .as_array()
        .filter(|a| a.len() == N)
        .ok_or_else(|| format!("{key} requires {N} numbers"))?;
    let mut out = [0.0; N];
    for (i, v) in a.iter().enumerate() {
        out[i] = v
            .as_f64()
            .ok_or_else(|| format!("{key}[{i}] must be numeric"))?;
    }
    Ok(out)
}
fn optional<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.get(key).filter(|v| !v.is_null())
}
fn member(v: &Value) -> Result<Option<Member>, String> {
    optional(v, "member")
        .map(|_| {
            let row = numeric::<3>(v, "member")?;
            if !(0..=3).contains(&(row[0] as i32)) || row[0].fract() != 0.0 {
                return Err("invalid Member variant".into());
            }
            Ok(Member::take(&row, &[]))
        })
        .transpose()
}
fn prepared<'a>(v: &'a Value, key: &str) -> Result<Vec<PreparedClass<'a>>, String> {
    entries(v, key)?
        .iter()
        .map(|v| {
            Ok(PreparedClass {
                name: text(v, "name")?,
                definition: required(v, "definition")?,
                id: DefId(uint(v, "id")?),
            })
        })
        .collect()
}
fn content(v: &Value, strings: &mut Strings) -> Result<Call, String> {
    let classes = prepared(v, "classes")?;
    let weapons = entries(v, "weapons")?
        .iter()
        .map(|v| {
            v.as_f64()
                .map(|n| DefId(n as u32))
                .ok_or("weapon id must be numeric".to_string())
        })
        .collect::<Result<_, _>>()?;
    let weapon_data = entries(v, "weaponData")?
        .iter()
        .map(|v| {
            Ok(AiWeaponData {
                json: text(v, "text")?.as_bytes().to_vec(),
            })
        })
        .collect::<Result<_, String>>()?;
    let items = entries(v, "items")?
        .iter()
        .map(|v| {
            Ok(ItemFacts {
                definition: required(v, "definition")?,
                id: DefId(uint(v, "id")?),
            })
        })
        .collect::<Result<_, String>>()?;
    let posts = entries(v, "posts")?
        .iter()
        .map(|v| {
            Ok(PostFacts {
                reference: SpawnPost::take(&numeric::<3>(v, "reference")?, &[]),
                name: v.get("name"),
                desc: required(v, "desc")?,
                ai: required(v, "ai")?,
            })
        })
        .collect::<Result<_, String>>()?;
    let hulls = entries(v, "hulls")?
        .iter()
        .map(|v| {
            let r = numeric::<2>(v, "hull")?;
            let seats = entries(v, "seats")?
                .iter()
                .map(|s| Ok((required(s, "settings")?, uint(s, "weapons")?)))
                .collect::<Result<_, String>>()?;
            Ok(HullFacts {
                hull: Ref::new(r[0] as u32, r[1] as u32),
                member: member(v)?,
                def: DefId(uint(v, "id")?),
                definition: required(v, "definition")?,
                name: text(v, "name")?,
                base: text(v, "base")?,
                target_class: required(v, "targetClass")?,
                category: number(v, "category")?,
                bounds: numeric(v, "bounds")?,
                max_health: number(v, "maxHealth")?,
                speed: number(v, "speed")?,
                turn_rate: number(v, "turnRate")?,
                slot_desc: optional(v, "slotDesc"),
                seats,
            })
        })
        .collect::<Result<_, String>>()?;
    let props = entries(v, "props")?
        .iter()
        .map(|v| {
            let o = numeric::<4>(v, "owner")?;
            let kind = F4ObjectKind::from_u8(o[0] as u8).ok_or("invalid F4 owner kind")?;
            Ok(PropFacts {
                desc: required(v, "desc")?,
                def: DefId(uint(v, "id")?),
                target: RepairTarget::take(&numeric::<3>(v, "target")?, &[]),
                owner: F4Handle {
                    kind,
                    slot: o[1] as u32,
                    generation: o[2] as u32,
                    life: o[3],
                },
                member: member(v)?,
                target_name: optional(v, "targetName").and_then(Value::as_str),
                target_class: optional(v, "targetClass"),
                team: number(v, "team")?,
                armed: required(v, "armed")?.truthy(),
                post: optional(v, "post")
                    .map(|_| numeric::<3>(v, "post").map(|r| SpawnPost::take(&r, &[])))
                    .transpose()?,
            })
        })
        .collect::<Result<_, String>>()?;
    let facts = ContentFacts {
        classes,
        weapons,
        weapon_data,
        items,
        posts,
        hulls,
        props,
    };
    let rows = content_rows(strings, &facts).map_err(|e| e.to_string())?;
    encoded_content(&rows, &facts).map_err(|e| e.to_string())
}
fn options(v: &Value) -> Result<Options, String> {
    Ok(Options {
        map: String::new(),
        side: String::new(),
        seed: uint(v, "seed")?,
        enhanced: v.get("enhanced").is_some_and(Value::truthy),
        heroes: v.get("heroes").is_some_and(Value::truthy),
        unit: None,
        name: None,
    })
}
fn setup(v: &Value, strings: &mut Strings) -> Result<Call, String> {
    let source = required(v, "facts")?;
    let facts = SetupFacts {
        stream_a: uint(source, "streamA")?,
        stream_b: uint(source, "streamB")?,
        state_a: uint(source, "stateA")?,
        state_b: uint(source, "stateB")?,
        next_unit_id: uint(source, "nextUnitId")?,
        hull_seats: entries(source, "hullSeats")?
            .iter()
            .map(|n| n.as_f64().map(|n| n as u32).ok_or("seat count".to_string()))
            .collect::<Result<_, _>>()?,
        buildings: uint(source, "buildings")?,
        classes: uint(source, "classes")?,
        posts: uint(source, "posts")?,
        navigation_posts: entries(source, "navigationPosts")?
            .iter()
            .map(|p| {
                let a = numeric::<3>(p, "position")?;
                Ok(AiNavPost {
                    post: SpawnPost::take(&numeric::<3>(p, "post")?, &[]),
                    name: strings.intern_str(text(p, "name")?),
                    position: a,
                })
            })
            .collect::<Result<_, String>>()?,
        unit_scale: number(source, "unitScale")?,
        tier: number(source, "tier")?,
    };
    let mut assets = AiAssets::default();
    if let Some(nav) = optional(v, "navigation") {
        assets.navigation = nav.as_str().ok_or("navigation text")?.as_bytes().to_vec();
    }
    assets.navigation_bin = entries(v, "navigationBin")?
        .iter()
        .map(|v| {
            v.as_f64()
                .map(|n| n as u8)
                .ok_or("navigation byte".to_string())
        })
        .collect::<Result<_, _>>()?;
    let row = setup_rows(
        strings,
        &options(v)?,
        number(v, "playerTeam")?,
        entries(v, "calls")?,
        &facts,
        &assets,
    )
    .map_err(|e| e.to_string())?;
    let mut call = Call {
        row: vec![0.0; AiWorldSetup::WORDS],
        ..Default::default()
    };
    row.put(&mut call.row, &mut call.heap);
    Ok(call)
}
fn pad_hull(v: &Value, strings: &mut dyn AiInterner) -> Result<PsHull, String> {
    let r = numeric::<2>(v, "vehicle")?;
    let record = optional(v, "record")
        .map(|v| -> Result<PsClass, String> {
            let list = |key| {
                optional(v, key)
                    .map(|_| {
                        entries(v, key)?
                            .iter()
                            .map(|n| n.as_f64().ok_or("pad class weight".to_string()))
                            .collect::<Result<Vec<_>, String>>()
                    })
                    .transpose()
            };
            let points = entries(v, "points")?
                .iter()
                .map(|p| {
                    let a = p.as_array().ok_or("pad point")?;
                    let mut point = [0.0; 4];
                    for (i, out) in point.iter_mut().enumerate() {
                        if let Some(v) = a.get(i).filter(|v| !v.is_null()) {
                            *out = v.as_f64().ok_or("pad point coordinate")?;
                        }
                    }
                    Ok(point)
                })
                .collect::<Result<_, String>>()?;
            Ok(PsClass {
                atk: list("atk")?,
                def: list("def")?,
                value_bleed: optional(v, "valueBleed")
                    .map(|_| number(v, "valueBleed"))
                    .transpose()?,
                points,
            })
        })
        .transpose()?;
    Ok(PsHull {
        vehicle: Ref::new(r[0] as u32, r[1] as u32),
        alive: required(v, "alive")?.truthy(),
        command: required(v, "command")?.truthy(),
        generation: number(v, "generation")?,
        team: number(v, "team")?,
        controlling_team: number(v, "controllingTeam")?,
        seated: required(v, "seated")?.truthy(),
        yaw: number(v, "yaw")?,
        name: strings.intern_str(text(v, "name")?),
        label: strings.intern_str(text(v, "label")?),
        radius: number(v, "radius")?,
        speed: number(v, "speed")?,
        value_bleed: number(v, "valueBleed")?,
        record,
    })
}
fn roster(v: &Value, strings: &mut Strings) -> Result<Call, String> {
    let prepared = prepared(v, "prepared")?;
    let row = roster_rows_with_pads(
        strings,
        &options(v)?,
        number(v, "playerTeam")?,
        number(v, "unitScale")?,
        entries(v, "calls")?,
        optional(v, "mission"),
        &prepared,
        |strings| {
            entries(v, "pads")
                .and_then(|pads| {
                    pads.iter()
                        .map(|p| {
                            Ok(F4PadSetup {
                                index: uint(p, "index")?,
                                pad: PsPad {
                                    variants: entries(p, "variants")?
                                        .iter()
                                        .map(|h| pad_hull(h, strings))
                                        .collect::<Result<_, String>>()?,
                                    vehicle: optional(p, "vehicle")
                                        .map(|h| pad_hull(h, strings))
                                        .transpose()?,
                                },
                            })
                        })
                        .collect::<Result<_, String>>()
                })
                .map_err(|message| bfe_sim::world::admit::AdmitError::Content {
                    stage: bfe_sim::world::admit::Stage::Ai,
                    message,
                })
        },
    )
    .map_err(|e| e.to_string())?;
    let mut call = Call {
        row: vec![0.0; AiRosterSetup::WORDS],
        ..Default::default()
    };
    row.put(&mut call.row, &mut call.heap);
    Ok(call)
}
fn hex(words: &[f64]) -> Vec<String> {
    words
        .iter()
        .map(|w| format!("{:016x}", w.to_bits()))
        .collect()
}
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let word = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | ((*b as u32) << (16 - i * 8)));
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                TABLE[((word >> (18 - i * 6)) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}
fn run() -> Result<(), String> {
    let check = std::env::args().any(|arg| arg == "--check");
    let mut checked = 0usize;
    let mut words_checked = 0usize;
    let mut strings = Strings::new();
    let mut sequence = 0usize;
    for line in std::io::stdin().lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let v = json::parse(&line).map_err(|e| e.to_string())?;
        let kind = text(&v, "kind")?;
        if kind == "strings" {
            strings = Strings::new();
            for v in entries(&v, "strings")? {
                let units = v
                    .as_array()
                    .ok_or("UTF-16 units")?
                    .iter()
                    .map(|v| v.as_f64().map(|n| n as u16).ok_or("UTF-16 unit"))
                    .collect::<Result<Vec<_>, _>>()?;
                strings.intern(&units);
            }
            continue;
        }
        let before = strings.list.len();
        let (export, call) = match kind {
            "content" => ("ai_world_content", content(&v, &mut strings)?),
            "setup" => ("ai_world_begin", setup(&v, &mut strings)?),
            "roster" => ("ai_world_roster", roster(&v, &mut strings)?),
            "boundary" => ("ai_world_boundary", Call::default()),
            "weaponPolicies" => ("ai_world_weapon_policies", Call::default()),
            "activate" => ("ai_world_activate", Call::default()),
            "propOwner" | "character" => {
                let export = if kind == "propOwner" {
                    "ai_world_prop_owner"
                } else {
                    "ai_world_roster_character"
                };
                (
                    export,
                    Call {
                        args: vec![uint(&v, "index")? as f64],
                        ..Default::default()
                    },
                )
            }
            _ => return Err(format!("unknown kind {kind}")),
        };
        if check {
            let expected = required(&v, "expected")?;
            if text(expected, "export")? != export {
                return Err(format!("expected a different export for {export}"));
            }
            let check_words = |key: &str, actual: &[f64], values: &[Value]| -> Result<(), String> {
                let actual = hex(actual);
                if actual.len() != values.len() {
                    return Err(format!(
                        "{export} {key}: {} words, expected {}",
                        actual.len(),
                        values.len()
                    ));
                }
                for (index, (got, want)) in actual.iter().zip(values).enumerate() {
                    if want.as_str() != Some(got) {
                        return Err(format!(
                            "{export} {key}[{index}]: {got}, expected {}",
                            want.as_str().unwrap_or("non-hex")
                        ));
                    }
                }
                Ok(())
            };
            check_words("arguments", &call.args, entries(expected, "arguments")?)?;
            let args = required(expected, "args")?;
            check_words("row", &call.row, entries(args, "words")?)?;
            check_words("heap", &call.heap, entries(args, "heap")?)?;
            if text(expected, "scratch")? != base64(&call.scratch) {
                return Err(format!("{export}: scratch bytes differ"));
            }
            let interns = entries(expected, "interns")?;
            if interns.len() != strings.list.len() - before {
                return Err(format!(
                    "{export}: intern count {} differs from {}",
                    strings.list.len() - before,
                    interns.len()
                ));
            }
            for (index, (units, expected)) in strings.list[before..].iter().zip(interns).enumerate()
            {
                if uint(expected, "id")? as usize != before + index
                    || text(expected, "text")?
                        .encode_utf16()
                        .ne(units.iter().copied())
                {
                    return Err(format!("{export}: intern {index} differs"));
                }
            }
            words_checked += call.args.len() + call.row.len() + call.heap.len();
            checked += 1;
            continue;
        }
        for (i, units) in strings.list[before..].iter().enumerate() {
            let scratch = units
                .iter()
                .flat_map(|u| u.to_le_bytes())
                .collect::<Vec<_>>();
            println!(
                "{}",
                json!({"type":"call","sequence":sequence,"battle":0,"phase":"admission","parent":null,
                "export":"sim_str_intern","arguments":hex(&[units.len() as f64]),"args":{"words":[],"heap":[]},"scratch":base64(&scratch),
                "intern":{"id":before+i,"text":String::from_utf16_lossy(units)},"returned":{"scalar":format!("{:016x}",((before+i) as f64).to_bits()),"block":null}})
            );
            sequence += 1;
        }
        println!(
            "{}",
            json!({"type":"call","sequence":sequence,"battle":0,"phase":"admission","parent":null,"export":export,"arguments":hex(&call.args),
            "args":{"words":hex(&call.row),"heap":hex(&call.heap)},"scratch":"","intern":null,"returned":null})
        );
        sequence += 1;
    }
    if check {
        println!("{}", json!({"checked":checked,"words":words_checked}));
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
