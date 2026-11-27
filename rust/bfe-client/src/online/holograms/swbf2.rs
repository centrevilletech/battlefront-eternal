//! The command posts' holograms in a 2005 battle, described by the 2005
//! game's own data and drawn by the same present as the 2004 holograms.
//!
//! A post whose class names a hologram class (HoloOdf) carries a hologram at
//! its model's hp_hologram mount. The hologram class gives the look (height,
//! size, reach, intensities, rotation, flicker, fade, light radius) and the
//! colours the viewer's team sees an owner in. The post class pairs side
//! names with the model an owner of that side shows (HoloImageGeometry
//! "<model> <side>") and gives the turn-on, neutralize and capture times.
//!
//! The game files every side name into one of eight slots, and a team shows
//! the model of the slot its mission name (SetTeamName) falls in; a slot the
//! class names no model for shows the hologram without a symbol. Posts of
//! one mission share one look and one set of symbols, those of the earliest listed post
//! that has a hologram, as the 2004 holograms do. The 2005 data has no flare
//! texture, so a 2005 hologram draws its beam, halo, symbol and light only.
use super::{Fields, Holograms, Model, Post, HOLOGRAMS, LOGO_TEXTURE, PARTS};
use crate::render_bridge::Bridge;
use bfe_content::Value;
use bfe_content_native::BattleContent;
use bfe_presentation::{
    post_effects::{Descriptor, Look, Posts},
    visual_effects_layout::visual_limits as limits,
};
use bfe_render::effect_layout as layout;
use std::collections::BTreeMap;
use three::{Matrix4, Vector3};

/// The side slots a post class fills with symbols.
const SLOTS: usize = 8;

/// The slot the game files a side name in (letter case aside): a name it
/// does not know takes the slot of every other name.
fn slot(side: &str) -> usize {
    match side.to_ascii_lowercase().as_str() {
        "neutral" => 0,
        "alliance" | "all" => 1,
        "empire" | "imp" => 2,
        "republic" | "rep" => 3,
        "cis" => 4,
        "hero" | "heroes" => 6,
        "villain" | "villains" => 7,
        _ => 5,
    }
}

/// The symbols of a 2005 battle by slot, waiting for the mission to name
/// its teams.
pub(super) struct Teams {
    slots: [Option<(Vec<Model>, bool)>; SLOTS],
    named: bool,
}

/// What the data says about a mission's holograms.
#[derive(Debug, PartialEq)]
struct Description {
    look: serde_json::Value,
    size: f64,
    light_radius: f64,
    // neutral, friendly, enemy, locals
    colors: [[f64; 3]; 4],
    turn_on: f64,
    neutralize_time: f64,
    capture_time: f64,
    // Each hologram's post: its place in the posts' list, name and anchor.
    posts: Vec<(usize, String, [f64; 3])>,
    // Each symbol's slot and model name.
    symbols: Vec<(usize, String)>,
}

// An entity class by name, in any letter case.
fn class<'a>(classes: &'a Value, name: &str) -> Option<&'a Value> {
    let table = classes.get("entc")?.as_object()?;
    table.get(name).or_else(|| table.entries().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, class)| class))
}
// Every value a class gives a property, in its order.
fn values<'a>(class: &'a Value, key: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    class.get("properties").and_then(Value::as_array).unwrap_or(&[]).iter().filter_map(move |pair| {
        pair.at(0)?.as_str()?.eq_ignore_ascii_case(key).then_some(())?;
        pair.at(1)?.as_str()
    })
}
// A property's value: the last one the class gives.
fn value<'a>(class: &'a Value, key: &'a str) -> Option<&'a str> {
    values(class, key).last()
}
fn number(class: &Value, key: &str, default: f64) -> f64 {
    value(class, key).and_then(|v| v.trim().parse().ok()).filter(|v: &f64| v.is_finite()).unwrap_or(default)
}
fn color(class: &Value, key: &str, default: [f64; 3]) -> [f64; 3] {
    let mut out = default;
    for (slot, word) in out.iter_mut().zip(value(class, key).unwrap_or("").split_whitespace()) {
        if let Ok(v) = word.parse::<f64>() {
            *slot = v;
        }
    }
    out
}

