//! The cold AI weapon source (ai-data.js and weapon-policy-content.js).
//!
//! The simulation already parses authored policy and computes per-level
//! defaults. Admission transports original bytes and canonical weapon ids;
//! serializing parsed JSON here would lose the original byte stream and its
//! authored numeric forms. The AI world consumes the same public source.
use super::Call;
use crate::gen::{AiWeaponData, AiWeaponPolicySource};
use crate::store::DefId;
use crate::world::admit::{AdmitError, Stage};
use bfe_content::battle::Battle;
use bfe_content::files::Files;

/// bindWeaponPolicyContent's `data.filter(Boolean).map(json => ({json}))`.
/// An empty but successfully fetched Uint8Array is truthy in JavaScript and
/// therefore remains in the list; only a failed fetch is omitted.
pub fn source_with_data(weapons: &[DefId], data: impl IntoIterator<Item = Option<Vec<u8>>>) -> AiWeaponPolicySource {
    AiWeaponPolicySource {
        weapons: weapons.to_vec(),
        data: data.into_iter().flatten().map(|json| AiWeaponData { json }).collect(),
    }
}

/// loadAIWeaponData follows Object.keys(sides), never sorted faction names.
/// Read original bytes through Files, including invalid JSON: the native
/// parser decides validity, and a failed fetch is omitted by filter(Boolean).
pub fn source(battle: &Battle, files: &dyn Files, weapons: &[DefId]) -> AiWeaponPolicySource {
    source_with_data(weapons, battle.sides.iter().map(|side| {
        files.read(&format!("sides/{}/ai.json", side.name)).map(|bytes| bytes.to_vec())
    }))
}

/// putAiWeaponPolicySource, using the generated layout for both nested byte
/// arrays and definition ids. Heap offsets are relative to the heap start.
pub fn call(source: &AiWeaponPolicySource) -> Call {
    let mut row = vec![0.0; AiWeaponPolicySource::WORDS];
    let mut heap = Vec::new();
    source.put(&mut row, &mut heap);
    Call { row, heap, ..Call::default() }
}



/// The ids are the weapon owner's previously assigned entries, in order.
/// Original AI bytes are shared with ai_world_content through source().
pub fn build(cx:&mut super::Cx)->Result<super::Calls,AdmitError> {
    // bindWeaponPolicyContent sends any classes that another generated row
    // reserved by handle but physical construction has not sent yet.
    let (_,mut calls)=super::combat::with_bindings(cx,Stage::Policies,|bindings,strings| {
        let definitions=bindings.combat.weapons.values.clone();
        for definition in &definitions {bindings.combat.weapon(definition,strings)?;}
        Ok(((),std::mem::take(&mut bindings.combat.calls)))
    })?;
    let weapons=weapon_ids(cx)?;
    let input=source(cx.battle,cx.files,&weapons);
    calls.push((Stage::Policies,"ai_weapon_policies",call(&input)));
    Ok(calls)
}
pub fn weapon_ids(cx:&super::Cx)->Result<Vec<DefId>,AdmitError> {
    cx.ids.defs.iter().filter(|(key,_)|key.starts_with("weapon:")).map(|(_,id)| {
        if id.is_finite()&&*id>=0.0&&*id<=u32::MAX as f64&&id.fract()==0.0 {Ok(DefId(*id as u32))}
        else {Err(AdmitError::Content {stage:Stage::Policies,message:"invalid earlier weapon id".into()})}
    }).collect()
}
