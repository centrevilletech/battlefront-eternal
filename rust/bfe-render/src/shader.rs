// Programs made from the GLSL three.js compiled for a material, so they
// light exactly as three's do, with only where their inputs come from
// changed:
//
// - Every value the frame shares (camera, sun, ambient, fade, the frame's
//   point lights...) comes from one std140 uniform block, BfeFrame, written
//   once a frame, instead of a uniform call a value, a program and a frame.
// - Instanced programs (soldiers' bodies, the guns they hold) read what three
//   set per object (its matrices, light-region values, point lights) from a
//   row of an instance texture, one row an instance; a body's bones come
//   from one texture holding every drawn soldier's.
//
// The rest of three's source is left as it is: the same lighting code runs.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Vertex,
    Fragment,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    // One draw a mesh, model matrix the identity (the map's own meshes).
    World,
    // Instanced, one bone a vertex (actor.js RIGID_SKIN bodies).
    Skinned,
    // Instanced, no skin (a held gun).
    Rigid,
    // Three's GLSL as it is, every uniform its own: the pass that draws with
    // it sets each one (far.rs), the camera's too. For materials that keep
    // values of their own under a frame block member's name (a water's own
    // `time`).
    Plain,
    // One draw an object whose model matrix is the identity, which its
    // shader reads as modelViewMatrix (the effects' particle batches): the
    // view matrix, from the frame block.
    Effect,
}

// BfeFrame's members as three declares them, in block order. Offsets are in
// floats (std140): see FRAME_OFFSETS.
pub const FRAME_MEMBERS: &[(&str, &str)] = &[
    ("projectionMatrix", "mat4"),
    ("viewMatrix", "mat4"),
    ("cameraPosition", "vec3"),
    ("time", "float"),
    ("sunDirection", "vec3"),
    ("bump", "float"),
    ("sunColor", "vec3"),
    ("shadowStrength", "float"),
    ("ambient", "vec3"),
    ("aaSamples", "float"),
    ("ambientTop", "vec3"),
    ("terrainShade", "float"),
    ("ambientBottom", "vec3"),
    ("farAaSamples", "float"),
    ("characterAmbient", "vec3"),
    ("cascadeNear", "float"),
    ("fogColor", "vec3"),
    ("cascadeFar", "float"),
    ("fade", "vec2"),
    ("resolution", "vec2"),
    ("fogRange", "vec2"),
];

pub const FRAME_OFFSETS: &[usize] = &[0, 16, 32, 35, 36, 39, 40, 43, 44, 47, 48, 51, 52, 55, 56, 59, 60, 63, 64, 66, 68];
pub const FRAME_LIGHT_COUNT: usize = 70;
pub const FRAME_LIGHTS: usize = 72;
pub const FRAME_FLOATS: usize = 72 + 4 * 32;
pub const MAX_LIGHTS: usize = 16;

const FRAME_BLOCK: &str = "layout(std140) uniform BfeFrame {
  mat4 projectionMatrix;
  mat4 viewMatrix;
  vec3 cameraPosition; float time;
  vec3 sunDirection; float bump;
  vec3 sunColor; float shadowStrength;
  vec3 ambient; float aaSamples;
  vec3 ambientTop; float terrainShade;
  vec3 ambientBottom; float farAaSamples;
  vec3 characterAmbient; float cascadeNear;
  vec3 fogColor; float cascadeFar;
  vec2 fade; vec2 resolution;
  vec2 fogRange; float bfeLightCount; float bfeFramePad;
  vec4 bfePointLights[32];
};
";

// Texels of an instance row (RGBA32F): the matrices three would have set,
// then the light region's values, the light count and bone base, then the
// indices of the frame's point lights that reach the instance, the bind
// matrix, then a material's own values the instance gives (EXTRAS).
pub const ROW_MODEL: usize = 0;
pub const ROW_MODEL_VIEW: usize = 4;
pub const ROW_NORMAL: usize = 8;
pub const ROW_BIND_INVERSE: usize = 11;
pub const ROW_LOOK: usize = 15;
pub const ROW_COUNTS: usize = 16;
pub const ROW_LIGHTS: usize = 17;
pub const ROW_BIND: usize = 21;
pub const ROW_EXTRA: usize = 25;
pub const ROW_EXTRA_TEXELS: usize = 5;
pub const ROW_TEXELS: usize = 32;

