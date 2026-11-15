//! Authored admission for Atmosphere, Snow, Weather, SkyFlyers and placed FX.
//! Browser resources report availability once. Rust selects the admitted rows,
//! resolves their limits, and writes the exact same native configuration used
//! by the retained phases. No host rebuilds these records field by field.
use crate::layout::{
    atmosphere_config as ac, dust_config as dc, environment_instance as ei, sky_class as cl,
    sky_config as sc,
};
use crate::{AtmospherePhase, EnvironmentPhase, SkyPhase, SnowPhase};
use serde_json::{json, Value};
// A missing authored scalar had the same zero default when the host wrote
// its Float64 configuration. Keep defaults at admission, outside frame work.
fn number(v: &Value, key: &str) -> f64 {
    v[key].as_f64().unwrap_or(0.)
}
// Missing optional arrays admit no records. This also handles maps prepared
// without enhanced sky or dust data, which still use their classic content.
fn array(v: &Value) -> impl Iterator<Item = &Value> {
    v.as_array().into_iter().flatten()
}
// Configuration rows are fixed layouts. Each caller selects its generated
// field offset; truncation here protects the remaining row from malformed
// optional arrays without allowing content to overwrite a neighbouring class.
fn copy(out: &mut [f64], at: usize, v: &Value) {
    for (i, n) in array(v).enumerate() {
        if at + i >= out.len() {
            break;
        }
        out[at + i] = n.as_f64().unwrap_or(0.);
    }
}

