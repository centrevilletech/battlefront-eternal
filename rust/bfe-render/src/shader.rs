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
    // The temporal upscaler's motion (motion-vectors.js: a moving body's
    // proxy, the first-person view's locked mask), drawn by the motion pass
    // (motion.rs): instanced as Skinned and Rigid are, each instance's
    // matrices, bones' base and last frame's placement read from its row
    // (motion()); its programs read no frame block, the pass sets the
    // uniforms the frame shares (the projection, the view-projections now
    // and last frame, the depth range).
    Motion,
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
];

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
    let declared: Vec<_> = EXTRAS.iter().filter(|&&(n, t, _, _)| source.lines().any(|l| declaration(l) == Some((t, n)))).collect();
    let mut out = String::new();
    for &&(name, ty, _, _) in &declared {
        out.push_str(&format!("{ty} {name};\n"));
    }
    out.push_str("void bfeLoadExtras() {\n");
    for &&(name, _, texel, parts) in &declared {
        let swizzle = if parts.is_empty() { String::new() } else { format!(".{parts}") };
        out.push_str(&format!("  {name} = bfeTexel({}){swizzle};\n", ROW_EXTRA + texel));
    }
    out.push_str("}\n");
    out
}

pub fn rewrite(source: &str, stage: Stage, kind: Kind) -> Result<String, String> {
    if kind == Kind::Motion {
        return if stage == Stage::Vertex { motion(source) } else { Ok(source.to_string()) };
    }
    if kind == Kind::Plain {
        return Ok(source.to_string());
    }
    let instanced = kind == Kind::Skinned || kind == Kind::Rigid;
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

// A motion program's row (Kind::Motion; motion.rs writes it): an
// instance's matrices where an instance row has them (ROW_MODEL,
// ROW_MODEL_VIEW, ROW_NORMAL, ROW_BIND_INVERSE, ROW_BIND), its bones' base
// in the bone texture at ROW_COUNTS .y and last frame's in previousBones at
// .z, last frame's model matrix in the point lights' texels and last
// frame's bind inverse in the extras' (a motion program reads neither).
pub const ROW_PREVIOUS_MODEL: usize = ROW_LIGHTS;
pub const ROW_PREVIOUS_BIND_INVERSE: usize = ROW_EXTRA;

const MOTION_VERTEX: &str = "uniform highp sampler2D bfeInstances;
uniform int bfeInstanceBase;
int bfeRow; int bfeBoneBase; int bfePreviousBoneBase;
mat4 modelMatrix; mat4 modelViewMatrix; mat3 normalMatrix; mat4 bindMatrixInverse; mat4 bindMatrix; mat4 previousModel; mat4 previousBindInverse;
vec4 bfeTexel(int x) { return texelFetch(bfeInstances, ivec2(x, bfeRow), 0); }
void bfeLoad() {
  bfeRow = bfeInstanceBase + gl_InstanceID;
  modelMatrix = mat4(bfeTexel(0), bfeTexel(1), bfeTexel(2), bfeTexel(3));
  modelViewMatrix = mat4(bfeTexel(4), bfeTexel(5), bfeTexel(6), bfeTexel(7));
  normalMatrix = mat3(bfeTexel(8).xyz, bfeTexel(9).xyz, bfeTexel(10).xyz);
  bindMatrixInverse = mat4(bfeTexel(11), bfeTexel(12), bfeTexel(13), bfeTexel(14));
  vec4 counts = bfeTexel(16); bfeBoneBase = int(counts.y); bfePreviousBoneBase = int(counts.z);
  previousModel = mat4(bfeTexel(17), bfeTexel(18), bfeTexel(19), bfeTexel(20));
  bindMatrix = mat4(bfeTexel(21), bfeTexel(22), bfeTexel(23), bfeTexel(24));
  previousBindInverse = mat4(bfeTexel(25), bfeTexel(26), bfeTexel(27), bfeTexel(28));
}
";

// The uniforms a motion program takes from its row instead.
const MOTION_PER_INSTANCE: &[&str] = &["modelMatrix", "modelViewMatrix", "normalMatrix", "bindMatrixInverse", "bindMatrix", "previousModel", "previousBindInverse"];

// Kind::Motion's vertex stage: the row's inputs (MOTION_VERTEX) where
// three's prefix declares its first uniform, the declarations they replace
// dropped, three's bone read (getBoneMatrix) and the proxy's read of last
// frame's bones (motion-vectors.js previousBone) offset by the row's bases,
// and the row loaded first in main. The rest is three's as it compiled it:
// its projection and the frame's other values stay uniforms.
fn motion(source: &str) -> Result<String, String> {
    let mut out = String::with_capacity(source.len() + 2048);
    let mut inserted = false;
    for line in source.lines() {
        if let Some((_, name)) = declaration(line) {
            if !inserted {
                out.push_str(MOTION_VERTEX);
                inserted = true;
            }
            if MOTION_PER_INSTANCE.contains(&name) {
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    if !inserted {
        return Err("no uniform declarations found".into());
    }
    out = out.replacen("int j = int( i ) * 4;", "int j = ( bfeBoneBase + int( i ) ) * 4;", 1);
    out = out.replacen("int j = int(i) * 4;", "int j = (bfePreviousBoneBase + int(i)) * 4;", 1);
    if out.contains("previousBones") && !out.contains("bfePreviousBoneBase + int(i)") {
        return Err("previousBone not found".into());
    }
    let main = out.find("void main() {").ok_or("no main")?;
    out.insert_str(main + "void main() {".len(), "\n  bfeLoad();");
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

    #[test]
    fn a_motion_proxy_reads_its_placement_and_both_frames_bones_from_its_row() {
        // three's prefix, its skinning chunk and the proxy's own declarations
        // (motion-vectors.js), as three compiles a skinned proxy.
        let vs = "#version 300 es\n#define USE_SKINNING\nuniform mat4 modelMatrix;\nuniform mat4 modelViewMatrix;\nuniform mat4 projectionMatrix;\nuniform mat4 viewMatrix;\nuniform mat3 normalMatrix;\n\
            uniform mat4 bindMatrix;\nuniform mat4 bindMatrixInverse;\nuniform highp sampler2D boneTexture;\nmat4 getBoneMatrix( const in float i ) {\n  int j = int( i ) * 4;\n  return mat4(float(j));\n}\n\
            uniform mat4 currentViewProjection;\nuniform mat4 previousModel;\nuniform mat4 previousBindInverse;\nuniform highp sampler2D previousBones;\n\
            mat4 previousBone(const in float i) {\n  int j = int(i) * 4;\n  return mat4(float(j));\n}\nin vec3 position;\nvoid main() {\n  gl_Position = projectionMatrix * modelViewMatrix * previousModel * vec4(position, 1.0);\n}\n";
        let out = rewrite(vs, Stage::Vertex, Kind::Motion).unwrap();
        for gone in ["uniform mat4 modelMatrix;", "uniform mat4 modelViewMatrix;", "uniform mat3 normalMatrix;", "uniform mat4 bindMatrix;", "uniform mat4 bindMatrixInverse;",
            "uniform mat4 previousModel;", "uniform mat4 previousBindInverse;"] {
            assert!(!out.contains(gone), "{gone} declared still");
        }
        for kept in ["uniform mat4 projectionMatrix;", "uniform mat4 viewMatrix;", "uniform mat4 currentViewProjection;", "uniform highp sampler2D boneTexture;", "uniform highp sampler2D previousBones;"] {
            assert!(out.contains(kept), "{kept} is the pass's to set");
        }
        assert!(!out.contains("BfeFrame"), "no frame block");
        assert!(out.contains("int j = ( bfeBoneBase + int( i ) ) * 4;") && out.contains("int j = (bfePreviousBoneBase + int(i)) * 4;"));
        assert!(out.contains("void main() {\n  bfeLoad();"));
        assert!(out.find("uniform highp sampler2D bfeInstances;").unwrap() < out.find("uniform mat4 projectionMatrix;").unwrap(), "the row's inputs where the prefix starts");
        assert!(out.contains("previousModel = mat4(bfeTexel(17)") && out.contains("previousBindInverse = mat4(bfeTexel(25)"));
        // The fragment stage is three's as it is; a source that reads last
        // frame's bones in a form the rewrite does not know is refused.
        let fs = "#version 300 es\nuniform highp sampler2D sceneDepth;\nvoid main() {}\n";
        assert_eq!(rewrite(fs, Stage::Fragment, Kind::Motion).unwrap(), fs);
        assert!(rewrite(&vs.replace("int j = int(i) * 4;", "int j = 4 * int(i);"), Stage::Vertex, Kind::Motion).is_err());
        // The locked mask: no skinning, no last frame.
        let locked = rewrite("#version 300 es\nuniform mat4 modelViewMatrix;\nuniform mat4 projectionMatrix;\nin vec3 position;\nvoid main() {\n  gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);\n}\n", Stage::Vertex, Kind::Motion).unwrap();
        assert!(locked.contains("bfeLoad();") && !locked.contains("uniform mat4 modelViewMatrix;"));
    }
}
