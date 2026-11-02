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
// indices of the frame's point lights that reach the instance.
pub const ROW_MODEL: usize = 0;
pub const ROW_MODEL_VIEW: usize = 4;
pub const ROW_NORMAL: usize = 8;
pub const ROW_BIND_INVERSE: usize = 11;
pub const ROW_LOOK: usize = 15;
pub const ROW_COUNTS: usize = 16;
pub const ROW_LIGHTS: usize = 17;
pub const ROW_BIND: usize = 21;
pub const ROW_TEXELS: usize = 32;

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
}
";

const INSTANCE_FRAGMENT: &str = "flat in int vBfeRow;
int bfeRow;
";

const INSTANCE_FRAGMENT_LOAD: &str = "void bfeLoad() { bfeRow = vBfeRow; bfeLoadLook(); }
";

// Uniforms an instanced program takes from its row instead.
const PER_INSTANCE: &[&str] = &[
    "modelMatrix", "modelViewMatrix", "normalMatrix", "bindMatrixInverse", "bindMatrix",
    "sunScale", "ambientPull", "brightness", "zeroLights", "zeroLightCount",
];

// A uniform declaration on one line: (type, name) when it is one.
fn declaration(line: &str) -> Option<(&str, &str)> {
    let t = line.trim();
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

pub fn rewrite(source: &str, stage: Stage, kind: Kind) -> Result<String, String> {
    let instanced = kind != Kind::World;
    let mut out = String::with_capacity(source.len() + 4096);
    let mut inserted = false;
    for line in source.lines() {
        if let Some((ty, name)) = declaration(line) {
            if !inserted {
                // Three's prefix declares its own uniforms after every define,
                // outside any condition: the block and the instance inputs go
                // there, where every define they test is already set.
                out.push_str(FRAME_BLOCK);
                if instanced {
                    out.push_str(if stage == Stage::Vertex { INSTANCE_VERTEX } else { INSTANCE_FRAGMENT });
                    out.push_str(INSTANCE_COMMON);
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