// Atmosphere.constructor previously counted/filter-matched classes in JS and
// flattened each authored box into the native config. Texture availability is
// an external resource fact; authored order and instance offsets are native.
pub fn atmosphere(data: &Value) -> (AtmospherePhase, Value) {
    let recipe = crate::content::recipe(
        crate::layout::environment_recipe::ATMOSPHERE as u32,
        &data["data"],
    );
    // Resource failure is reported as availability, never a replacement class.
    // Filtering here preserves authored class order and its random-stream order.
    let classes = array(&recipe["classes"])
        .filter(|c| {
            data["available"][c["name"].as_str().unwrap_or("")]
                .as_bool()
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    // A missing fog texture removes its batch entirely. Dust then begins at
    // batch zero, just as the original resource shell indexed its uploads.
    let fog = data["fog"].as_bool().unwrap_or(false);
    let count = classes.iter().map(|c| array(&c["instances"]).count()).sum();
    let mut phase = AtmospherePhase::new(classes.len(), count, fog);
    let c = &mut phase.config;
    let source = &data["data"]["fogCloud"];
    if fog {
        c[ac::HAS_FOG] = 1.;
        copy(c, ac::NEAR, &source["range"]);
        c[ac::CELL] = source["cell"].as_f64().filter(|v| *v != 0.).unwrap_or(256.);
        for (key, at) in [
            ("size", ac::SIZE),
            ("density", ac::DENSITY),
            ("rotation", ac::ROTATION),
            ("height", ac::HEIGHT),
        ] {
            c[at] = number(source, key);
        }
        copy(c, ac::VELOCITY, &source["velocity"]);
        c[ac::ALPHA] = number(&recipe, "alpha");
    }
    // Instances are packed class by class. Their stable prefix offsets must
    // agree with Atmosphere::new, including classes with multiple placed boxes.
    c[ac::CLASSES] = classes.len() as f64;
    let mut first = 0;
    let mut batches = vec![];
    if fog {
        batches.push(
            json!({"name":"map-fog-cloud","texture":source["texture"],"tint":recipe["tint"]}),
        );
    }
    // Each dust class keeps its original name-derived seed. Texture identity
    // has no influence on the native seed or particle initialization order.
    for (k, row) in classes.iter().enumerate() {
        let name = row["name"].as_str().unwrap_or("");
        let def = array(&data["data"]["dust"]["classes"])
            .find(|v| v[0].as_str() == Some(name))
            .map(|v| &v[1])
            .unwrap_or(&Value::Null);
        let at = ac::STRIDE + k * dc::STRIDE;
        let c = &mut c[at..at + dc::STRIDE];
        for (key, at) in [
            ("count", dc::COUNT),
            ("minSize", dc::MIN_SIZE),
            ("maxSize", dc::MAX_SIZE),
            ("minLifetime", dc::MIN_LIFETIME),
            ("maxLifetime", dc::MAX_LIFETIME),
            ("alpha", dc::ALPHA),
            ("radius", dc::RADIUS),
            ("minDistance", dc::MIN_DISTANCE),
            ("maxDistance", dc::MAX_DISTANCE),
            ("heightScale", dc::HEIGHT_SCALE),
        ] {
            c[at] = number(def, key);
        }
        for (key, at) in [
            ("minPos", dc::MIN_POS),
            ("maxPos", dc::MAX_POS),
            ("minVel", dc::MIN_VEL),
            ("maxVel", dc::MAX_VEL),
            ("radiusFade", dc::FADE_MIN),
        ] {
            copy(c, at, &def[key]);
        }
        c[dc::SEED] = number(row, "seed");
        c[dc::INSTANCES] = array(&row["instances"]).count() as f64;
        for position in array(&row["instances"]) {
            copy(&mut phase.instances, first * 3, position);
            first += 1;
        }
        batches.push(json!({"name":format!("map-dust-{name}"),"texture":def["texture"],"tint":[1.,1.,1.,1.]}));
    }
    phase.start();
    (phase, json!({"batches":batches}))
}

// Snow.constructor owns no density, repeated-box or cover-size policy. The
// renderer receives the compiled flakes/boxes and their typed draw values.
// The snow recipe rounds combined density before choosing the box budget.
// Starting the phase here ensures allocation and flake initialization happen
// during loading, before a renderer can borrow any of its draw rows.
pub fn snow(data: &Value) -> (SnowPhase, Value) {
    let r = crate::content::recipe(crate::layout::environment_recipe::SNOW as u32, data);
    let mut p = SnowPhase::new();
    copy(&mut p.config, 0, &r["config"]);
    p.start();
    (p, r)
}

// SkyFlyers.constructor resolves mission membership and resource bounds once.
// Missing resource parts exclude a class before phase allocation. Class ids
// refer back to authored definitions, so browser asset caches keep identity.
pub fn sky(data: &Value) -> (SkyPhase, Value) {
    let source = &data["data"];
    let mission = data["mission"].as_str().unwrap_or("");
    // Mission indices refer to authored definitions rather than model names.
    // Two classes may share a model while retaining independent flight state.
    let selected = |enhanced| {
        crate::content::recipe(
            crate::layout::environment_recipe::FLYERS as u32,
            &json!({"data":source,"mission":mission,"enhanced":enhanced}),
        )
    };
    let classic = selected(false);
    let enhanced = selected(true);
    let mut rows = vec![];
    for (index, def) in array(&source["flyers"]).enumerate() {
        let index_value = json!(index);
        let a = array(&classic).any(|i| i == &index_value);
        let b = array(&enhanced).any(|i| i == &index_value);
        // Empty model parts are an actual resource omission. Selection tests
        // both mission lists before dropping such a class, matching the host.
        let parts = &data["models"][def["model"].as_str().unwrap_or("")];
        if (!a && !b) || array(parts).next().is_none() {
            continue;
        }
        let radius =
            crate::content::recipe(crate::layout::environment_recipe::RADIUS as u32, parts);
        rows.push(
            json!({"index":index,"count":def["count"],"radius":radius,"classic":a,"enhanced":b}),
        );
    }
    // One native class is allocated per admitted authored class. Bounds are
    // reduced over model parts once, not recomputed while ships are placed.
    let mut p = SkyPhase::new(rows.len());
    p.config[sc::SEED] = number(data, "seed");
    p.config[sc::CLASSES] = rows.len() as f64;
    p.config[sc::ENHANCED] = f64::from(data["enhanced"].as_bool().unwrap_or(false));
    for (k, row) in rows.iter().enumerate() {
        let def = &source["flyers"][row["index"].as_u64().unwrap_or(0) as usize];
        let c = &mut p.config[sc::STRIDE + k * cl::STRIDE..sc::STRIDE + (k + 1) * cl::STRIDE];
        for (key, at) in [
            ("count", cl::COUNT),
            ("distance", cl::DISTANCE),
            ("inDirection", cl::IN_DIRECTION),
            ("lifeTime", cl::LIFE_TIME),
        ] {
            c[at] = number(def, key);
        }
        copy(c, cl::HEIGHT, &def["height"]);
        copy(c, cl::ACCELERATION, &def["acceleration"]);
        for (k, range) in array(&def["velocity"]).take(3).enumerate() {
            copy(c, cl::VELOCITY + k * 2, range);
        }
        c[cl::RADIUS] = number(row, "radius");
        c[cl::CLASSIC] = f64::from(row["classic"].as_bool().unwrap_or(false));
        c[cl::ENHANCED] = f64::from(row["enhanced"].as_bool().unwrap_or(false));
    }
    // Browser metadata contains only the ids needed to install real assets.
    // Flight parameters stay in the phase and are never mirrored back to JS.
    p.start();
    (p, json!({"classes":rows}))
}

// EnvironmentEffects.constructor publishes spheres/positions directly from
// the authored recipe; no temporary Three sphere or quaternion computes them.
// Native metadata decides which optional enhanced resource groups are needed.
pub fn sources(data: &Value) -> (EnvironmentPhase, Value) {
    let mut r = crate::content::recipe(crate::layout::environment_recipe::SOURCES as u32, data);
    let count = array(&r["instances"]).count();
    let mut p = EnvironmentPhase::new(count);
    // Enhanced groups reserve space for the source volume and its optional
    // fountain jet. The capacity is independent of current camera visibility.
    let mut enhanced = 0;
    let mut steam = false;
    let mut resets = vec![];
    for (k, row) in array(&r["instances"]).enumerate() {
        let c = &mut p.config[k * ei::STRIDE..(k + 1) * ei::STRIDE];
        copy(c, ei::SPHERE, &row["sphere"]);
        copy(c, ei::POSITION, &row["position"]);
        let classic = row["classic"].as_bool().unwrap_or(false);
        let fountain = row["fountain"].as_bool().unwrap_or(false);
        c[ei::CLASSIC] = f64::from(classic);
        c[ei::JET] = f64::from(fountain);
        if !classic {
            enhanced += 1;
            steam |= !fountain;
            resets.push(k);
        }
    }
    // Resolve effect-name lookup at admission. Particle play commands carry
    // their already compiled warm-up duration rather than querying a JS Map.
    let warmup = r["warmup"].clone();
    if let Some(rows) = r["instances"].as_array_mut() {
        for (k, row) in rows.iter_mut().enumerate() {
            let name = data["instances"][k]["effect"].as_str().unwrap_or("");
            row["warmup"] = json!(warmup[name].as_f64().unwrap_or(0.));
            row["jet"] = if row["fountain"].as_bool().unwrap_or(false) {
                json!({"direction":[0.,1.,0.],"extent":[5.,5.3,5.],"kind":3})
            } else {
                Value::Null
            };
        }
    }
    // Fountains use basin water and spray; dry steam maps use steam wisps.
    // These resource requirements are authored policy, not browser choices.
    let basins = array(&data["basins"]).next().is_some();
    r["volumeCapacity"] = json!(enhanced * 2);
    r["water"] = json!(basins);
    r["steam"] = json!(!basins && steam);
    r["resets"] = json!(resets);
    p.start();
    (p, r)
}