// A material's values an instanced program reads from its row when its
// source declares them (each instance's own, as three sets them per draw):
// name, type, texel (from ROW_EXTRA) and its components there. A zero
// material's scroll (a scrolling texture's speed), and the values a heated
// copy of one gives each object (destruction-debris.js heatMaterial: a
// dying droideka's body dissolving).
const EXTRAS: &[(&str, &str, usize, &str)] = &[
    ("scroll", "vec2", 0, "xy"),
    ("heat", "float", 1, "x"), ("heatBase", "float", 1, "y"), ("heatScale", "float", 1, "z"), ("soot", "float", 1, "w"),
    ("heatTear", "vec4", 2, ""), ("dissolve", "float", 3, "x"),
    // The arc caster's shock overlay on a struck body (arc-lightning.js), in
    // the texels a heated body's values use: an overlay is never heated.
    ("shockTime", "float", 1, "x"), ("intensity", "float", 1, "y"), ("seed", "float", 1, "z"), ("inflate", "float", 1, "w"),
    ("color", "vec3", 2, "xyz"),
];

// The shadow receivers' inputs (three's USE_SHADOWMAP programs, the near
// scene's), one std140 block for every program, written once a frame, in
// place of what three sets each program: each cascade's shadow values (three's
// DirectionalLightShadow struct), its world-to-map matrix, the inverse
// transpose that carries plane normals into its map (materials.js
// cascadePlanes) and its slice of the view (cascades). Arrays of four, the
// most cascades a preset has; a program reads the first NUM_DIR_LIGHT_SHADOWS.
// The depth maps stay samplers (cascadeMaps[i], on the core's units 16 + i).
// Offsets in floats: SHADOW_VALUES (8 a cascade: intensity, bias, normal
// bias, radius, map size, 2 pad), SHADOW_MATRICES (16), SHADOW_PLANES (12:
// three columns of four), SHADOW_SLICES (4: the slice, 2 pad).
pub const SHADOW_VALUES: usize = 0;
pub const SHADOW_MATRICES: usize = 32;
pub const SHADOW_PLANES: usize = 96;
pub const SHADOW_SLICES: usize = 144;
pub const SHADOW_FLOATS: usize = 160;
const SHADOW_BLOCK: &str = "layout(std140) uniform BfeShadow {
  DirectionalLightShadow directionalLightShadows[4];
  mat4 directionalShadowMatrix[4];
  mat3 cascadePlanes[4];
  vec2 cascades[4];
};
";
// What the block holds that the source declares again later (after three's
// struct): dropped there.
const SHADOW_MEMBERS: &[(&str, &str)] = &[("directionalShadowMatrix", "mat4"), ("cascadePlanes", "mat3"), ("cascades", "vec2")];
// An instance's own shadow value, from its row (the record's EXTRA + 14),
// when the program receives shadows.
const SHADOW_EXTRAS: &[(&str, &str, usize, &str)] = &[("receiveShadow", "bool", 3, "z")];

// Whether a program's source is one of three's shadow receivers'.
pub fn receives_shadows(source: &str) -> bool {
    source.lines().any(|l| l.trim() == "#define USE_SHADOWMAP")
}

const INSTANCE_COMMON: &str = "uniform highp sampler2D bfeInstances;
float sunScale; vec2 ambientPull; float brightness;
int zeroLightCount; int bfeBoneBase; ivec4 bfeLightIndex[4];
vec4 bfeTexel(int x) { return texelFetch(bfeInstances, ivec2(x, bfeRow), 0); }
void bfeLoadLook() {
  vec4 look = bfeTexel(15); sunScale = look.x; ambientPull = look.yz; brightness = look.w;
  vec4 counts = bfeTexel(16); zeroLightCount = int(counts.x); bfeBoneBase = int(counts.y);
  for (int k = 0; k < 4; k++) bfeLightIndex[k] = ivec4(bfeTexel(17 + k));
}
vec4 bfeLightSphere(int i) { return bfePointLights[2 * bfeLightIndex[i >> 2][i & 3]]; }
vec4 bfeLightColor(int i) { return bfePointLights[2 * bfeLightIndex[i >> 2][i & 3] + 1]; }
";