// Where a model's hp_hologram mount sits in the model: the bone's own
// transforms up to the root (the origin when the model has none).
fn mount(skeletons: Option<&Value>, model: &str) -> [f64; 3] {
    let bones = skeletons
        .and_then(Value::as_object)
        .and_then(|table| table.get(model).or_else(|| table.entries().find(|(k, _)| k.eq_ignore_ascii_case(model)).map(|(_, v)| v)))
        .and_then(Value::as_array)
        .unwrap_or(&[]);
    let named = |bone: &Value| bone.get("name").and_then(Value::as_str).is_some_and(|n| n.eq_ignore_ascii_case("hp_hologram"));
    let Some(mut at) = bones.iter().position(named) else { return [0.; 3] };
    let mut point = Vector3::new(0., 0., 0.);
    for _ in 0..bones.len() {
        let mut local = Matrix4::default();
        for (k, slot) in local.elements.iter_mut().enumerate() {
            if let Some(v) = bones[at].get("local").and_then(|m| m.at(k)).and_then(Value::as_f64) {
                *slot = v;
            }
        }
        point.apply_matrix4(&local);
        match bones[at].get("parent").and_then(Value::as_f64) {
            Some(parent) if parent >= 0. && (parent as usize) < bones.len() => at = parent as usize,
            _ => break,
        }
    }
    [point.x, point.y, point.z]
}

// The mission's posts (the composed map's list) read against the map's
// entity classes and the props' skeletons.
fn describe(posts: &[serde_json::Value], classes: &Value, skeletons: Option<&Value>) -> Option<Description> {
    let mut source = None;
    let mut out = Vec::new();
    for (index, post) in posts.iter().enumerate().take(limits::POSTS) {
        let Some(post_class) = post["class"].as_str().and_then(|name| class(classes, name)) else { continue };
        // A post whose class has no HoloOdf has no hologram.
        let Some(holo) = value(post_class, "HoloOdf").filter(|n| !n.is_empty()).and_then(|name| class(classes, name)) else {
            continue;
        };
        if out.len() == HOLOGRAMS {
            break;
        }
        source.get_or_insert((post_class, holo));
        let mut matrix = Matrix4::default();
        for (k, slot) in matrix.elements.iter_mut().enumerate() {
            if let Some(v) = post["matrix"][k].as_f64() {
                *slot = v;
            }
        }
        let model = value(post_class, "GeometryName").unwrap_or("");
        let [x, y, z] = mount(skeletons, model);
        let mut anchor = Vector3::new(x, y, z);
        anchor.apply_matrix4(&matrix);
        let name = post["name"].as_str().filter(|n| !n.is_empty()).map_or_else(|| format!("#{index}"), str::to_string);
        out.push((index, name, [anchor.x, anchor.y, anchor.z]));
    }
    let (post_class, holo) = source?;
    // Absent values take those the released preparation gives a 2004 hologram.
    let look = serde_json::json!({
        "visibleDistance": number(holo, "HoloVisibleDistance", 200.),
        "height": number(holo, "HoloHeight", 3.5),
        "beamIntensity": number(holo, "HoloBeamIntensity", 1.),
        "lightIntensity": number(holo, "HoloLightIntensity", 1.),
        "imageIntensity": number(holo, "HoloImageIntensity", 1.),
        "flareIntensity": number(holo, "HoloFlareIntensity", 1.),
        "rotateRate": number(holo, "HoloRotateRate", 0.),
        "flickerRate": number(holo, "HoloFlickerRate", 0.),
        "flickerAlpha": [number(holo, "HoloFlickerAlphaMin", 1.), number(holo, "HoloFlickerAlphaMax", 1.)],
        "fadeInTime": number(holo, "HoloFadeInTime", 2.),
    });
    let mut symbols = Vec::new();
    for entry in values(post_class, "HoloImageGeometry") {
        let mut words = entry.split_whitespace();
        if let (Some(model), Some(side), None) = (words.next(), words.next(), words.next()) {
            symbols.push((slot(side), model.to_ascii_lowercase()));
        }
    }
    Some(Description {
        look,
        size: number(holo, "HoloSize", 1.),
        light_radius: number(holo, "HoloLightRadius", 8.),
        colors: [
            color(holo, "NeutralColor", [255., 255., 255.]),
            color(holo, "FriendlyColor", [30., 220., 30.]),
            color(holo, "EnemyColor", [240., 30., 30.]),
            color(holo, "LocalsColor", [240., 220., 30.]),
        ],
        turn_on: number(post_class, "HoloTurnOnTime", 2.2),
        neutralize_time: number(post_class, "NeutralizeTime", 12.),
        capture_time: number(post_class, "CaptureTime", 10.),
        posts: out,
        symbols,
    })
}

