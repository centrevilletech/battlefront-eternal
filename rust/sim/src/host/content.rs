//! What the host's records name from the battle's content that the
//! recorded admission cannot give it: the ordnance definitions' classes (a
//! grenade record's `def`) and the combat registry ids of the explosions.
//! The page's capture named an explosion by
//! `registry.combat.definitionId(def)`, the id of the definition object
//! (src/runtime/network-combat-registry.js), and the store source's tables
//! carry the ordnance and item definitions' ids but no explosion's. An
//! explosion the page registered while it wrote an ordnance or item
//! definition's fields (`writeFields`, src/sim/wasm/ordnance-sim.js) is the
//! object that definition holds under the field naming it, so its id is
//! that object's in the registry the content crate builds from the same
//! content (`createCombatRegistry(registryContent(data))`).
use std::collections::HashMap;
use std::path::Path;

use super::Content as HostContent;
use crate::abi::context::Context;
use crate::abi::context_step::ExplosionOwner;
use crate::world::admit::Options;
use bfe_content::battle::{Battle, Loader};
use bfe_content::content::Content;
use bfe_content::defs::identity;
use bfe_content::files::{DirFiles, Files};
use bfe_content::registry::{combat_registry, registry_content};
use bfe_content::value::Value;

/// world-hash.mjs's page query for the row's options.
fn query(options: &Options) -> String {
    let era = if options.side == "rep" || options.side == "cis" {
        "cw"
    } else {
        "gcw"
    };
    let ai = if options.enhanced {
        "enhanced"
    } else {
        "classic"
    };
    let mut query = format!(
        "?map={}&side={}&era={era}&ai={ai}&seed={}",
        options.map, options.side, options.seed
    );
    if options.heroes {
        query.push_str("&heroes=1");
    }
    if let Some(unit) = &options.unit {
        query.push_str("&unit=");
        query.push_str(unit);
    }
    query
}

/// The content the host needs: each explosion's registry id by explosion
/// index, resolved through whatever named it (an ordnance or item field, a
/// unit class, a building, a recon droid class, a hull; 0 where nothing
/// did), and each ordnance definition's class by definition index, read
/// from the registry object its id names.
pub fn content_tables(
    context: &Context,
    content_dir: &Path,
    options: &Options,
    store_source: u32,
) -> Result<(Vec<u32>, HostContent), String> {
    let files = DirFiles::new(content_dir);
    content_tables_files(context, &files, options, store_source)
}

/// Resolve the same tables through the native client's selected content provider.
pub fn content_tables_files(
    context: &Context,
    files: &dyn Files,
    options: &Options,
    store_source: u32,
) -> Result<(Vec<u32>, HostContent), String> {
    let loader = Loader::new(files);
    let battle = Battle::load(&loader, &options.map, &query(options))?;
    let content = Content::build(battle)?;
    // One tree for the walk and the lookups: the registry names objects.
    let tree = registry_content(&content);
    let registry = combat_registry(&tree)?;
    content_tables_from_registry(context, store_source, &tree, &registry)
}

