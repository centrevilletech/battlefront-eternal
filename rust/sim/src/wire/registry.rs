//! Stable content IDs and input profile normalization. Content object identity
//! stays with the host; this boundary owns names, validation and UTF-16 order.
use crate::{
    abi::sim,
    gen::{WrInputs, WrName, WrResult, WrTables},
    store::{StrId, Strings},
};
type Result<T> = std::result::Result<T, &'static str>;
fn name(value: &WrName, strings: &Strings) -> Result<StrId> {
    let id = value.value.ok_or("invalid entity registry name")?;
    let units = strings.units(id);
    if units.is_empty() || units.len() > 128 {
        return Err("invalid entity registry name");
    }
    Ok(id)
}
fn ordered(names: Vec<WrName>, strings: &Strings) -> Result<Vec<WrName>> {
    let mut ids = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for item in names {
        let id = name(&item, strings)?;
        if seen.insert(id) {
            ids.push(id);
        }
    }
    ids.sort_by(|a, b| strings.units(*a).cmp(strings.units(*b)));
    if ids.len() > 65535 {
        return Err("entity registry capacity exceeded");
    }
    Ok(ids
        .into_iter()
        .map(|value| WrName { value: Some(value) })
        .collect())
}
pub fn inputs(mut a: WrInputs, strings: &Strings) -> Result<WrResult> {
    if !a.epoch_valid
        || !a.epoch.is_finite()
        || a.epoch.fract() != 0.
        || !(1. ..=4294967295.).contains(&a.epoch)
    {
        return Err("invalid content epoch");
    }
    if !a.codes_valid || a.codes.len() > 128 {
        return Err("invalid input-code registry");
    }
    let mut seen = std::collections::HashSet::new();
    if a.codes
        .iter()
        .filter_map(|c| c.value)
        .any(|id| !seen.insert(id))
    {
        return Err("invalid input-code registry");
    }
    for item in &a.codes {
        name(item, strings)?;
    }
    if !a.profiles_valid || a.profiles.is_empty() || a.profiles.len() > 256 {
        return Err("invalid binding-profile registry");
    }
    a.profiles.sort_by(|a, b| {
        (a.id - b.id)
            .partial_cmp(&0.)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for profile in &mut a.profiles {
        if !profile.id_valid
            || !profile.id.is_finite()
            || profile.id.fract() != 0.
            || !(0. ..=65535.).contains(&profile.id)
        {
            return Err("invalid entity registry id");
        }
        if !profile.valid || profile.bindings.len() > 32 {
            return Err("invalid binding profile");
        }
        profile
            .bindings
            .sort_by(|a, b| strings.units(a.action).cmp(strings.units(b.action)));
        for binding in &profile.bindings {
            name(
                &WrName {
                    value: Some(binding.action),
                },
                strings,
            )?;
            if !binding.valid
                || binding.codes.len() > 8
                || binding.codes.iter().any(|code| !a.codes.contains(code))
            {
                return Err("binding code missing from registry");
            }
        }
    }
    Ok(WrResult {
        codes: a.codes,
        profiles: a.profiles,
        ..WrResult::default()
    })
}
pub fn tables(a: WrTables, strings: &Strings) -> Result<WrResult> {
    let classes = ordered(a.classes, strings)?;
    let weapons = ordered(a.weapons, strings)?;
    let posts = ordered(a.posts, strings)?;
    Ok(WrResult {
        classes,
        weapons,
        posts,
        ..WrResult::default()
    })
}
#[no_mangle]
pub extern "C" fn wire_registry_words(op: u32) -> u32 {
    match op {
        0 => WrInputs::WORDS as u32,
        1 => WrTables::WORDS as u32,
        _ => 0,
    }
}
#[no_mangle]
pub extern "C" fn wire_registry_run(op: u32) -> *const f64 {
    let s = sim();
    let result = {
        let (row, heap) = s.calls.args();
        match op {
            0 => inputs(WrInputs::take(row, heap), &s.strings),
            1 => tables(WrTables::take(row, heap), &s.strings),
            _ => Err("invalid registry operation"),
        }
    };
    let out = match result {
        Ok(value) => value,
        Err(error) => WrResult {
            error: Some(s.strings.intern_str(error)),
            ..WrResult::default()
        },
    };
    s.calls
        .result(WrResult::WORDS, |row, heap| out.put(row, heap));
    s.calls.block.ptr()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_use_utf16_order_and_keep_first_deduplication() {
        let mut strings = Strings::default();
        let names = [
            vec![0xe000],
            vec![0xd800, 0xdc00],
            vec![0xd800],
            vec![98],
            vec![65],
            vec![98],
        ];
        let names = names
            .into_iter()
            .map(|s| WrName {
                value: Some(strings.intern(&s)),
            })
            .collect();
        let out = ordered(names, &strings).unwrap();
        assert_eq!(
            out.iter()
                .map(|v| strings.units(v.value.unwrap()))
                .collect::<Vec<_>>(),
            vec![&[65][..], &[98], &[0xd800], &[0xd800, 0xdc00], &[0xe000]]
        );
        assert_eq!(
            ordered(vec![WrName { value: None }], &strings),
            Err("invalid entity registry name")
        );
    }
}