fn description(content: &BattleContent) -> Option<Description> {
    let admission = &content.content.battle.swbf2.as_ref()?.map_admission;
    let posts = content.map["commandPosts"].as_array()?;
    let skeletons = admission.props.as_ref().and_then(|p| p.get("skeletons"));
    describe(posts, admission.map_classes.as_ref()?, skeletons)
}

/// Every texture a 2005 battle's holograms draw with: their symbols'.
pub(super) fn textures(content: &BattleContent) -> Vec<String> {
    let mut names = vec![LOGO_TEXTURE.to_string()];
    let models = content.content.props_models.as_ref();
    for (_, model) in description(content).map(|d| d.symbols).unwrap_or_default() {
        for part in models.and_then(|m| m.get(&model)).into_iter().flat_map(|m| m.iter()) {
            if let Some(texture) = part.field("texture").and_then(Value::as_str) {
                names.push(texture.to_string());
            }
        }
    }
    names
}

#[allow(clippy::too_many_arguments)]
pub(super) fn new(
    content: &BattleContent,
    bridge: &mut Bridge,
    resources: &mut crate::resources::Resources,
    textures: &[u32],
    images: &BTreeMap<String, usize>,
    data: usize,
    matrices: usize,
) -> Result<Holograms, String> {
    let described = description(content);
    let listed = content.map["commandPosts"].as_array().cloned().unwrap_or_default();
    let (look, size, light_radius, colors, times) = match &described {
        Some(d) => (
            serde_json::from_value::<Look>(d.look.clone()).unwrap_or_default(),
            d.size,
            d.light_radius,
            d.colors,
            [d.turn_on, d.neutralize_time, d.capture_time],
        ),
        None => (Look::default(), 1., 0., [[255.; 3]; 4], [2.2, 12., 10.]),
    };
    let posts = described.as_ref().map_or_else(Vec::new, |d| {
        d.posts.iter().map(|(index, name, anchor)| Post { index: *index, name: name.clone(), anchor: *anchor, landing: false }).collect()
    });
    let mut holograms = Holograms {
        colors,
        height: look.height,
        size,
        turn_on: times[0],
        neutralize_time: times[1],
        trace: crate::app::bench::fx().is_some(),
        capture_time: times[2],
        light_radius,
        owner: Posts::new(Descriptor { look, posts: listed }),
        posts,
        pads: Vec::new(),
        beam: None,
        flare: None,
        halo: None,
        logos: Vec::new(),
        fields: Fields::default(),
        time: 0.,
        seed: 0x9E37_79B9_7F4A_7C15,
        data,
        matrices,
        drawn: 0,
        draws: 0,
        swbf2: None,
    };
    for &(name, size) in layout::PRODUCERS[layout::ACTOR_BASIC].1 {
        let at = holograms.fields.length;
        holograms.fields.length += size as usize;
        match name {
            "projectionMatrix" => holograms.fields.projection = at,
            "mapTransform" => holograms.fields.map = at,
            "diffuse" => holograms.fields.diffuse = at,
            "opacity" => holograms.fields.opacity = at,
            _ => {}
        }
    }
    bridge.fx_reserve((data + HOLOGRAMS * PARTS * holograms.fields.length) as u32);
    let Some(described) = described.filter(|d| !d.posts.is_empty()) else { return Ok(holograms) };
    let length = holograms.fields.length;
    let model = |bridge: &mut Bridge, source: u32, texture: i32| -> Result<Model, String> {
        let row = bridge.actor_effect(source, layout::ACTOR_BASIC as u32, false, false, texture, "{}")?;
        if row.len() != 4 + length {
            return Err("hologram model values differ from the catalog".into());
        }
        Ok(Model { id: row[0] as u32, values: row[4..].iter().map(|v| *v as f32).collect() })
    };
    // The procedural art, as the 2004 holograms upload it: a 128
    // square halo, then the beam's 4 by 64 gradient, flipped.
    let art = |resources: &mut crate::resources::Resources, bridge: &mut Bridge, pixels: &[u8], width: usize, height: usize, flip: bool| -> Result<i32, String> {
        let source = resources.source(&serde_json::json!({"width":width,"height":height,"depth":1,"target":glow::TEXTURE_2D,"format":1023,"dataType":1009,"srgb":false}).to_string())?;
        resources.source_pixels(source, pixels)?;
        let sample = resources.sample(source, &serde_json::json!({"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1006,"mag":1006,"anisotropy":1,"flip":flip,"premultiply":false,"alignment":4,"mipmaps":false,"compare":null}).to_string())?;
        resources.upload(sample)?;
        Ok(bridge.r.core.add_texture(resources.texture(sample)?) as i32)
    };
    let split = 128 * 128 * 4;
    let pixels = holograms.owner.art.clone();
    let halo = art(resources, bridge, &pixels[..split], 128, 128, false)?;
    let gradient = art(resources, bridge, &pixels[split..], 4, 64, true)?;
    let image = |name: &str| images.get(name).map_or(-1, |image| textures[*image] as i32);
    let cone = bridge.actor_source(&serde_json::json!({"kind":"cylinder","height":holograms.height}).to_string(), &[], &[])?;
    let square = bridge.actor_source(r#"{"kind":"plane","width":1,"height":1}"#, &[], &[])?;
    if let Some(cone) = cone {
        holograms.beam = Some(model(bridge, cone, gradient)?);
    }
    if let Some(square) = square {
        holograms.halo = Some(model(bridge, square, halo)?);
    }
    // Each slot's symbol from the map's prop models, each part with its own texture.
    let mut teams = Teams { slots: Default::default(), named: false };
    let models = content.content.props_models.as_ref();
    for (at, name) in &described.symbols {
        let mut parts = Vec::new();
        let mut scrolls = false;
        for (k, part) in models.and_then(|m| m.get(name)).into_iter().flat_map(|m| m.iter()).enumerate() {
            let texture = part.field("texture").and_then(Value::as_str).unwrap_or("");
            scrolls |= k == 0 && texture.eq_ignore_ascii_case(LOGO_TEXTURE);
            let record = serde_json::json!({"kind":"model","parts":[{"vertexOffset":0,"vertexCount":part.count(),
                "indexOffset":0,"indexCount":part.index.len()}]});
            if let Some(source) = bridge.actor_source(&record.to_string(), &part.data, &part.index)? {
                parts.push(model(bridge, source, image(texture))?);
            }
        }
        if !parts.is_empty() {
            teams.slots[*at] = Some((parts, scrolls));
        }
    }
    holograms.swbf2 = Some(Box::new(teams));
    Ok(holograms)
}

impl Holograms {
    /// A 2005 battle's teams as its mission named them: each team shows the
    /// symbol of its name's slot. Done once, when a name is first known; a
    /// 2004 battle has nothing to name.
    pub fn name_teams(&mut self, names: &[&str]) {
        let Some(teams) = self.swbf2.as_mut().filter(|t| !t.named) else { return };
        if names.iter().all(|name| name.is_empty()) {
            return;
        }
        teams.named = true;
        for (k, name) in names.iter().enumerate() {
            let team = k + 1;
            let Some(symbol) = teams.slots[slot(name)].take() else { continue };
            if self.logos.len() <= team {
                self.logos.resize_with(team + 1, || None);
            }
            self.logos[team] = Some(symbol);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classes() -> Value {
        bfe_content::json::parse(
            r#"{"entc":{
                "post_lamp":{"name":"Post_Lamp","base":"commandpost","properties":[
                    ["GeometryName","post_frame"],["HoloOdf","glow_lamp"],["HoloTurnOnTime","1.5"],
                    ["NeutralizeTime","7.0"],["CaptureTime","9.0"],
                    ["HoloImageGeometry","sign_a rep"],["HoloImageGeometry","sign_b CIS"],
                    ["HoloImageGeometry","sign_c villains"],["HoloImageGeometry","broken"]]},
                "post_dark":{"name":"post_dark","base":"commandpost","properties":[["GeometryName","post_frame"]]},
                "glow_lamp":{"name":"glow_lamp","base":"hologram","properties":[
                    ["NeutralColor","10 20 30"],["FriendlyColor","40 50 60"],["EnemyColor","70 80 90"],
                    ["LocalsColor","100 110 120"],["HoloHeight","4.25"],["HoloSize","1.75"],
                    ["HoloLightRadius","6.5"],["HoloImageIntensity","0.25"],["HoloFlickerAlphaMin","0.1"],
                    ["HoloFlickerAlphaMax","0.9"],["HoloHeight","5.0"]]}}}"#,
        )
        .unwrap()
    }

    #[test]
    fn side_names_fall_in_the_games_slots() {
        assert_eq!([slot("Alliance"), slot("all"), slot("EMPIRE"), slot("imp")], [1, 1, 2, 2]);
        assert_eq!([slot("republic"), slot("rep"), slot("CIS"), slot("neutral")], [3, 3, 4, 0]);
        assert_eq!([slot("hero"), slot("heroes"), slot("Villain"), slot("villains")], [6, 6, 7, 7]);
        assert_eq!([slot("jawas"), slot("")], [5, 5]);
    }

    #[test]
    fn a_2005_mission_describes_its_holograms_from_its_classes() {
        let skeletons = bfe_content::json::parse(
            r#"{"Post_Frame":[
                {"name":"root","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0.5,0,1]},
                {"name":"arm","parent":0,"local":[1,0,0,0,0,1,0,0,0,0,1,0,2,0,0,1]},
                {"name":"HP_Hologram","parent":1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,1,0,1]}]}"#,
        )
        .unwrap();
        // A quarter turn about +y, then a move to (10, 0, -20).
        let turned = [0., 0., -1., 0., 0., 1., 0., 0., 1., 0., 0., 0., 10., 0., -20., 1.];
        let posts = vec![
            serde_json::json!({"name":"alpha","class":"POST_LAMP","matrix":turned}),
            serde_json::json!({"name":"beta","class":"post_dark","matrix":turned}),
            serde_json::json!({"name":"","class":"post_lamp","matrix":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}),
            serde_json::json!({"name":"gamma","class":"no_such_class"}),
        ];
        let d = describe(&posts, &classes(), Some(&skeletons)).expect("holograms");
        // The mount (2, 1.5, 0) in the post, turned and moved; the dark post
        // and the unknown class have none; a nameless post is named by place.
        assert_eq!(d.posts.len(), 2);
        assert_eq!((d.posts[0].0, d.posts[0].1.as_str()), (0, "alpha"));
        for (got, want) in d.posts[0].2.iter().zip([10., 1.5, -22.]) {
            assert!((got - want).abs() < 1e-9, "{:?}", d.posts[0].2);
        }
        assert_eq!((d.posts[1].0, d.posts[1].1.as_str(), d.posts[1].2), (2, "#2", [2., 1.5, 0.]));
        // The last value of a repeated property counts; absent ones take the
        // released defaults.
        assert_eq!(d.look["height"], 5.0);
        assert_eq!(d.look["imageIntensity"], 0.25);
        assert_eq!(d.look["flickerAlpha"], serde_json::json!([0.1, 0.9]));
        assert_eq!(d.look["visibleDistance"], 200.0);
        assert_eq!((d.size, d.light_radius), (1.75, 6.5));
        assert_eq!(d.colors, [[10., 20., 30.], [40., 50., 60.], [70., 80., 90.], [100., 110., 120.]]);
        assert_eq!((d.turn_on, d.neutralize_time, d.capture_time), (1.5, 7., 9.));
        assert_eq!(d.symbols, vec![(3, "sign_a".into()), (4, "sign_b".into()), (7, "sign_c".into())]);
    }

    #[test]
    fn a_mission_without_holograms_describes_none() {
        let posts = vec![serde_json::json!({"name":"beta","class":"post_dark"})];
        assert!(describe(&posts, &classes(), None).is_none());
        assert!(describe(&[], &classes(), None).is_none());
    }

    #[test]
    fn teams_take_their_slots_symbol_once_named() {
        let mut teams = Teams { slots: Default::default(), named: false };
        teams.slots[3] = Some((vec![Model { id: 7, values: vec![] }], true));
        teams.slots[6] = Some((vec![Model { id: 9, values: vec![] }], false));
        let mut h = Holograms {
            owner: Posts::new(Descriptor::default()),
            posts: vec![],
            pads: vec![],
            colors: [[0.; 3]; 4],
            height: 0.,
            size: 1.,
            turn_on: 0.,
            trace: false,
            neutralize_time: 0.,
            capture_time: 0.,
            light_radius: 0.,
            beam: None,
            flare: None,
            halo: None,
            logos: vec![],
            fields: Fields::default(),
            time: 0.,
            seed: 1,
            data: 0,
            matrices: 0,
            drawn: 0,
            draws: 0,
            swbf2: Some(Box::new(teams)),
        };
        // Unnamed teams wait; then "rep" shows slot 3's symbol, a name with
        // no symbol shows none, and a later renaming changes nothing.
        h.name_teams(&["", ""]);
        assert!(h.logos.is_empty());
        h.name_teams(&["REP", "villain"]);
        assert_eq!(h.logos.len(), 2);
        assert_eq!(h.logos[1].as_ref().map(|(m, s)| (m[0].id, *s)), Some((7, true)));
        h.name_teams(&["hero", "hero"]);
        assert_eq!(h.logos[1].as_ref().map(|(m, _)| m[0].id), Some(7));
        assert!(h.logos.get(2).is_none_or(Option::is_none));
    }
}