const INSTANCE_VERTEX: &str = "uniform int bfeInstanceBase;
flat out int vBfeRow;
int bfeRow;
mat4 modelMatrix; mat4 modelViewMatrix; mat3 normalMatrix; mat4 bindMatrixInverse; mat4 bindMatrix;
";

const INSTANCE_VERTEX_LOAD: &str = "void bfeLoad() {
  bfeRow = bfeInstanceBase + gl_InstanceID; vBfeRow = bfeRow;
  modelMatrix = mat4(bfeTexel(0), bfeTexel(1), bfeTexel(2), bfeTexel(3));
  modelViewMatrix = mat4(bfeTexel(4), bfeTexel(5), bfeTexel(6), bfeTexel(7));
  normalMatrix = mat3(bfeTexel(8).xyz, bfeTexel(9).xyz, bfeTexel(10).xyz);
  bindMatrixInverse = mat4(bfeTexel(11), bfeTexel(12), bfeTexel(13), bfeTexel(14));
  bindMatrix = mat4(bfeTexel(21), bfeTexel(22), bfeTexel(23), bfeTexel(24));
  bfeLoadLook();
  bfeLoadExtras();
}
";

const INSTANCE_FRAGMENT: &str = "flat in int vBfeRow;
int bfeRow;
";

const INSTANCE_FRAGMENT_LOAD: &str = "void bfeLoad() { bfeRow = vBfeRow; bfeLoadLook(); bfeLoadExtras(); }
";

// Uniforms an instanced program takes from its row instead.
const PER_INSTANCE: &[&str] = &[
    "modelMatrix", "modelViewMatrix", "normalMatrix", "bindMatrixInverse", "bindMatrix",
    "sunScale", "ambientPull", "brightness", "zeroLights", "zeroLightCount",
];

// A uniform declaration on one line: (type, name) when it is one (a
// comment after it aside).
fn declaration(line: &str) -> Option<(&str, &str)> {
    let t = line.split("//").next().unwrap_or("").trim();
    if !t.starts_with("uniform ") || !t.ends_with(';') {
        return None;
    }
    let mut words = t[8..t.len() - 1].split_whitespace();
    let mut ty = words.next()?;
    if ty == "highp" || ty == "mediump" || ty == "lowp" {
        ty = words.next()?;
    }
    let rest: String = words.collect::<Vec<_>>().join(" ");
    let name = rest.split('[').next()?.trim();
    // The name, without any array size.
    let name_end = name.len();
    let name = &t[t.find(name)?..t.find(name)? + name_end];
    Some((ty, name))
}

// The EXTRAS a source declares, as globals and their load from the row
// (bfeLoadExtras, which bfeLoad calls).
fn extras(source: &str) -> String {
    let shadows: &[_] = if receives_shadows(source) { SHADOW_EXTRAS } else { &[] };
    let declared: Vec<_> = EXTRAS.iter().chain(shadows).filter(|&&(n, t, _, _)| source.lines().any(|l| declaration(l) == Some((t, n)))).collect();
    let mut out = String::new();
    for &&(name, ty, _, _) in &declared {
        out.push_str(&format!("{ty} {name};\n"));
    }
    out.push_str("void bfeLoadExtras() {\n");
    for &&(name, ty, texel, parts) in &declared {
        let swizzle = if parts.is_empty() { String::new() } else { format!(".{parts}") };
        let test = if ty == "bool" { " > 0.5" } else { "" };
        out.push_str(&format!("  {name} = bfeTexel({}){swizzle}{test};\n", ROW_EXTRA + texel));
    }
    out.push_str("}\n");
    out
}