/// The same cold resolution over retained registry objects. A dedicated host
/// can refresh definitions after joins without reloading map geometry.
pub fn content_tables_from_registry(
    context: &Context,
    store_source: u32,
    tree: &Value,
    registry: &[(String, Value)],
) -> Result<(Vec<u32>, HostContent), String> {
    // `ids`: the registry's WeakMap from a definition object to its id.
    let ids: HashMap<usize, u32> = registry
        .iter()
        .enumerate()
        .filter_map(|(i, (_, value))| identity(value).map(|object| (object, i as u32 + 1)))
        .collect();
    let mut explosions = Vec::new();
    for field in context
        .explosion_fields(store_source)
        .map_err(|e| e.message().to_string())?
    {
        let index = field.explosion as usize;
        if explosions.len() <= index {
            explosions.resize(index + 1, 0);
        }
        if explosions[index] != 0 || field.definition < 1.0 {
            continue;
        }
        let Some((_, definition)) = registry.get(field.definition as usize - 1) else {
            continue;
        };
        if let Some(&id) = definition
            .get(field.field)
            .and_then(identity)
            .and_then(|object| ids.get(&object))
        {
            explosions[index] = id;
        }
    }
    // A class's or a hull's explosion: the object its content definition
    // holds (`unit.classDef.explosion` in src/sim/wasm/f3-items.js,
    // `vehicle.def.explosion` and `finalExplosion || explosion` in
    // src/sim/wasm/f3-hulls.js).
    // LiveObjects.buildings (src/props.js): the props instances whose model
    // the props name, in order, that are traps, power-up stations, armed
    // buildings or command posts; a building's explosion is its instance's.
    let props = tree.get("props");
    let models = props.and_then(|p| p.get("models"));
    let buildings: Vec<&Value> = props
        .and_then(|p| p.get("instances"))
        .and_then(Value::as_array)
        .map(|instances| {
            instances
                .iter()
                .filter(|desc| {
                    let model = desc.get("model").and_then(Value::as_str);
                    let base = desc.get("base").and_then(Value::as_str).unwrap_or("");
                    model.is_some_and(|m| {
                        models.and_then(|ms| ms.get(m)).is_some_and(|v| v.truthy())
                    }) && matches!(
                        base,
                        "trap" | "powerupstation" | "armedbuilding" | "commandpost"
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    for owned in context
        .explosion_owners()
        .map_err(|e| e.message().to_string())?
    {
        let index = owned.explosion as usize;
        if explosions.len() <= index {
            explosions.resize(index + 1, 0);
        }
        if explosions[index] != 0 {
            continue;
        }
        let name = |id| {
            context
                .string(id)
                .map(|units| String::from_utf16_lossy(&units))
                .map_err(|e| e.message().to_string())
        };
        let object = match owned.owner {
            ExplosionOwner::Class(class) => {
                let class = name(class)?;
                let sides = tree.get("sides").and_then(Value::as_object);
                sides
                    .and_then(|sides| {
                        sides
                            .values()
                            .find_map(|side| side.get("units").and_then(|units| units.get(&class)))
                    })
                    .and_then(|def| def.get("explosion"))
            }
            ExplosionOwner::Building(index) => buildings
                .get(index as usize)
                .and_then(|desc| desc.get("explosion")),
            // A droid class is the registry definition the store source's
            // remote table names (`registry.combat.definitionId(def)`).
            ExplosionOwner::Recon(class) => {
                let id = context
                    .wire_remote_id(store_source, class)
                    .map_err(|e| e.message().to_string())?;
                registry
                    .get((id as usize).wrapping_sub(1))
                    .and_then(|(_, def)| def.get("explosion"))
            }
            ExplosionOwner::Hull { class, last } => {
                let class = name(class)?;
                let def = tree
                    .get("vehicles")
                    .and_then(|v| v.get("classes"))
                    .and_then(|c| c.get(&class));
                def.and_then(|def| {
                    if last {
                        def.get("finalExplosion")
                            .filter(|v| v.truthy())
                            .or_else(|| def.get("explosion"))
                    } else {
                        def.get("explosion")
                    }
                })
            }
        };
        if let Some(&id) = object
            .and_then(identity)
            .and_then(|object| ids.get(&object))
        {
            explosions[index] = id;
        }
    }
    let count = context
        .ordnance_definitions()
        .map_err(|e| e.message().to_string())?;
    let mut ordnance_classes = Vec::with_capacity(count);
    for def in 0..count as u32 {
        let id = context
            .wire_ordnance_id(store_source, def)
            .map_err(|e| e.message().to_string())?;
        let class = match registry
            .get((id as usize).wrapping_sub(1))
            .and_then(|(_, definition)| definition.get("class"))
        {
            Some(Value::String(class)) => {
                let units: Vec<u16> = class.encode_utf16().collect();
                Some(
                    context
                        .intern(&units)
                        .map_err(|e| e.message().to_string())?,
                )
            }
            _ => None,
        };
        ordnance_classes.push(class);
    }
    Ok((explosions, HostContent { ordnance_classes }))
}