pub fn rewrite(source: &str, stage: Stage, kind: Kind) -> Result<String, String> {
    if kind == Kind::Plain {
        return Ok(source.to_string());
    }
    let instanced = kind == Kind::Skinned || kind == Kind::Rigid;
    // Shadow receivers read the BfeShadow block (World and instanced kinds;
    // a Classic source has no USE_SHADOWMAP and stays as it was).
    let shadows = (kind == Kind::World || instanced) && receives_shadows(source);
    let mut block = false;
    let mut out = String::with_capacity(source.len() + 4096);
    let mut inserted = false;
    for line in source.lines() {
        if let Some((ty, name)) = declaration(line) {
            if !inserted {
                // Three's prefix declares its own uniforms after every define,
                // outside any condition: the block and the instance inputs go
                // there, where every define they test is already set.
                out.push_str(FRAME_BLOCK);
                if kind == Kind::Effect {
                    out.push_str("#define modelViewMatrix viewMatrix\n");
                }
                if instanced {
                    out.push_str(if stage == Stage::Vertex { INSTANCE_VERTEX } else { INSTANCE_FRAGMENT });
                    out.push_str(INSTANCE_COMMON);
                    out.push_str(&extras(source));
                    out.push_str(if stage == Stage::Vertex { INSTANCE_VERTEX_LOAD } else { INSTANCE_FRAGMENT_LOAD });
                }
                inserted = true;
            }
            if let Some(&(_, want)) = FRAME_MEMBERS.iter().find(|(n, _)| *n == name) {
                if want != ty {
                    return Err(format!("{name} is a {ty} here, the frame block holds a {want}"));
                }
                continue;
            }
            if instanced && PER_INSTANCE.contains(&name) {
                continue;
            }
            if instanced && EXTRAS.iter().any(|&(n, t, _, _)| n == name && t == ty) {
                continue;
            }
            if shadows && ty == "DirectionalLightShadow" && name == "directionalLightShadows" {
                out.push_str(SHADOW_BLOCK);
                block = true;
                continue;
            }
            if shadows && SHADOW_MEMBERS.contains(&(name, ty)) {
                continue;
            }
            if shadows && instanced && SHADOW_EXTRAS.iter().any(|&(n, t, _, _)| n == name && t == ty) {
                continue;
            }
            if kind == Kind::Effect && name == "modelViewMatrix" {
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    if !inserted {
        return Err("no uniform declarations found".into());
    }
    if shadows && !block && SHADOW_MEMBERS.iter().any(|&(n, t)| source.lines().any(|l| declaration(l) == Some((t, n)))) {
        return Err("a shadow receiver without three's DirectionalLightShadow uniforms".into());
    }
    if instanced {
        out = out
            .replace("zeroLights[ 2 * i + 1 ]", "bfeLightColor( i )")
            .replace("zeroLights[ 2 * i ]", "bfeLightSphere( i )");
        if out.contains("zeroLights[") {
            return Err("a point-light read in a form the rewrite does not know".into());
        }
        if stage == Stage::Vertex && kind == Kind::Skinned {
            let before = out.len();
            out = out.replacen("int j = int( i ) * 4;", "int j = ( bfeBoneBase + int( i ) ) * 4;", 1);
            if out.len() == before {
                return Err("getBoneMatrix not found".into());
            }
        }
        let main = out.find("void main() {").ok_or("no main")?;
        out.insert_str(main + "void main() {".len(), "\n  bfeLoad();");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn declarations() {
        assert_eq!(declaration("uniform mat4 viewMatrix;"), Some(("mat4", "viewMatrix")));
        assert_eq!(declaration("  uniform vec4 zeroLights[ZERO_LIGHTS * 2];"), Some(("vec4", "zeroLights")));
        assert_eq!(declaration("\tuniform highp sampler2D boneTexture;"), Some(("sampler2D", "boneTexture")));
        assert_eq!(declaration("vec3 x;"), None);
    }
    #[test]
    fn an_instance_reads_a_materials_own_values_from_its_row() {
        let fs = "#version 300 es\nprecision highp float;\n#define HEATED\nuniform vec2 scroll;\nuniform float heat;\nuniform vec4 heatTear; // where it tore\nuniform float dissolve;\nuniform float time;\nout vec4 c;\nvoid main() {\n  c = vec4(scroll, heat + dissolve, heatTear.w + time);\n}\n";
        let out = rewrite(fs, Stage::Fragment, Kind::Skinned).unwrap();
        for gone in ["uniform vec2 scroll;", "uniform float heat;", "uniform vec4 heatTear;", "uniform float dissolve;", "uniform float time;"] {
            assert!(!out.contains(gone), "{gone} declared still");
        }
        assert!(out.contains("vec2 scroll;\nfloat heat;\nvec4 heatTear;\nfloat dissolve;\n"), "each a global of the program's");
        assert!(out.contains("scroll = bfeTexel(25).xy;") && out.contains("heat = bfeTexel(26).x;") && out.contains("heatTear = bfeTexel(27);") && out.contains("dissolve = bfeTexel(28).x;"));
        assert!(!out.contains("heatBase"), "only what the source declares");
        assert!(out.contains("bfeLoadLook(); bfeLoadExtras(); }"));
        // A program that declares none loads none.
        let plain = rewrite("#version 300 es\nuniform float sunScale;\nvoid main() {}\n", Stage::Vertex, Kind::Rigid).unwrap();
        assert!(plain.contains("void bfeLoadExtras() {\n}\n") && plain.contains("bfeLoadExtras();"));
    }

    // A receiver's fragment shader as three compiles a zero material's with
    // shadows on (materials.js; the shadow chunk, then the material's own).
    const RECEIVER: &str = "#version 300 es\n#define USE_SHADOWMAP\n#define NUM_DIR_LIGHT_SHADOWS 2\nprecision highp float;\nuniform mat4 viewMatrix;\nuniform bool receiveShadow;\n#ifdef USE_SHADOWMAP\n\t#if NUM_DIR_LIGHT_SHADOWS > 0\n\t\tuniform sampler2D directionalShadowMap[ NUM_DIR_LIGHT_SHADOWS ];\n\t\tstruct DirectionalLightShadow {\n\t\t\tfloat shadowIntensity;\n\t\t\tfloat shadowBias;\n\t\t\tfloat shadowNormalBias;\n\t\t\tfloat shadowRadius;\n\t\t\tvec2 shadowMapSize;\n\t\t};\n\t\tuniform DirectionalLightShadow directionalLightShadows[ NUM_DIR_LIGHT_SHADOWS ];\n\t#endif\n#endif\nuniform sampler2D map;\nuniform vec2 cascades[4];\nuniform float cascadeFar;\n#if defined( USE_SHADOWMAP ) && NUM_DIR_LIGHT_SHADOWS > 0\nuniform highp sampler2DShadow cascadeMaps[ NUM_DIR_LIGHT_SHADOWS ];\nuniform mat4 directionalShadowMatrix[ NUM_DIR_LIGHT_SHADOWS ];\nuniform mat3 cascadePlanes[ NUM_DIR_LIGHT_SHADOWS ];\n#endif\nout vec4 c;\nvoid main() {\n  c = receiveShadow ? directionalShadowMatrix[ 1 ] * vec4(cascadePlanes[ 0 ] * vec3(cascades[ 1 ], cascadeFar), directionalLightShadows[ 1 ].shadowMapSize.x) : texture(map, vec2(0.0));\n}\n";

    #[test]
    fn a_shadow_receiver_reads_the_shadow_block() {
        for kind in [Kind::World, Kind::Skinned, Kind::Rigid] {
            let out = rewrite(RECEIVER, Stage::Fragment, kind).unwrap();
            assert!(out.contains(SHADOW_BLOCK), "{kind:?}: the block");
            for gone in ["uniform DirectionalLightShadow", "uniform mat4 directionalShadowMatrix[", "uniform mat3 cascadePlanes[", "uniform vec2 cascades[4];", "uniform float cascadeFar;"] {
                assert!(!out.contains(gone), "{kind:?}: {gone} declared still");
            }
            assert!(out.contains("struct DirectionalLightShadow {"), "three's struct stays, before the block");
            assert!(out.find("struct DirectionalLightShadow {").unwrap() < out.find("uniform BfeShadow").unwrap());
            assert!(out.contains("uniform highp sampler2DShadow cascadeMaps[ NUM_DIR_LIGHT_SHADOWS ];"), "the maps stay samplers");
            assert!(out.contains("uniform sampler2D map;"));
        }
        // receiveShadow: the draw's own uniform for the world, an instance's from its row.
        assert!(rewrite(RECEIVER, Stage::Fragment, Kind::World).unwrap().contains("uniform bool receiveShadow;"));
        let skinned = rewrite(RECEIVER, Stage::Fragment, Kind::Skinned).unwrap();
        assert!(!skinned.contains("uniform bool receiveShadow;") && skinned.contains("bool receiveShadow;\n") && skinned.contains("receiveShadow = bfeTexel(28).z > 0.5;"));
        // Not a receiver (no USE_SHADOWMAP): byte for byte as before.
        let classic = RECEIVER.replace("#define USE_SHADOWMAP\n", "");
        for kind in [Kind::World, Kind::Skinned] {
            let out = rewrite(&classic, Stage::Fragment, kind).unwrap();
            assert!(!out.contains("BfeShadow") && out.contains("uniform DirectionalLightShadow directionalLightShadows[ NUM_DIR_LIGHT_SHADOWS ];") && out.contains("uniform vec2 cascades[4];"));
            assert!(out.contains("uniform bool receiveShadow;"), "{kind:?}");
        }
        // The plain and effect kinds are three's GLSL as it is (no receivers).
        assert!(!rewrite(RECEIVER, Stage::Fragment, Kind::Plain).unwrap().contains("BfeShadow"));
        // Offsets in floats as std140 lays the block out.
        assert_eq!((SHADOW_VALUES, SHADOW_MATRICES, SHADOW_PLANES, SHADOW_SLICES, SHADOW_FLOATS), (0, 4 * 8, 4 * 8 + 4 * 16, 4 * 8 + 4 * 16 + 4 * 12, 4 * 8 + 4 * 16 + 4 * 12 + 4 * 4));
    }

    #[test]
    fn a_shock_overlay_reads_its_values_from_the_heat_texels() {
        let fs = "#version 300 es\nprecision highp float;\nuniform float shockTime;\nuniform float intensity;\nuniform float seed;\nuniform float inflate;\nuniform vec3 color;\nout vec4 c;\nvoid main() {\n  c = vec4(color * intensity, shockTime + seed + inflate);\n}\n";
        let out = rewrite(fs, Stage::Fragment, Kind::Skinned).unwrap();
        for gone in ["uniform float shockTime;", "uniform float intensity;", "uniform float seed;", "uniform float inflate;", "uniform vec3 color;"] {
            assert!(!out.contains(gone), "{gone} declared still");
        }
        assert!(out.contains("shockTime = bfeTexel(26).x;") && out.contains("intensity = bfeTexel(26).y;") && out.contains("seed = bfeTexel(26).z;")
            && out.contains("inflate = bfeTexel(26).w;") && out.contains("color = bfeTexel(27).xyz;"));
        // A colour that is an attribute (a zero material's vertex colour) is not one.
        let vs = rewrite("#version 300 es\nuniform float sunScale;\nin vec4 color;\nvoid main() { gl_Position = color; }\n", Stage::Vertex, Kind::Rigid).unwrap();
        assert!(vs.contains("in vec4 color;") && !vs.contains("bfeTexel(27)"));
    }

    #[test]
    fn an_effect_reads_the_view_matrix_for_its_model_view() {
        let vs = "#version 300 es\nuniform mat4 modelMatrix;\nuniform mat4 modelViewMatrix;\nuniform mat4 projectionMatrix;\nuniform mat4 viewMatrix;\nin vec3 p;\nvoid main() {\n  gl_Position = projectionMatrix * modelViewMatrix * vec4(p, 1.0);\n}\n";
        let out = rewrite(vs, Stage::Vertex, Kind::Effect).unwrap();
        assert!(out.contains("#define modelViewMatrix viewMatrix"));
        assert!(!out.contains("uniform mat4 modelViewMatrix;"));
        assert!(!out.contains("uniform mat4 viewMatrix;") && !out.contains("uniform mat4 projectionMatrix;"));
        assert!(out.contains("uniform mat4 modelMatrix;"), "the rest of three's declarations stay");
        assert!(!out.contains("bfeLoad"), "not instanced");
        let world = rewrite(vs, Stage::Vertex, Kind::World).unwrap();
        assert!(world.contains("uniform mat4 modelViewMatrix;") && !world.contains("#define modelViewMatrix"));
    }
    #[test]
    fn frame_offsets_are_std140() {
        // Each member's offset as std140 lays the block out.
        let sizes = |ty: &str| match ty { "mat4" => (16, 4), "vec3" => (3, 4), "vec2" => (2, 2), _ => (1, 1) };
        let mut at = 0usize;
        for (i, (_, ty)) in FRAME_MEMBERS.iter().enumerate() {
            let (size, align) = sizes(ty);
            at = (at + align - 1) / align * align;
            assert_eq!(at, FRAME_OFFSETS[i], "{i}");
            at += size;
        }
        assert_eq!(at, 70);
    }
}
