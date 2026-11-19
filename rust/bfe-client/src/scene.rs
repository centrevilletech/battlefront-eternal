//! Native scene admission for inspecting prepared maps before a live World
//! exists. Asset work ends before the first frame; the free camera publishes
//! generated commands through Host and the renderer borrows one fixed frame.
/// The water settings a scene takes (waterQuality, oceanEffects).
pub use crate::water_host::Settings as WaterSettings;
use crate::{
    canvas::Canvas,
    generated_input::*,
    host::World,
    input::ViewActions,
    render_bridge::Bridge,
    resources::{Ranges, Resources},
};
use bfe_content_native::{
    loader::{Blend, MaterialKind},
    Loader,
};
use bfe_render::{shader, static_layout as L};
use glow::HasContext;
use std::path::Path;
use three::{CoordinateSystem, Matrix4, Vector3};

// Visibility is independent of execution order. Component probes retain
// the same semantic role bank as the normal interactive scene.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Component {
    All,
    Props,
    Foliage,
    Water,
    Sky,
}
#[derive(Clone, Copy)]
pub struct View {
    pub values: [f64; shader::FRAME_FLOATS],
}
pub struct MapCamera {
    pub position: [f64; 3],
    pub first_person: bool,
    yaw: f64,
    pitch: f64,
    time: f64,
    view: View,
    slots: [usize; 3],
    phase_camera: bool,
    subject_eye: [f64; 3],
    flight_mode: bool,
    flight_first_person: bool,
    far_plane: f64,
}
impl MapCamera {
    pub fn new(position: [f64; 3], size: [i32; 2]) -> Result<Self, String> {
        if position.iter().any(|v| !v.is_finite()) || size.iter().any(|n| !(1..=8192).contains(n)) {
            return Err("invalid native camera".into());
        }
        let mut view = View {
            values: [0.0; shader::FRAME_FLOATS],
        };
        let mut projection = Matrix4::default();
        let top = 0.3 * (60.0f64.to_radians() * 0.5).tan();
        let right = top * size[0] as f64 / size[1] as f64;
        projection.make_perspective(
            -right,
            right,
            top,
            -top,
            0.3,
            20000.0,
            CoordinateSystem::WebGL,
        );
        set(&mut view, "projectionMatrix", &projection.elements);
        // The inspector has an explicit neutral environment. Recorded battles
        // retain their authored lighting/post chain through the session host.
        for (name, values) in [
            ("sunDirection", &[0.4, 0.8, 0.4][..]),
            ("sunColor", &[0.6, 0.6, 0.6]),
            ("ambient", &[0.5, 0.5, 0.5]),
            ("ambientTop", &[0.5, 0.5, 0.5]),
            ("ambientBottom", &[0.3, 0.3, 0.3]),
            ("characterAmbient", &[0.5, 0.5, 0.5]),
            ("aaSamples", &[1.0]),
            ("farAaSamples", &[1.0]),
            ("bump", &[1.0]),
            ("terrainShade", &[1.0]),
            ("resolution", &[size[0] as f64, size[1] as f64]),
            ("fogRange", &[19999.0, 20000.0]),
            ("fade", &[0.0, 20000.0]),
        ] {
            set(&mut view, name, values);
        }
        let slots = ["viewMatrix", "cameraPosition", "time"].map(|name| {
            shader::FRAME_OFFSETS[shader::FRAME_MEMBERS
                .iter()
                .position(|(n, _)| *n == name)
                .expect("frame catalog member")]
        });
        Ok(Self {
            position,
            first_person: false,
            yaw: 0.0,
            pitch: -0.4,
            time: 0.0,
            view,
            slots,
            phase_camera: false,
            subject_eye: position,
            flight_mode: false,
            flight_first_person: false,
            far_plane: 20000.0,
        })
    }
    pub fn resize(&mut self, size: [i32; 2]) {
        if size[0] <= 0 || size[1] <= 0 {
            return;
        }
        let mut projection = Matrix4::default();
        let top = 0.3 * (60.0f64.to_radians() * 0.5).tan();
        let right = top * size[0] as f64 / size[1] as f64;
        projection.make_perspective(
            -right,
            right,
            top,
            -top,
            0.3,
            self.far_plane,
            CoordinateSystem::WebGL,
        );
        set(&mut self.view, "projectionMatrix", &projection.elements);
        set(&mut self.view, "resolution", &[size[0] as f64, size[1] as f64]);
    }
    pub fn set_view_distance(&mut self, range: [f64; 2]) {
        self.set_view_range(range, range[1]);
    }
    /// The near fade and, separately, the near projection's far plane (at the SHORT view distance the fade
    /// ends early while units and vehicles still draw out to the authored range).
    pub fn set_view_range(&mut self, fade: [f64; 2], far: f64) {
        self.far_plane = far;
        set(&mut self.view, "fade", &fade);
        self.view.values[10] = -(far + 0.3) / (far - 0.3);
        self.view.values[14] = -0.6 * far / (far - 0.3);
    }
    pub fn subject_eye(&self) -> [f64; 3] {
        self.subject_eye
    }
    // Boarding loads flightFirstPerson. Leaving keeps the active view, as
    // VehicleDriver.exit does; later ground toggles do not change that setting.
    pub fn set_flight_perspective(&mut self, first: bool) {
        self.flight_first_person = first;
        if self.flight_mode {
            self.first_person = first;
        }
    }
    pub fn set_flight_mode(&mut self, flight: bool) {
        if self.flight_mode != flight {
            if flight {
                self.first_person = self.flight_first_person;
            } else {
                self.flight_first_person = self.first_person;
            }
            self.flight_mode = flight;
        }
    }
    pub fn aim(&self) -> [f64; 2] {
        [self.yaw - std::f64::consts::PI, self.pitch]
    }
    pub fn snapshot(&self)->&View{&self.view}
    pub(crate) fn drawn_view(&mut self, view: View) {
        self.view = view;
        self.phase_camera = true;
        self.position.copy_from_slice(&view.values[self.slots[1]..self.slots[1] + 3]);
        self.far_plane = view.values[14] / (view.values[10] + 1.);
    }
    /// The far plane's distance, which bounds the lights the camera packs.
    pub fn far(&self) -> f64 {
        self.far_plane
    }
    /// The frame's point lights, eight floats each, as the shaders read
    /// them from the frame's values.
    pub fn set_lights(&mut self, lights: &[f64], count: usize) {
        let count = count.min(shader::MAX_LIGHTS).min(lights.len() / 8);
        self.view.values[shader::FRAME_LIGHT_COUNT] = count as f64;
        self.view.values[shader::FRAME_LIGHTS..shader::FRAME_LIGHTS + count * 8].copy_from_slice(&lights[..count * 8]);
    }
    pub fn aspect(&self)->f64{let projection=&self.view.values[..16];projection[5]/projection[0]}
    /// A camera phase has already selected and obstructed this view. Publish
    /// its exact matrices instead of deriving them again from yaw and pitch.
    pub fn accept_camera(&mut self, output: &[f64]) {
        use bfe_presentation::layout::camera_output as o;
        self.phase_camera = true;
        self.position.copy_from_slice(&output[o::POSITION..o::POSITION + 3]);
        self.view.values[..16].copy_from_slice(&output[o::PROJECTION..o::PROJECTION + 16]);
        self.view.values[16..32].copy_from_slice(&output[o::INVERSE..o::INVERSE + 16]);
        let at = self.slots[1];
        self.view.values[at..at + 3].copy_from_slice(&self.position);
    }
    pub fn follow(&mut self, position: [f64; 3], yaw: f64, pitch: f64, dt: f64) {
        // CameraPhase eases from its previous displayed pose. Following the
        // subject updates input aim without replacing that pose with its eye.
        self.subject_eye = position;
        if !self.phase_camera {
            self.position = position;
        }
        self.yaw = yaw + std::f64::consts::PI;
        self.pitch = pitch;
        self.time += dt;
    }

}
impl World for MapCamera {
    type Frame = View;
    fn step(
        &mut self,
        c: &ClientCommand,
        look: &ClientLook,
        _: &ClientFlightCommand,
        _: &ViewActions,
        dt: f64,
    ) -> Result<(), String> {
        // Inspection never invents a soldier, generation or weapon authority.
        // Relative pixels use the same device mapping as a future live World.
        let safe = |v: f64| {
            if v.is_finite() {
                v.clamp(-1e6, 1e6)
            } else {
                0.0
            }
        };
        self.yaw = (self.yaw - safe(look.x) * 0.002).rem_euclid(std::f64::consts::TAU);
        self.pitch = (self.pitch - safe(look.y) * 0.002).clamp(-1.55, 1.55);
        let held = |key: ClientHeld| f64::from(u8::from(c.held & (1 << (key as u8)) != 0));
        let forward = held(ClientHeld::Forward) - held(ClientHeld::Backward);
        let right = held(ClientHeld::Right) - held(ClientHeld::Left);
        let norm = (forward * forward + right * right).sqrt().max(1.0);
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.134)
        } else {
            0.0
        };
        let speed = 24.0 * dt / norm;
        self.position[0] += (-self.yaw.sin() * forward + self.yaw.cos() * right) * speed;
        self.position[1] += self.pitch.sin() * forward * speed + held(ClientHeld::Space) * speed;
        self.position[2] += (-self.yaw.cos() * forward - self.yaw.sin() * right) * speed;
        self.time += dt;
        Ok(())
    }
    fn present(&mut self, _: f64) -> &View {
        if self.phase_camera {
            self.view.values[self.slots[2]] = self.time;
            return &self.view;
        }
        let eye = Vector3::new(self.position[0], self.position[1], self.position[2]);
        let target = Vector3::new(
            eye.x - self.yaw.sin() * self.pitch.cos(),
            eye.y + self.pitch.sin(),
            eye.z - self.yaw.cos() * self.pitch.cos(),
        );
        let mut matrix = Matrix4::default();
        matrix.look_at(&eye, &target, &Vector3::new(0.0, 1.0, 0.0));
        matrix.set_position_v3(&eye).invert();
        let [view, eye, time] = self.slots;
        self.view.values[view..view + 16].copy_from_slice(&matrix.elements);
        self.view.values[eye..eye + 3].copy_from_slice(&self.position);
        self.view.values[time] = self.time;
        &self.view
    }
}
// Names and offsets belong to the renderer's frame catalog. No client copy of
// a shader or input schema is maintained alongside this admission boundary.
fn set(view: &mut View, name: &str, values: &[f64]) {
    if let Some(i) = shader::FRAME_MEMBERS.iter().position(|(n, _)| *n == name) {
        let at = shader::FRAME_OFFSETS[i];
        view.values[at..at + values.len()].copy_from_slice(values);
    }
}

// NearSceneRange holds the fade's start and end at the 2004 game's shortest and
// longest view distance: [start short, start long, end short, end long]. The
// longest pair is the authored default; a short pair that is missing or not
// below it falls back to the longest.
fn short_fade(range: &serde_json::Value, long: [f64; 2]) -> [f64; 2] {
    let (start, end) = (range[0].as_f64().unwrap_or(f64::NAN), range[2].as_f64().unwrap_or(f64::NAN));
    if start.is_finite() && end.is_finite() && start >= 0. && end > start.max(0.3) && end <= long[1] { [start, end] } else { long }
}
#[derive(Clone,Copy)]
pub(crate) struct TextureOptions{pub compressed:bool,pub max_size:u32,pub capabilities:u32}
impl TextureOptions{
    pub fn discover(gl:&glow::Context,compressed:bool)->Self{
        let ext=gl.supported_extensions();
        let capabilities=texture_capabilities(|name|ext.contains(name),&unsafe{gl.get_parameter_string(glow::RENDERER)},gl.version().is_embedded);
        Self{compressed,max_size:unsafe{gl.get_parameter_i32(glow::MAX_TEXTURE_SIZE)}.max(1) as u32,capabilities}
    }
}
// The formats the GPU samples itself (texture_format::Capabilities bits). ETC2 is
// core in GLES 3.0 and listed by desktop GL with GL_ARB_ES3_compatibility, but desktop
// GPUs before Intel's Broadwell (and AMD's and NVIDIA's) have no ETC2 sampler, so Mesa
// decodes every upload on the CPU into four bytes a texel, on desktop GL and GLES alike.
// So ETC2 counts only on a GLES context whose GPU has no desktop format (S3TC, BPTC):
// a phone-class GPU, which samples it itself. A desktop GPU with neither BC7 nor ASTC
// (Sandy Bridge) takes BC1/BC3.
fn texture_capabilities(has:impl Fn(&str)->bool,renderer:&str,embedded:bool)->u32{
    let mut capabilities=0;
    if has("GL_KHR_texture_compression_astc_ldr"){capabilities|=1;}
    if has("GL_ARB_texture_compression_bptc")||has("GL_EXT_texture_compression_bptc"){capabilities|=2;}
    if has("GL_EXT_texture_compression_s3tc"){capabilities|=4;}
    if embedded&&capabilities&6==0{capabilities|=8;}
    if bfe_render::texture_format::unpacks_bc7(renderer){capabilities|=64;}
    capabilities
}
/// An RGBA image's grey and alpha, when its red, green and blue are equal in every texel.
fn grey_alpha(rgba: &[u8]) -> Option<Vec<u8>> {
    let (texels, rest) = rgba.as_chunks::<4>();
    if !rest.is_empty() || !texels.iter().all(|p| p[0] == p[1] && p[1] == p[2]) {
        return None;
    }
    Some(texels.iter().flat_map(|p| [p[0], p[3]]).collect())
}

#[cfg(test)]
mod short_fade_tests {
    // Endor's NearSceneRange: full detail to 60 m at the 2004 game's shortest distance.
    #[test]
    fn short_view_distance_reads_the_first_and_third_values() {
        assert_eq!(super::short_fade(&serde_json::json!([50.0, 180.0, 60.0, 220.0]), [180., 220.]), [50., 60.]);
        assert_eq!(super::short_fade(&serde_json::json!([300.0, 180.0, 400.0, 220.0]), [180., 220.]), [180., 220.]);
        assert_eq!(super::short_fade(&serde_json::Value::Null, [180., 220.]), [180., 220.]);
    }
}
#[cfg(test)]
mod texture_capability_tests {
    use bfe_render::texture_format::{choose,Capabilities,Format};
    // Sandy Bridge on desktop GL lists ES3 compatibility and S3TC, no BC7.
    #[test]
    fn desktop_gl_without_bc7_takes_s3tc_not_emulated_etc2() {
        for embedded in [false,true]{
            let bits=super::texture_capabilities(|n|matches!(n,"GL_ARB_ES3_compatibility"|"GL_EXT_texture_compression_s3tc"),"Mesa Intel(R) HD Graphics 2000 (SNB GT1)",embedded);
            let c=Capabilities::from_bits(bits);
            assert_eq!(choose(c,true,false,256,256,bits&64!=0),Format::Bc1);
            assert_eq!(choose(c,true,true,256,256,bits&64!=0),Format::Bc3);
        }
    }
    #[test]
    fn only_a_grey_image_keeps_grey_and_alpha() {
        assert_eq!(super::grey_alpha(&[255, 255, 255, 7, 0, 0, 0, 0]), Some(vec![255, 7, 0, 0]));
        assert_eq!(super::grey_alpha(&[255, 255, 254, 7]), None);
    }
    // A GLES 3.0 GPU with no ASTC, BPTC or S3TC samples ETC2 (core there) rather than taking RGBA8.
    #[test]
    fn a_gles_gpu_with_no_desktop_format_takes_etc2() {
        let bits=super::texture_capabilities(|_|false,"Mali-G52",true);
        assert_eq!(choose(Capabilities::from_bits(bits),true,false,256,256,false),Format::Etc2Rgb);
        assert_eq!(choose(Capabilities::from_bits(super::texture_capabilities(|_|false,"llvmpipe",false)),true,false,256,256,false),Format::Rgba);
    }
}
/// Exclusively owned CPU assets moved from admission to the GL thread.
pub(crate) struct Prepared {
    spawn_objectives: [Vec<String>; 2],
    spawn_vehicles: std::collections::HashMap<String, [Vec<String>; 2]>,
    menu_grid: Option<u32>,
    menu_models: Vec<crate::menu_models::Model>,
    spawn_shots: Vec<bfe_battle_ui::spawn::CameraShot>,
    content: bfe_content_native::BattleContent,
    layer_ids: Vec<Vec<usize>>,
    details: Vec<Option<usize>>,
    foliage: Vec<bfe_content_native::foliage::Class>,
    foliage_images: Vec<usize>,
    canopy_leaf: Option<usize>,
    lamps:crate::lamps::Lamps,
    placed: bfe_content_native::placed::Placements,
    water: crate::water_host::Images,
    features: [u32; L::FEATURES_LENGTH],
    prepared_views: Option<crate::online::first_person::Prepared>,
    prepared_particles: Option<crate::online::particles::Prepared>,
    prepared_hud: Option<crate::online::hud::Prepared>,
    prepared_sound: Option<crate::online::sound_data::Prepared>,
    used: Vec<bool>,
    live: bool,
    shadow_size: usize,
}
// SAFETY: Rc aliases exist only inside this sealed asset graph. Loader and
// preparation temporaries are dropped before construction returns. There are
// no GL objects, borrowed data, thread-local caches or callbacks in this type.
// Moving its sole owner between threads cannot race a reference count; once
// consumed by open_prepared, its ordinary Rc-bearing outputs are !Send again.
unsafe impl Send for Prepared {}
impl Prepared{
    pub fn shadows(&mut self,choice:&str){let(count,size)=crate::shadows::quality(choice);self.features[L::FEATURES_SHADOWS]=count as u32;self.features[L::FEATURES_CSM]=(count>0) as u32;self.shadow_size=size;}
}

// Immediate native GL calls can wait for the GPU. That wait is not CPU pressure.
pub(crate) use bfe_render::profile::CpuSample;

pub struct Scene {
    pub spawn_objectives: [Vec<String>;2],
    pub spawn_vehicles: std::collections::HashMap<String,[Vec<String>;2]>,
    pub menu_grid: Option<u32>,
    pub menu_models: Vec<crate::menu_models::Model>,
    pub spawn_shots: Vec<bfe_battle_ui::spawn::CameraShot>,
    pub bridge: Box<Bridge>,
    pub resources: Resources,
    far_source: u32,
    far_sample: u32,
    far_target: u32,
    // ParticleDepth's target and its depth texture in the renderer's table:
    // the opaque scene's depth, copied once a frame before anything blends,
    // for soft particles and arcs to fade against.
    depth_target: u32,
    depth_source: u32,
    depth_color_source: u32,
    depth_sample: u32,
    depth_color: u32,
    render_scale: f64,
    adaptive_scale: f64,
    resolution_target: f64,
    resolution_limit:f64,
    resolution_configured:bool,
    resolution_last_frame:Option<std::time::Instant>,
    resolution: bfe_sim::client::timing::Timing,
    resolution_fence: Option<(glow::Fence,std::time::Instant)>,
    // Timestamps at the frame's start, the scene's end and the frame's end; result pending; scene mark written.
    resolution_queries:Option<([glow::Query;3],bool,bool)>,
    resolution_query_active:bool,
    resolution_gpu_ms:f64,
    resolution_scene_ms:f64,
    resolution_cpu_ms:f64,
    resolution_cpu_sample:Option<CpuSample>,
    /// Whether the context's timestamp queries work, once asked.
    resolution_timestamps:Option<bool>,
    render_size: [i32; 2],
    reduced_far_terrain: bool,
    shadows_enabled: bool,
    shadow_bones: usize,
    shadow_size: usize,
    features: [u32; L::FEATURES_LENGTH],
    bump_mapping: bool,
    terrain_detail: bool,
    post_images: Vec<(u32, u32, u32, usize, f64)>,
    post_mode: usize,
    temporal_motion: Option<bfe_render::native_motion::NativeMotion>,
    blur_downsize: f64,
    profile_start: Option<std::time::Instant>,
    /// This frame's last passes draw into the window itself, not the canvas (see `set_direct`).
    pub direct: bool,
    profile_rss_after: bool,
    bloom: bool,
    soft_focus: bool,
    vehicle_blur: bool,
    vehicle_speed: f64,
    depth_texture: u32,
    far_reach: f64,
    distance_scale: f64,
    near_range: [f64; 2],
    /// The near scene's fade at the 2004 game's shortest view distance (NearSceneRange's first and third values).
    near_short: [f64; 2],
    shadows: Option<crate::shadows::Shadows>,
    canopies: Option<crate::canopies::Canopies>,
    tree_sun: [f64;3],
    lamps:crate::lamps::Lamps,
    lamp_worlds:Vec<u32>,
    image_step: [f64; bfe_render::post::STEP],
    image_state: [f64; bfe_render::post::STATE],
    pub canvas: Canvas,
    pub admitted: usize,
    pub deferred: usize,
    pub draws: u64,
    pub size: [i32; 2],
    static_content: crate::static_scene::Admission,
    far_projection: [f64; 16],
    far_world_slots: Vec<usize>,
    near_world_ids: Vec<u32>,
    // A battle's own frame; an inspected map draws straight on the canvas.
    frame: Option<SceneFrame>,
    // Graphics.textures: the content's images, which follow the anisotropy
    // setting, and the value they were last given.
    filtered: Vec<(u32,bool)>,
    anisotropy: f64,
    water: Option<crate::water_host::WaterHost>,
    water_data: usize,
    water_time: f64,
    // The soldiers on foot, gathered each frame for the water's waders.
    water_soldiers: Vec<(u32, [f64; 3], [f64; 3])>,
    storm: Option<crate::storm_host::StormHost>,
    flyers: Option<crate::sky_host::SkyHost>,
    pub online: Option<Box<crate::online::replicas::Replicas>>,
}
// Graphics.sceneTarget: the near scene is drawn over the far scene's image
// into a target of its own, which the antialiasing setting multisamples,
// and the post chain puts the resolved image on the canvas. The HUD and
// the pages are drawn on the canvas after it, at one sample.
struct SceneFrame {
    source: u32,
    sample: u32,
    target: u32,
    texture: u32,
    // The samples of the near scene's target and of the far scene's.
    samples: i32,
    far_samples: i32,
    // The post chain's antialiasing pass (post_chain aa).
    aa: usize,
    // Where aaSamples and farAaSamples sit in the frame's values.
    slots: [usize; 2],
}
// The canvas dimensions are already device pixels, as in getDrawingBufferSize.
fn render_extent(canvas:[i32;2],base:f64,adaptive:f64)->[i32;2]{
    let scale=effective_scale(canvas,base,adaptive);
    canvas.map(|v|(v as f64*scale).round().max(1.)as i32)
}
// Dynamic resolution never draws the scene below 720 lines or half the window, whichever is
// larger (a smaller picture stops looking like the game), unless the player chose a lower scale.
fn effective_scale(canvas:[i32;2],base:f64,adaptive:f64)->f64{
    let floor=(720./canvas[1].max(1) as f64).clamp(0.5,1.);
    (base*adaptive).max(base.min(floor))
}
// Whether drawing the scene smaller is worth it for a slow frame: the floor would bring it under
// the controller's slow line (85% of the budget), or take at least a quarter off it. The GPU time
// that is not the scene's (the stretch or post chain to the window, the HUD, the interface) stays
// whatever the scene's size. Unknown split: yes.
fn resolution_can_help(total_ms:f64,scene_ms:f64,scale:f64,floor:f64,budget_ms:f64)->bool{
    if !(scene_ms.is_finite()&&total_ms.is_finite()&&scale>0.)||budget_ms<=0.{return true;}
    if total_ms<=budget_ms*0.85{return true;}
    let ratio=(floor/scale).min(1.);
    let saving=scene_ms.clamp(0.,total_ms)*(1.-ratio*ratio);
    total_ms-saving<=budget_ms*0.85||saving>=total_ms*0.25
}
fn post_image(size:[i32;2],slot:usize)->String{
    use bfe_render::post_chain::layout::resource as R;
    if matches!(slot,R::HISTORY0|R::HISTORY1|R::MOTION|R::VELOCITY){serde_json::json!({"width":size[0],"height":size[1],"depth":1,"target":glow::TEXTURE_2D,"format":1023,"dataType":1016,"srgb":false,"renderTarget":true}).to_string()}else{frame_image(size)}
}
// A frame-sized colour image a target draws into.
// The far scene's image (sky, horizon and far terrain) at the reduced far terrain setting is drawn at half
// the scene's size and stretched when read: it is distant, fogged scenery behind the near scene, and at
// 1920x1080 on an Intel HD 2000 the full-size far pass cost 7.4 ms of a 20 ms frame. The near scene reads it
// by its own pixel position over `resolution`, so any size of the far image lines up.
// The reduced far image never drops below this many lines: smaller, the mountains and forests the far scene
// draws on open maps (Kashyyyk) blur into one smear.
const FAR_MIN_LINES: f64 = 540.;
/// The far image's size: the scene's, or with reduced far terrain down to half of it, but no fewer than
/// FAR_MIN_LINES lines (so a 960x540 scene keeps a full-size far image and a 1920x1080 one gets 960x540).
fn far_extent(size: [i32; 2], reduced: bool) -> [i32; 2] {
    if !reduced { return size; }
    let scale = (FAR_MIN_LINES / size[1].max(1) as f64).clamp(0.5, 1.);
    size.map(|v| ((v as f64 * scale).round() as i32).max(1))
}
fn frame_image(size: [i32; 2]) -> String {
    serde_json::json!({"width":size[0],"height":size[1],"depth":1,"target":glow::TEXTURE_2D,"format":1023,"dataType":1009,"srgb":false,"renderTarget":true}).to_string()
}
// A target over `color` with a depth buffer of its own. `stencil` gives the
// depth buffer the canvas's format, which a depth copy between them needs.
fn frame_target(size: [i32; 2], color: u32, stencil: bool, samples: i32) -> String {
    serde_json::json!({"width":size[0],"height":size[1],"color":color,"depth":null,"depthBuffer":true,"stencil":stencil,"samples":samples,"resolveDepth":false,"resolveStencil":false,"cube":false}).to_string()
}
// How a content image is sampled (loadTexture: repeating, trilinear)
// at an anisotropy. The driver's limit is applied where the value is set.
fn content_sampling(anisotropy: f64) -> String {
    format!(
        r#"{{"wrapS":1000,"wrapT":1000,"wrapR":1000,"min":1008,"mag":1006,"anisotropy":{anisotropy},"flip":false,"premultiply":false,"alignment":1,"mipmaps":true,"compare":null}}"#
    )
}
impl Scene {
    pub fn load(
        root: &Path,
        map: &str,
        size: [i32; 2],
        load: impl FnMut() -> glow::Context,
    ) -> Result<(Self, MapCamera), String> {
        Self::load_with_random(root, map, size, None, load)
    }
    /// Recorded random draws are admission facts, shared with a client view.
    /// Ordinary inspection uses one deterministic host stream instead.
    pub fn load_with_random(
        root: &Path,
        map: &str,
        size: [i32; 2],
        random_values: Option<&[f64]>,
        load: impl FnMut() -> glow::Context,
    ) -> Result<(Self, MapCamera), String> {
        if random_values.is_some_and(|v| {
            v.len() > 4_194_304 || v.iter().any(|x| !x.is_finite() || !(0.0..1.0).contains(x))
        }) {
            return Err("invalid foliage random admission stream".into());
        }
        let mut loader = Loader::new(root);
        loader.compressed = false;
        let content = loader.load(map, &format!("map={map}&seed=1"))?;
        Self::admit(content,map,size,random_values,false,load)
    }
    /// Online joins admit the advertised mission from local files. The server
    /// cannot supply asset paths or executable class definitions at runtime.
    pub fn load_online(
        root: &Path,
        query: &str,
        size: [i32; 2],
        mut load: impl FnMut() -> glow::Context,
    ) -> Result<(Self, MapCamera), String> {
        let prepared = Self::prepare_online(root, query,TextureOptions::discover(&load(),true))?;
        crate::loading_work::finish(Self::open_prepared(prepared, size, load))
    }
    pub(crate) fn prepare_online(root: &Path, query: &str, textures:TextureOptions) -> Result<Prepared, String> {
        let selection = bfe_content::mission::Query::parse(query);
        let map = if selection.get("map").is_empty() {
            "nab2"
        } else {
            selection.get("map")
        };
        let mut loader = Loader::new(root);
        loader.compressed = textures.compressed;
        loader.max_texture_size=textures.max_size;
        loader.texture_capabilities=textures.capabilities;
        Self::prepare_scene(loader.load(map, query)?, map, None, true)
    }
    fn admit(
        content: bfe_content_native::BattleContent,
        map: &str,
        size: [i32; 2],
        random_values: Option<&[f64]>,
        live: bool,
        mut load: impl FnMut() -> glow::Context,
    ) -> Result<(Self, MapCamera), String> {
        let prepared = Self::prepare_scene(content, map, random_values, live)?;
        crate::loading_work::finish(Self::open_prepared(prepared, size, load))
    }
    fn prepare_scene(
        mut content: bfe_content_native::BattleContent,
        map: &str,
        random_values: Option<&[f64]>,
        live: bool,
    ) -> Result<Prepared, String> {
        let spawn_screen = content.content.battle.world.get("spawnScreen");
        let strings = |value: Option<&bfe_content::Value>| {
            value
                .and_then(bfe_content::Value::as_array)
                .map_or_else(Vec::new, |rows| {
                    rows.iter()
                        .filter_map(bfe_content::Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
        };
        let spawn_objectives = std::array::from_fn(|i| {
            strings(
                spawn_screen
                    .and_then(|s| s.get("teams"))
                    .and_then(|t| t.get(&(i + 1).to_string()))
                    .and_then(|t| t.get("objectives")),
            )
        });
        let spawn_vehicles = spawn_screen
            .and_then(|s| s.get("postVehicles"))
            .and_then(bfe_content::Value::as_object)
            .map_or_else(std::collections::HashMap::new, |posts| {
                posts
                    .entries()
                    .map(|(name, teams)| {
                        (
                            name.to_string(),
                            std::array::from_fn(|i| strings(teams.get(&(i + 1).to_string()))),
                        )
                    })
                    .collect()
            });
        let menu_grid = if live {
            content
                .texture_paths
                .get("shell_cube")
                .map(|p| content.textures.load(p))
                .transpose()?
                .map(|id| id as u32)
        } else {
            None
        };
        let menu_models = if live {
            crate::menu_models::prepare(&mut content)?
        } else {
            Vec::new()
        };
        let spawn_shots = content
            .content
            .battle
            .world
            .get("spawnScreen")
            .and_then(|s| s.get("cameraShots"))
            .and_then(bfe_content::Value::as_array)
            .map_or_else(Vec::new, |shots| {
                shots
                    .iter()
                    .filter_map(|s| {
                        let p = s.get("position")?.as_array()?;
                        let q = s.get("quaternion")?.as_array()?;
                        if p.len() != 3 || q.len() != 4 {
                            return None;
                        }
                        Some(bfe_battle_ui::spawn::CameraShot {
                            position: [p[0].as_f64()?, p[1].as_f64()?, p[2].as_f64()?],
                            quaternion: [
                                q[0].as_f64()?,
                                q[1].as_f64()?,
                                q[2].as_f64()?,
                                q[3].as_f64()?,
                            ],
                        })
                    })
                    .collect()
            });
        // Resolve terrain sampler identities through the authored map table,
        // before GPU admission freezes the texture bank and frame storage.
        let mut layers = std::collections::BTreeMap::new();
        if let Some(table) = content
            .content
            .battle
            .world
            .get("textures")
            .and_then(|v| v.as_object())
        {
            for (name, path) in table.entries() {
                if let Some(path) = path.as_str() {
                    layers.insert(name.to_lowercase(), format!("maps/{map}/{path}"));
                }
            }
        }
        let canopy_leaf=content.map["treeCanopies"]["texture"].as_str().map(|name|{
            let path=layers.get(&name.to_lowercase()).cloned().unwrap_or_else(||format!("maps/{map}/{name}"));content.textures.load(&path)
        }).transpose()?;
        let mut details = Vec::with_capacity(content.world.len());
        let mut layer_ids = Vec::with_capacity(content.world.len());
        for mesh in &content.world {
            let mut ids = Vec::new();
            if let Some(names) = mesh.material.record["layers"].as_array() {
                for name in names {
                    let name = name.as_str().ok_or("invalid terrain sampler name")?;
                    let path = layers
                        .get(&name.to_lowercase())
                        .ok_or("missing terrain sampler")?;
                    ids.push(content.textures.load(path)?);
                }
            }
            layer_ids.push(ids);
            details.push(
                if let Some(name) = mesh.material.record["detail"].as_str() {
                    let path = layers
                        .get(&name.to_lowercase())
                        .ok_or("missing terrain detail sampler")?;
                    Some(content.textures.load(path)?)
                } else {
                    None
                },
            );
        }
        let mut sun = Vector3::new(0., -1., 0.);
        if let Ok(v) =
            serde_json::from_value::<[f64; 3]>(content.map["lighting"]["sunDirection"].clone())
        {
            sun.set(v[0], v[1], v[2]).normalize();
        }
        let order = content
            .content
            .battle
            .world
            .get("foliage")
            .and_then(|v| v.get("classes"))
            .and_then(|v| v.as_object())
            .map(|o| o.entries().map(|(n, _)| n.to_owned()).collect::<Vec<_>>())
            .unwrap_or_default();
        let mut seeded = bfe_content_native::foliage::Seeded::new(1);
        let mut random_at = 0;
        let mut random = || {
            if let Some(values) = random_values {
                let value = values.get(random_at).copied().unwrap_or(f64::NAN);
                random_at += 1;
                value
            } else {
                seeded.next()
            }
        };
        let foliage =
            bfe_content_native::foliage::admit(&content.map["foliage"], &order, sun, &mut random)?;
        let mut foliage_images = Vec::new();
        for class in &foliage {
            let path = layers
                .get(&class.texture.to_lowercase())
                .ok_or_else(|| format!("missing foliage texture {}", class.texture))?;
            foliage_images.push(content.textures.load(path)?);
        }
        let placed = if live{bfe_content_native::placed::admit_live(&content)?}else{bfe_content_native::placed::admit(&content)?};
        let water = crate::water_host::images(&content.map["water"], |name| {
            layers
                .get(&name.to_lowercase())
                .map(|path| content.textures.load(path))
                .transpose()
        })?;
        // constructs eight ordnance light shells and each visible post hologram.
        // Preserve that cold catalog identity; this neutral inspection frame
        // publishes zero active lights until a native World supplies them.
        const ORDNANCE_LIGHTS: usize = 8;
        let world = &content.content.battle.world;
        let posts = if world.get("hologram").is_some_and(|v| v.truthy()) {
            world
                .get("commandPosts")
                .and_then(|v| v.as_array())
                .map_or(0, |posts| {
                    posts
                        .iter()
                        .filter(|post| post.get("holoOdf").is_none_or(|v| v.truthy()))
                        .count()
                })
        } else {
            0
        };
        let mut features = [0; L::FEATURES_LENGTH];
        features[L::FEATURES_SHADOWS] = if live { 4 } else { 0 };
        features[L::FEATURES_CSM] = live as u32;
        features[L::FEATURES_POINTS] = (ORDNANCE_LIGHTS + posts) as u32;
        let prepared_views = if live {
            Some(crate::online::first_person::Prepared::new(&mut content)?)
        } else {
            None
        };
        let prepared_particles = if live {
            Some(crate::online::particles::Prepared::new(&mut content)?)
        } else {
            None
        };
        let prepared_hud = if live {
            Some(crate::online::hud::Prepared::new(&mut content)?)
        } else {
            None
        };
        let prepared_sound = if live {
            Some(crate::online::sound_data::Prepared::new(&content)?)
        } else {
            None
        };
        let mut used = vec![false; content.textures.images.len()];
        for (i, mesh) in content
            .world
            .iter()
            .enumerate()
            .filter(|(_, m)| inspectable(m) || m.material.kind == MaterialKind::Sky)
        {
            for id in mesh
                .material
                .texture
                .into_iter()
                .chain(mesh.material.bump)
                .chain(details[i])
                .chain(layer_ids[i].iter().copied())
            {
                *used.get_mut(id).ok_or("invalid map image id")? = true;
            }
        }
        for id in foliage_images
            .iter()
            .copied()
            .chain(water.all())
        {
            used[id] = true;
        }
        for part in &placed.parts {
            let mesh = content
                .models
                .iter()
                .find(|m| m.name == format!("{}/{}", part.model, part.source))
                .ok_or("missing placed model resource")?;
            if let Some(id) = mesh.material.texture {
                used[id] = true;
            }
        }
        for mesh in content.models.iter().filter(|m| m.name.starts_with("sky/")) {
            if let Some(id) = mesh.material.texture { used[id] = true; }
        }
        if live {
            used.fill(true);
        }
        let lamps=crate::lamps::Lamps::prepare(&content)?;
        Ok(Prepared {
            spawn_objectives,
            spawn_vehicles,
            menu_grid,
            menu_models,
            spawn_shots,
            content,
            layer_ids,
            details,
            foliage,
            foliage_images, canopy_leaf, lamps,
            placed,
            water,
            features,
            prepared_views,
            prepared_particles,
            prepared_hud,
            prepared_sound,
            used,
            live, shadow_size:2048,
        })
    }
    pub(crate) async fn open_prepared(
        prepared: Prepared,
        size: [i32; 2],
        mut load: impl FnMut() -> glow::Context,
    ) -> Result<(Self, MapCamera), String> {
        let Prepared {
            spawn_objectives,
            spawn_vehicles,
            menu_grid,
            mut menu_models,
            spawn_shots,
            mut content,
            layer_ids,
            details,
            foliage,
            foliage_images, canopy_leaf, lamps,
            placed,
            water,
            features,
            prepared_views,
            prepared_particles,
            prepared_hud,
            prepared_sound,
            used,
            live, shadow_size,
        } = prepared;
        crate::loading_work::frame().await;
        let canvas = Canvas::new(load(), size)?;
        let mut resources = Resources::new(load());
        resources.canvas = Some(canvas.framebuffer);
        let mut bridge = Bridge::new_boxed(load(), Ranges::new(load()));
        bridge.r.core.canvas_framebuffer = Some(canvas.framebuffer);
        let shadow_count=features[L::FEATURES_SHADOWS] as usize;
        bridge.r.cameras.input[bfe_render::cameras::layout::input::CASCADE_COUNT]=shadow_count as f64;
        let sampling = &content_sampling(1.);
        let mut textures = vec![u32::MAX; content.textures.images.len()];
        let mut filtered = Vec::new();
        let mut budget = crate::loading_work::Budget::new();
        for (i, image) in content.textures.images.iter().enumerate() {
            // Battle admission has already decoded side resources. Inspection
            // uploads only the images used by the admitted world descriptors.
            if !used[i] {
                continue;
            }
            let compressed=image.format!=bfe_render::texture_format::Format::Rgba||image.mips.len()>1;
            // A grey image (the HUD's font pages) keeps grey and alpha only: the same texels in half the memory.
            let grey=(!compressed).then(||grey_alpha(&image.data)).flatten();
            let format=if grey.is_some(){1030}else{image.format.descriptor()};
            let source=resources.source(&serde_json::json!({"width":image.width,"height":image.height,"depth":1,"target":glow::TEXTURE_2D,"format":format,"dataType":1009,"srgb":false,"greyAlpha":grey.is_some()}).to_string())?;
            if compressed{
                let mips=image.mips.iter().map(|m|serde_json::json!({"width":m.width,"height":m.height,"offset":m.offset,"length":m.length})).collect::<Vec<_>>();
                resources.source_blocks(source,&image.data,&serde_json::to_string(&mips).map_err(|e|e.to_string())?)?;
            }else{resources.source_pixels(source,grey.as_deref().unwrap_or(&image.data))?;}
            let sampling=if compressed{sampling.replace("\"mipmaps\":true","\"mipmaps\":false")}else{sampling.clone()};
            let sample = resources.sample(source, &sampling)?;
            resources.upload(sample)?;
            // The texture is the only copy kept: anisotropy changes it in
            // place and compression applies at the next map load.
            resources.release_pixels(source)?;
            filtered.push((sample,compressed));
            textures[i] = bridge.r.core.add_texture(resources.texture(sample)?);
            if budget.tick().await {
                bridge.r.core.state.reset();
            }
        }
        let menu_grid=menu_grid.map(|id|textures[id as usize]);
        for model in &mut menu_models {model.texture=textures[model.texture as usize];}
        let empty = resources.world_fallback(false)?;
        let empty = bridge.r.core.add_texture(resources.texture(empty)?);
        let neutral = resources.world_fallback(true)?;
        let neutral = bridge.r.core.add_texture(resources.texture(neutral)?);
        // buildWorld attaches canopy footprints before native source admission.
        // Zero state rows retain the authored billboard LOD in this inspector;
        // the existing tree descriptor still owns grouping and mask geometry.
        let canopy = &content.map["treeCanopies"];
        let mut canopy_state=None;
        let canopy_texture =
            if let Some(instances) = canopy["instances"].as_array().filter(|v| !v.is_empty()) {
                if instances.len() > 65536 {
                    return Err("native canopy state capacity".into());
                }
                let state = resources
                    .canopy_state(instances.len() as u32, &vec![0; instances.len() * 16])?;
                canopy_state=Some(state);
                Some(bridge.r.core.add_texture(resources.texture(state)?))
            } else {
                None
            };
        let water_data = &content.map["water"];
        let sea_water = bfe_environment::content::recipe(bfe_environment::layout::environment_recipe::WATER as u32, water_data)["ocean"].as_bool().unwrap_or(false);
        let mut ocean_mask = empty;
        if sea_water {
            let data = &content.map["water"];
            let grid = data["grid"]
                .as_u64()
                .filter(|n| (1..=4096).contains(n))
                .ok_or("invalid ocean mask grid")? as usize;
            let mut pixels = vec![0u8; grid * grid];
            for cell in data["cells"].as_array().ok_or("missing ocean mask cells")? {
                let x = cell[0]
                    .as_u64()
                    .filter(|v| *v < grid as u64)
                    .ok_or("invalid ocean mask column")? as usize;
                let z = cell[1]
                    .as_u64()
                    .filter(|v| *v < grid as u64)
                    .ok_or("invalid ocean mask row")? as usize;
                pixels[z * grid + x] = 255;
            }
            let source=resources.source(&serde_json::json!({"width":grid,"height":grid,"depth":1,"target":glow::TEXTURE_2D,"format":1028,"dataType":1009,"srgb":false}).to_string())?;
            resources.source_pixels(source, &pixels)?;
            let sample=resources.sample(source,r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1003,"mag":1003,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}"#)?;
            resources.upload(sample)?;
            ocean_mask = bridge.r.core.add_texture(resources.texture(sample)?);
        }
        let mut static_content =
            crate::static_scene::props(&mut bridge, &content, &textures, placed, shadow_count, true)?;
        crate::loading_work::frame().await;
        bridge.r.core.state.reset();
        for (class, image) in foliage.iter().zip(foliage_images) {
            bridge.foliage_class(
                textures[image],
                class.grass,
                live,
                &class.definition.to_string(),
                &class.cards,
                &class.rows,
                &class.bounds,
                &[1., 0., 0., 0., 1., 0., 0., 0., 1.],
            )?;
            static_content.foliage += 1;
        }
        let mut far_slot = bfe_render::far::FAR_SLOTS;
        let mut far_world_slots = Vec::new();
        let mut near_world_ids = Vec::new();
        let mut admitted = 0;
        let mut deferred = 0;
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for (mesh_index, (mesh, layer_ids)) in content.world.iter().zip(&layer_ids).enumerate() {
            if budget.tick().await {
                bridge.r.core.state.reset();
            }
            if mesh.material.kind == MaterialKind::Sky {
                let sky = crate::static_scene::sky(&mut bridge, mesh, &textures, far_slot, empty)?;
                far_slot = crate::static_scene::sky_end(&sky);
                static_content.skies.push(sky);
                admitted += 1;
                continue;
            }
            if !inspectable(mesh) {
                deferred += 1;
                continue;
            }
            let raw = &mesh.geometry.buffers[0];
            let vertices = raw
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                .collect::<Vec<_>>();
            let mut authored = mesh.material.record.clone();
            let name = mesh.name.to_ascii_lowercase();
            if canopy_texture.is_some() && (name.contains("treebillboard") || name.contains("bush"))
            {
                authored["treeCanopies"] = canopy.clone();
            }
            let record: bfe_render::static_world::Record =
                serde_json::from_value(authored.clone()).map_err(|e| e.to_string())?;
            for row in vertices
                .chunks_exact(record.stride)
                .filter(|_| !mesh.material.far)
            {
                for k in 0..3 {
                    min[k] = min[k].min(row[k] as f64);
                    max[k] = max[k].max(row[k] as f64);
                }
            }
            let source = bridge.static_source(
                &authored.to_string(),
                vertices,
                mesh.geometry.indices.clone(),
            )?;
            if live && !mesh.material.far && mesh.material.blend == Blend::Opaque
                && mesh.material.kind != MaterialKind::Terrain {
                bridge.static_depth_add(source, mesh.material.texture.map_or(-1, |id| textures[id] as i32))?;
            }
            if let Some(weights) = mesh.geometry.buffers.get(1) {
                bridge.static_source_weights(source, weights)?;
            }
            let mut samplers = [-1; L::SAMPLERS_LENGTH];
            samplers[L::SAMPLERS_EMPTY] = empty as i32;
            samplers[L::SAMPLERS_LAYER1] = neutral as i32;
            samplers[L::SAMPLERS_DETAIL] = neutral as i32;
            if let Some(id) = canopy_texture {
                samplers[L::SAMPLERS_TREE] = id as i32;
            }
            if let Some(id) = mesh.material.texture {
                samplers[L::SAMPLERS_MAP] = textures[id] as i32;
            }
            if let Some(id) = mesh.material.bump {
                samplers[L::SAMPLERS_BUMP] = textures[id] as i32;
            }
            if mesh.material.kind == MaterialKind::Terrain {
                samplers[L::SAMPLERS_BUMP] = -1;
                if let Some(id) = layer_ids.first() {
                    samplers[L::SAMPLERS_MAP] = textures[*id] as i32;
                    samplers[L::SAMPLERS_LAYER1] = textures[*layer_ids.get(1).unwrap_or(id)] as i32;
                }
                if let Some(id) = details[mesh_index] {
                    samplers[L::SAMPLERS_DETAIL] = textures[id] as i32;
                }
            }
            for (i, id) in layer_ids.iter().take(8).enumerate() {
                samplers[L::SAMPLERS_LAYERS + i] = textures[*id] as i32;
            }
            let world_id = bridge.static_world(
                source,
                mesh.material.far,
                if mesh.material.far {
                    mesh.material.record["lowResScale"].as_f64().unwrap_or(1.)
                } else {
                    1.
                },
                &samplers,
                if mesh.material.far {
                    &[0; L::FEATURES_LENGTH]
                } else {
                    &features
                },
                admitted as u32,
                far_slot as u32,
                admitted as u32,
            )?;
            if !mesh.material.far && mesh.material.blend == Blend::Opaque {
                near_world_ids.push(world_id);
            }
            if mesh.material.far {
                far_world_slots.push(far_slot);
                bridge.r.far.input[far_slot] = 1.;
                far_slot += 2;
            }
            admitted += 1;
        }
        crate::loading_work::frame().await;
        bridge.r.core.state.reset();
        // Graphics.farTarget and NearPass.writeImage retain separate images:
        // the shared schedule clears near depth/color before the far image's
        // depth-tested composite. A canvas alias cannot preserve that order.
        let far_source=resources.source(&serde_json::json!({"width":size[0],"height":size[1],"depth":1,"target":glow::TEXTURE_2D,"format":1023,"dataType":1009,"srgb":false,"renderTarget":true}).to_string())?;
        let far_sample=resources.sample(far_source,r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1006,"mag":1006,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}"#)?;
        let far_target=resources.target(&serde_json::json!({"width":size[0],"height":size[1],"color":far_sample,"depth":null,"depthBuffer":true,"stencil":false,"samples":0,"resolveDepth":false,"resolveStencil":false,"cube":false}).to_string())?;
        // The canvas keeps its depth in a 24-bit depth and stencil buffer; a
        // depth copy needs the same format on the other side. The target's
        // colour is one byte a pixel and never drawn.
        let depth_source=resources.source(&serde_json::json!({"width":size[0],"height":size[1],"depth":1,"target":glow::TEXTURE_2D,"format":1027,"dataType":1020,"srgb":false,"renderTarget":true}).to_string())?;
        let depth_sample=resources.sample(depth_source,r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1003,"mag":1003,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}"#)?;
        let depth_color=resources.source(&serde_json::json!({"width":size[0],"height":size[1],"depth":1,"target":glow::TEXTURE_2D,"format":1028,"dataType":1009,"srgb":false,"renderTarget":true}).to_string())?;
        let depth_color_source = depth_color;
        let depth_color=resources.sample(depth_color,r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1003,"mag":1003,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}"#)?;
        let depth_target=resources.target(&serde_json::json!({"width":size[0],"height":size[1],"color":depth_color,"depth":depth_sample,"depthBuffer":true,"stencil":true,"samples":0,"resolveDepth":false,"resolveStencil":false,"cube":false}).to_string())?;
        let depth_texture = bridge.r.core.add_texture(resources.texture(depth_sample)?);
        let far_texture = bridge.r.core.add_texture(resources.texture(far_sample)?);
        bridge.set_far_map(far_texture);
        let image_program = bridge.add_owned_program("effects/far-image")?;
        use bfe_render::post;
        let mut image_step = [0.; post::STEP];
        image_step[post::STEP_PROGRAM] = image_program as f64;
        image_step[post::STEP_TARGET] = post::TARGET_BOUND;
        image_step[post::STEP_WIDTH] = size[0] as f64;
        image_step[post::STEP_HEIGHT] = size[1] as f64;
        image_step[post::STEP_UNIFORMS] = 1.;
        image_step[post::STEP_FIRST + 1] = post::TYPE_SAMPLER as f64;
        image_step[post::STEP_FIRST + 2] = far_texture as f64;
        let mut image_state = [0.; post::STATE];
        image_state[post::STATE_FLAGS] = post::STATE_DEPTH_TEST as f64;
        image_state[post::STATE_DEPTH_FUNC] = glow::LEQUAL as f64;
        // Inspection has not admitted post-processing programs. Missing ids
        // must stay outside the live program bank, never alias world shader 0.
        use bfe_render::post_chain::layout::{facts as pf, program as pp};
        bridge.r.post_chain.input[pf::PROGRAMS..pf::PROGRAMS + pp::COUNT].fill(u32::MAX as f64);
        let post_images = Vec::new();
        let frame = if live {
            let source = resources.source(&frame_image(size))?;
            let sample = resources.sample(source, r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1006,"mag":1006,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}"#)?;
            let target = resources.target(&frame_target(size, sample, true, 0))?;
            let texture = bridge.r.core.add_texture(resources.texture(sample)?);
            let blur = &content.map["post"]["blur"];
            bridge.r.post_chain.input[pf::BLUR_ENABLED] = blur["enabled"].as_bool().unwrap_or(false) as u8 as f64;
            bridge.r.post_chain.input[pf::BLUR_BLEND] = blur["blend"].as_f64().unwrap_or(0.3);
            let slots = ["aaSamples", "farAaSamples"].map(|name| {
                shader::FRAME_OFFSETS[shader::FRAME_MEMBERS.iter().position(|(n, _)| *n == name).expect("frame catalog member")]
            });
            Some(SceneFrame { source, sample, target, texture, samples: 0, far_samples: 0, aa: 0, slots })
        } else {
            None
        };
        // WorldPass.prepareNative seals the opaque draw order after admission.
        // Adding descriptors alone leaves the retained submission list empty.
        bridge.r.world.sort(false);
        let mut water_host = if !water_data.is_null() {
            let far_reach = content.map["distance"]["farScene"][1].as_f64().unwrap_or(3000.);
            // A weather of anything but quads storms (storms()).
            let rain = &content.map["weather"]["precipitation"];
            let stormy = rain.is_object() && !rain["Type"][0].as_str().unwrap_or("").eq_ignore_ascii_case("quads");
            let mut host = crate::water_host::WaterHost::new(
                &mut bridge,
                &mut resources,
                water_data,
                &water,
                &textures,
                &mut far_slot,
                far_reach,
                size,
                live,
                (ocean_mask, stormy),
            )?;
            host.set_settings(&mut bridge, &mut resources, &crate::water_host::Settings::default())?;
            Some(host)
        } else {
            None
        };
        let rain = &content.map["weather"]["precipitation"];
        let storm = if sea_water && rain.is_object() && !rain["Type"][0].as_str().unwrap_or("").eq_ignore_ascii_case("quads") {
            Some(crate::storm_host::StormHost::new(&mut bridge, &mut resources, &content.world, &textures, water_data, &mut far_slot)?)
        } else { None };
        crate::loading_work::frame().await;
        bridge.r.core.state.reset();
        let flyers = crate::sky_host::SkyHost::new(&mut bridge, &mut resources, &content, &textures, &mut far_slot)?;
        bridge.r.far.prepare_frame_capacity();
        crate::loading_work::frame().await;
        bridge.r.core.state.reset();
        bridge.r.foliage.prepare_frame_caches();
        if admitted == 0 {
            return Err("map contains no inspectable static world".into());
        }
        let center = std::array::from_fn::<_, 3, _>(|k| (min[k] + max[k]) * 0.5);
        let radius = ((max[0] - min[0]).max(max[2] - min[2])) * 0.5;
        let mut camera = MapCamera::new(
            [
                center[0],
                center[1] + radius * 0.35,
                center[2] + radius * 0.6,
            ],
            size,
        )?;
        environment(&mut camera.view, &content.map);
        if live {
            // Graphics.applyDistance uses the authored near fade, including
            // the foliage cutoff. The inspector's wide diagnostic lens is
            // not a battle view-distance setting.
            let range = &content.map["distance"]["nearScene"];
            let end = range[3].as_f64().filter(|v| v.is_finite() && *v > 0.3).unwrap_or(500.);
            let start = range[1].as_f64().filter(|v| v.is_finite() && *v >= 0. && *v < end).unwrap_or(300.);
            set(&mut camera.view, "fade", &[start, end]);
            camera.far_plane = end;
            camera.resize(size);
        }
        let mut projection = Matrix4::default();
        let top = 2. * (60.0f64.to_radians() * 0.5).tan();
        let right = top * size[0] as f64 / size[1] as f64;
        let far_reach = content.map["distance"]["farScene"][1]
            .as_f64()
            .unwrap_or(3000.);
        projection.make_perspective(-right, right, top, -top, 2., far_reach, CoordinateSystem::WebGL);
        deferred += static_content.deferred;
        eprintln!(
            "native static admission: props={}, foliage={}, water={}, sky={}",
            static_content.props,
            static_content.foliage,
            usize::from(water_host.is_some()),
            static_content.skies.len()
        );
        if std::env::var_os("BFE_BENCH").is_some() {
            let mut foliage_counts = std::collections::BTreeMap::<&str, usize>::new();
            for instance in content.map["foliage"]["instances"].as_array().into_iter().flatten() {
                *foliage_counts.entry(instance["class"].as_str().unwrap_or("")).or_default() += 1;
            }
            let meshes: Vec<_> = content.world.iter().enumerate().map(|(i, m)| serde_json::json!({
                "segment": i, "name": m.name, "kind": format!("{:?}", m.material.kind),
                "flags": m.material.flags, "cutout": m.material.cutout,
                "blended": m.material.blend != Blend::Opaque, "doubleSided": m.material.double_sided,
                "far": m.material.far, "vertices": m.geometry.vertices, "triangles": m.geometry.indices.len()/3,
                "admitted": inspectable(m) || m.material.kind == MaterialKind::Sky,
            })).collect();
            let props = content.content.battle.props.as_ref().map(bfe_content_native::loader::authored_json);
            eprintln!("bench world census: {}", serde_json::json!({
                "map": content.map["name"], "world": meshes,
                "props": props.as_ref().map(|p| &p["instances"]),
                "propParts": static_content.props, "deferred": deferred,
                "foliageClasses": static_content.foliage,
                "foliage": {"classes": content.map["foliage"]["classes"], "instancesByClass": foliage_counts},
                "waterSurfaces": usize::from(water_host.is_some()), "water": content.map["water"],
                "skyLayers": static_content.skies.len(), "lights": content.map["lighting"],
                "lightRegions": content.map["lightRegions"], "lumi": content.map["lumi"],
                "collision": {"buildingsTriangles": content.content.collision.indices.len()/3,
                    "terrainTriangles": content.content.collision.terrain_indices.len()/3,
                    "propBodies": content.content.prop_bodies.len()},
            }));
        }
        crate::loading_work::frame().await;
        bridge.r.core.state.reset();
        // The HUD's solid art (rings, rules, bars) is a white pixel tinted per quad; the world's empty
        // fallback is transparent black and drew none of them.
        let solid = {
            let gl = &bridge.r.core.gl;
            let texture = unsafe {
                let t = bridge.r.core.hold_texture(gl.create_texture()?);
                gl.bind_texture(glow::TEXTURE_2D, Some(t));
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA8 as i32,
                    1,
                    1,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(&[255; 4])),
                );
                for key in [glow::TEXTURE_MIN_FILTER, glow::TEXTURE_MAG_FILTER] {
                    gl.tex_parameter_i32(glow::TEXTURE_2D, key, glow::NEAREST as i32);
                }
                t
            };
            bridge.r.core.state.reset();
            bridge.r.core.add_texture(texture)
        };
        let online = if live {
            Some(Box::new(
                crate::online::replicas::Replicas::new(
                    &mut content,
                    &mut bridge,
                    &textures,
                    prepared_views.ok_or("missing first-person resources")?,
                    prepared_hud.ok_or("missing battle HUD resources")?,
                    prepared_particles.ok_or("missing battle particle resources")?,
                    prepared_sound.ok_or("missing battle audio resources")?,
                    &mut resources,
                    solid,
                    size,
                )
                .await?,
            ))
        } else {
            None
        };
        // The water's near passes keep their values after everything the
        // effect data holds already.
        let water_fx = online.as_ref().map_or(0, |online| online.effects_end());
        if let Some(host) = &water_host {
            bridge.fx_reserve((water_fx + host.data_floats()) as u32);
        }
        let bones = static_content.records.iter()
            .map(|row| row[bfe_render::instances::REC_BONES] as usize)
            .chain([online.as_ref().map_or(1, |resources| resources.max_bones())])
            .max()
            .unwrap_or(1)
            .max(1);
        let shadows = if live && shadow_count>0 {
            Some(crate::shadows::Shadows::new(&mut bridge, &mut resources, bones,shadow_count,shadow_size).await?)
        } else {
            None
        };
        bridge.r.world.depth_prepass(&mut bridge.r.core, live && shadow_count > 0)?;
        while !bridge.r.frame_reserve_role() {
            crate::loading_work::frame().await;
        }
        crate::loading_work::frame().await;
        Ok((
            Self {
                water: water_host.take(),
                water_data: water_fx,
                water_time: f64::NAN,
                water_soldiers: Vec::new(),
                storm,
                flyers,
                online,
                bridge,
                resources,
                far_source,
                far_sample,
                far_target,
                depth_target,
                depth_source, depth_color_source, depth_sample, depth_color,
                render_scale: 1., render_size: size, adaptive_scale:1., resolution_target:0.,resolution_limit:0.,resolution_configured:false,resolution_last_frame:None, resolution:bfe_sim::client::timing::Timing::new(), resolution_fence:None,resolution_queries:None,resolution_query_active:false,resolution_gpu_ms:f64::NAN,resolution_scene_ms:f64::NAN,resolution_cpu_ms:f64::NAN,resolution_cpu_sample:None,resolution_timestamps:None,
                profile_start: None, profile_rss_after: false, direct: false,
                post_images, post_mode: 0, temporal_motion:None, blur_downsize:content.map["post"]["blur"]["downsize"].as_f64().filter(|v|v.is_finite()&&*v>0.).unwrap_or(0.25), bloom: false, soft_focus: false, vehicle_blur:false,vehicle_speed:0.,
                reduced_far_terrain: false, shadows_enabled: shadow_count>0, shadow_bones:bones, shadow_size, features, bump_mapping: true, terrain_detail: true,
                depth_texture,
                far_reach,
                distance_scale: 1.,
                near_range: [camera.view.values[bfe_render::cameras::layout::shader_frame::FADE], camera.view.values[bfe_render::cameras::layout::shader_frame::FADE + 1]],
                near_short: short_fade(&content.map["distance"]["nearScene"], [camera.view.values[bfe_render::cameras::layout::shader_frame::FADE], camera.view.values[bfe_render::cameras::layout::shader_frame::FADE + 1]]),
                shadows,lamps,lamp_worlds:vec![],
                tree_sun:camera.view.values[bfe_render::cameras::layout::shader_frame::SUN_DIRECTION..][..3].try_into().unwrap(),
                canopies:canopy_leaf.zip(canopy_state).map(|(leaf,state)|crate::canopies::Canopies::new(content.map["treeCanopies"].clone(),textures[leaf],state)),
                image_step,
                image_state,
                canvas,
                spawn_shots,
                menu_models,
                menu_grid,
                spawn_objectives,
                spawn_vehicles,
                admitted,
                deferred,
                draws: 0,
                size,
                static_content,
                far_projection: projection.elements,
                far_world_slots,
                near_world_ids,
                frame,
                filtered,
                anisotropy: 1.,
            },
            camera,
        ))
    }
    pub fn profile_memory(&self, label: &str) {
        if !self.bridge.r.core.profile.enabled {return;}
        let rss=std::fs::read_to_string("/proc/self/status").unwrap_or_default().lines().find(|l|l.starts_with("VmRSS:")).unwrap_or("VmRSS: unavailable").to_owned();
        let internal=self.bridge.r.instances.texture_bytes()+self.bridge.r.motion.texture_bytes()+self.bridge.r.effects.texture_bytes()+self.bridge.r.core.texture_bytes();
        let textures=self.resources.texture_bytes()+internal;
        eprintln!("graphics memory: {label} texture-bytes={} resource-texture-bytes={} internal-texture-bytes={} renderbuffer-bytes={} canvas-bytes={} {}",textures,self.resources.texture_bytes(),internal,self.resources.target_bytes(),self.size[0] as u64*self.size[1] as u64*8,rss);

    }
    /// Draws this frame's last passes (the stretch to the window's size and the HUD)
    /// straight into the window instead of the canvas, which the interface pass would
    /// otherwise copy into the window: one window-sized copy less a frame, plus the
    /// interface's own clear and draw. Only a frame whose scene has its own target
    /// (nothing reads the canvas back); the caller asks only when no interface is shown
    /// over the battle. Returns whether the frame draws direct.
    pub fn set_direct(&mut self, wanted: bool) -> bool {
        self.direct = wanted && self.frame.is_some();
        let target = if self.direct { None } else { Some(self.canvas.framebuffer) };
        self.resources.canvas = target;
        self.bridge.r.core.canvas_framebuffer = target;
        self.direct
    }
    pub fn finish_profile(&mut self) {
        self.resolution_gpu_end();
        if let Some((fence,_))=self.resolution_fence.take(){unsafe{self.bridge.r.core.gl.delete_sync(fence);}}
        self.delete_resolution_queries();
        self.profile_memory("battle end");
        self.bridge.r.core.profile.report(&self.bridge.r.core.gl);
    }

    /// The view distance: returns the near fade and the near projection's far plane.
    pub fn set_view_distance(&mut self, choice: &str) -> ([f64; 2], f64) {
        self.distance_scale = match choice { "far" => 2., "max" => 4., _ => 1. };
        let far = self.far_reach * self.distance_scale;
        self.far_projection[10] = -(far + 2.) / (far - 2.);
        self.far_projection[14] = -4. * far / (far - 2.);
        // Short: the near scene ends where the 2004 game's shortest view distance
        // ended it; past that every object is its far scene image, as there.
        if choice == "short" {
            // The static world and the far scene switch at the short range; units, vehicles and command
            // posts keep the authored range, as play needs them.
            self.bridge.r.actor_fade = Some(self.near_range);
            return (self.near_short, self.near_range[1].max(self.near_short[1]));
        }
        self.bridge.r.actor_fade = None;
        let range = self.near_range.map(|v| v * self.distance_scale);
        (range, range[1])
    }

    pub fn log_graphics(&self) {
        self.log_resolution();
        use bfe_render::post_chain::layout::state;
        eprintln!("battle graphics: scene={:?} canvas={:?} shadows={} farReduced={} bump={} terrainEnhanced={} bloom={} softFocus={} postSteps={}",
            self.render_size, self.size, self.shadows_enabled, self.reduced_far_terrain,
            self.bump_mapping, self.terrain_detail, self.bloom, self.soft_focus,
            self.bridge.r.post_chain.state[state::STEPS]);
        eprintln!("battle graphics distance: factor={} adaptive={} shadow-cascades={} shadow-size={}",self.distance_scale,self.adaptive_scale,self.features[L::FEATURES_SHADOWS],self.shadow_size);
    }

    pub fn set_post_effects(&mut self, bloom: bool, soft_focus: bool, upscaler: &str) -> Result<(),String> {
        use bfe_render::post_chain::layout::mode;
        self.bloom = bloom;
        self.soft_focus = soft_focus;
        self.post_mode = match upscaler { "bilinear" => mode::BILINEAR, "spatial" => mode::SPATIAL, "off"=>mode::OFF, _ => mode::TEMPORAL };
        self.configure_post()
    }

    fn configure_post(&mut self)->Result<(),String>{
        use bfe_render::post_chain::layout::{facts as pf,program as pp,resource as pr,mode,aa};
        let intermediate=self.bloom||self.soft_focus&&self.bridge.r.post_chain.input[pf::BLUR_ENABLED]!=0.||self.post_mode>=mode::SPATIAL||self.vehicle_blur&&self.vehicle_speed>0.0001/0.085;
        let fxaa=self.frame.as_ref().is_some_and(|f|f.aa!=aa::OFF);
        let temporal=self.post_mode==mode::TEMPORAL;
        if temporal&&self.temporal_motion.is_none(){self.temporal_motion=Some(bfe_render::native_motion::NativeMotion::new(&mut self.bridge.r.core)?);self.bridge.r.post_chain.invalidate();}
        if !temporal{self.temporal_motion=None;}
        if intermediate||fxaa{
            for (program,key) in [(pp::COPY,"post-hud/copy"),(pp::EXTRACT,"post-hud/bloom-extract"),(pp::BLUR,"post-hud/bloom-blur"),(pp::BILINEAR,"post-hud/bilinear"),(pp::SPATIAL,"post-hud/spatial"),(pp::COMPOSITE,"post-hud/composite"),(pp::FXAA,"post-hud/fxaa"),(pp::FXAA_FAST,"post-hud/fxaa-fast"),(pp::MOTION,"post-hud/motion"),(pp::RESOLVE,"post-hud/resolve"),(pp::SHARPEN,"post-hud/sharpen")]{
                self.bridge.r.post_chain.input[pf::PROGRAMS+program]=self.bridge.add_owned_program(key)? as f64;
            }
        }
        let mut wanted=Vec::new();
        if intermediate{wanted.extend([(pr::QUARTER,self.blur_downsize),(pr::BLOOM,0.25),(pr::SCRATCH,0.25),(pr::POST,1.),(pr::AA,1.)]);}
        // FXAA before an upscale writes its own target even with no other effect on (the Performance preset).
        else if fxaa{wanted.push((pr::AA,1.));}
        if temporal{wanted.extend([(pr::HISTORY0,-1.),(pr::HISTORY1,-1.),(pr::MOTION,1.),(pr::VELOCITY,1.)]);}
        let mut i=0;
        while i<self.post_images.len(){
            let(_,sample,texture,slot,_)=self.post_images[i];
            if wanted.iter().any(|(s,_)|*s==slot){i+=1;continue;}
            self.bridge.r.forget_texture(texture);self.resources.dispose_sample(sample)?;
            self.bridge.r.post_chain.input[pf::RESOURCES+slot*pr::STRIDE..][..pr::STRIDE].fill(-1.);self.post_images.remove(i);
        }
        for(slot,factor)in wanted{
            if self.post_images.iter().any(|r|r.3==slot){continue;}
            let extent=if factor<0.{self.size}else{self.render_size.map(|v|(v as f64*factor).ceil().max(1.) as i32)};
            let source=self.resources.source(&post_image(extent,slot))?;
            let sample=self.resources.sample(source,r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1006,"mag":1006,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}"#)?;
            self.resources.upload(sample)?;let texture=self.bridge.r.core.add_texture(self.resources.texture(sample)?);let target=self.bridge.post_target(texture);
            self.bridge.r.post_chain.input[pf::RESOURCES+slot*pr::STRIDE..][..pr::STRIDE].copy_from_slice(&[texture as f64,target as f64,extent[0]as f64,extent[1]as f64]);
            self.post_images.push((source,sample,texture,slot,factor));
        }
        self.bridge.r.core.state.reset();Ok(())
    }

    pub fn set_lamp_lighting(&mut self,enabled:bool)->Result<(),String>{
        if !self.lamps.configure(&mut self.bridge.r.core,enabled)?{return Ok(());}
        self.features[L::FEATURES_LAMPS]=enabled as u32;
        let count=self.features[L::FEATURES_SHADOWS];
        self.bridge.r.core.configure_instance_programs(count,enabled)?;self.bridge.actor_settings(count,enabled)?;
        self.bridge.static_world_settings(&self.features,&[0;L::FEATURES_LENGTH])?;
        self.lamp_worlds.clear();for &id in &self.near_world_ids{if self.bridge.world_uses_lamps(id){self.lamp_worlds.push(id);}}
        self.bridge.r.world.sort(false);self.bridge.r.core.state.reset();Ok(())
    }
    pub fn set_vehicle_blur(&mut self,enabled:bool)->Result<(),String>{self.vehicle_blur=enabled;self.configure_post()}
    pub fn vehicle_motion(&mut self,amount:f64,turn:f64,dt:f64)->Result<(),String>{
        use bfe_render::post_chain::layout::facts as F;
        let was=self.vehicle_blur&&self.vehicle_speed>0.0001/0.085;
        self.vehicle_speed+=(amount.clamp(0.,1.)-self.vehicle_speed)*(1.-(-4.*dt.max(0.)).exp());
        let f=&mut self.bridge.r.post_chain.input;f[F::SPEED_ENABLED]=self.vehicle_blur as u8 as f64;f[F::SPEED_AMOUNT]=self.vehicle_speed;f[F::SPEED_TURN]=turn.clamp(-2.,2.);
        if was!=(self.vehicle_blur&&self.vehicle_speed>0.0001/0.085){self.configure_post()?;}
        Ok(())
    }
    pub fn set_tree_detail(&mut self,enhanced:bool)->Result<(),String>{
        if let Some(canopies)=&mut self.canopies{
            canopies.configure(&mut self.bridge,enhanced,self.tree_sun)?;
        }Ok(())
    }
    pub fn set_far_terrain(&mut self, reduced: bool) -> Result<(), String> {
        if self.reduced_far_terrain == reduced { return Ok(()); }
        self.reduced_far_terrain = reduced;
        let far = far_extent(self.render_size, reduced);
        self.bridge.r.core.state.reset();
        self.resources.source_desc(self.far_source, &frame_image(far))?;
        self.resources.resize_target(self.far_target, &frame_target(far, self.far_sample, false, self.frame.as_ref().map_or(0, |f| f.far_samples)))?;
        self.bridge.r.core.state.reset();
        Ok(())
    }

    pub fn set_shadows(&mut self,choice:&str)->Result<(),String>{
        let(count,size)=crate::shadows::quality(choice);
        self.bridge.r.world.depth_prepass(&mut self.bridge.r.core, count > 0)?;
        if self.features[L::FEATURES_SHADOWS] as usize==count&&self.shadow_size==size{return Ok(());}
        if let Some(shadows)=self.shadows.take(){shadows.dispose(&mut self.bridge,&mut self.resources)?;}
        self.features[L::FEATURES_SHADOWS]=count as u32;self.features[L::FEATURES_CSM]=(count>0) as u32;self.shadow_size=size;self.shadows_enabled=count>0;
        self.bridge.r.cameras.input[bfe_render::cameras::layout::input::CASCADE_COUNT]=count as f64;
        self.bridge.r.core.configure_instance_programs(count as u32,self.lamps.enabled)?;
        self.bridge.actor_settings(count as u32,self.lamps.enabled)?;
        self.bridge.effect_settings(count>0)?;
        self.bridge.r.foliage.configure_shadows(&mut self.bridge.r.core,count>0)?;
        self.bridge.static_world_settings(&self.features,&[0;L::FEATURES_LENGTH])?;
        if count>0{self.shadows=Some(crate::loading_work::finish(crate::shadows::Shadows::new(&mut self.bridge,&mut self.resources,self.shadow_bones,count,size))?);}
        self.bridge.r.world.sort(false);self.bridge.r.core.state.reset();Ok(())
    }

    pub fn set_material_detail(&mut self, bump: bool, terrain: bool) {
        self.bump_mapping = bump;
        self.terrain_detail = terrain;
    }

    pub fn set_resolution(&mut self,base:f64,target:f64,limit:f64)->Result<(),String>{
        let base=if base.is_finite(){base.clamp(0.25,2.)}else{1.};
        let target=if target.is_finite(){target.max(0.)}else{0.};
        let limit=if limit.is_finite(){limit.max(0.)}else{0.};
        let target=if limit>0.{target.min(limit)}else{target};
        let changed=!self.resolution_configured||self.render_scale!=base||self.resolution_target!=target||self.resolution_limit!=limit;
        if !changed{return Ok(());}
        self.delete_resolution_queries();
        if let Some((fence,_))=self.resolution_fence.take(){unsafe{self.bridge.r.core.gl.delete_sync(fence);}}
        self.resolution_last_frame=None;self.resolution_cpu_ms=f64::NAN;self.resolution_gpu_ms=f64::NAN;self.resolution_scene_ms=f64::NAN;self.resolution_query_active=false;
        self.resolution_configured=true;self.resolution_target=target;self.resolution_limit=limit;self.render_scale=base;self.adaptive_scale=1.;
        self.resolution.run(15,target,0.,0.,0.,0.);
        let previous=self.render_size;self.resize_scene_targets()?;
        if previous==self.render_size{self.log_resolution();}
        Ok(())
    }
    pub(crate) fn resolution_frame_begin(&mut self){self.resolution_cpu_sample=self.resolution_enabled().then(CpuSample::now);}
    pub(crate) fn resolution_presented(&mut self,paused:bool)->Result<(),String>{
        self.resolution_gpu_end();
        if let Some(sample)=self.resolution_cpu_sample.take(){self.adapt_resolution(sample.elapsed_ms(),paused)?;}
        Ok(())
    }
    fn delete_resolution_queries(&mut self){
        let core=&mut self.bridge.r.core;core.scene_end_mark=None;
        if let Some((q,_,_))=self.resolution_queries.take(){for q in q{unsafe{core.gl.delete_query(q);}}}
    }
    fn resolution_gpu_begin(&mut self){
        if !self.resolution_enabled(){return;}
        let gl=&self.bridge.r.core.gl;
        // Without a working timer the controller reads the GPU from its fences instead (adapt_resolution).
        if self.resolution_queries.is_none()&&*self.resolution_timestamps.get_or_insert_with(||bfe_render::profile::gpu_timestamps(gl)){unsafe{
            let mut made=vec![];
            for _ in 0..3{if let Ok(q)=gl.create_query(){made.push(q);}}
            match <[glow::Query;3]>::try_from(made){Ok(q)=>self.resolution_queries=Some((q,false,false)),Err(made)=>for q in made{gl.delete_query(q);}}
        }}
        if let Some((q,false,_))=self.resolution_queries{
            unsafe{gl.query_counter(q[0],glow::TIMESTAMP);}
            self.bridge.r.core.scene_end_mark=Some(q[1]);
            self.resolution_query_active=true;
        }
    }
    fn resolution_gpu_end(&mut self){
        if !self.resolution_query_active{return;}
        self.resolution_query_active=false;
        let core=&mut self.bridge.r.core;
        let marked=core.scene_end_mark.take().is_none();
        if let Some((q,pending,scene))=&mut self.resolution_queries{unsafe{core.gl.query_counter(q[2],glow::TIMESTAMP);}*pending=true;*scene=marked;}
    }
    pub fn resolution_enabled(&self)->bool{self.resolution_target>0.}
    pub fn adapt_resolution(&mut self,cpu_ms:f64,paused:bool)->Result<(),String>{
        if !self.resolution_enabled(){return Ok(());}
        self.resolution_cpu_ms=cpu_ms;
        let now=std::time::Instant::now();
        let interval_ms=self.resolution_last_frame.replace(now).map_or(f64::NAN,|last|now.duration_since(last).as_secs_f64()*1000.);
        let gl=&self.bridge.r.core.gl;
        if let Some((q,pending,scene))=&mut self.resolution_queries{if *pending&&unsafe{gl.get_query_parameter_u32(q[2],glow::QUERY_RESULT_AVAILABLE)}!=0{
            let [start,mark,end]=q.map(|q|unsafe{gl.get_query_parameter_u64(q,glow::QUERY_RESULT)});
            let disjoint=gl.version().is_embedded&&unsafe{gl.get_parameter_i32(0x8FBB)}!=0;
            let ms=|to:u64|if disjoint||start==0{f64::NAN}else{to.saturating_sub(start)as f64/1e6};
            self.resolution_gpu_ms=ms(end);
            self.resolution_scene_ms=if *scene{ms(mark)}else{f64::NAN};
            *pending=false;
        }}
        let(behind,latency)=if let Some((fence,sent))=self.resolution_fence.take(){
            let state=unsafe{gl.client_wait_sync(fence,0,0)};
            unsafe{gl.delete_sync(fence);}
            if state==glow::WAIT_FAILED{(f64::NAN,f64::NAN)}else{((state==glow::TIMEOUT_EXPIRED) as u8 as f64,sent.elapsed().as_secs_f64()*1000.)}
        }else{(f64::NAN,f64::NAN)};
        self.resolution_fence=unsafe{gl.fence_sync(glow::SYNC_GPU_COMMANDS_COMPLETE,0)}.ok().map(|f|(f,std::time::Instant::now()));
        if paused{self.resolution.run(16,0.,0.,0.,0.,0.);return Ok(());}
        // When even the floor could not bring the frame under the line, the cost is not the
        // scene's size: give the controller no GPU evidence, so it holds instead of shrinking.
        let scale=effective_scale(self.size,self.render_scale,self.adaptive_scale);
        let floor=effective_scale(self.size,self.render_scale,0.5);
        let (gpu,behind,latency)=if resolution_can_help(self.resolution_gpu_ms,self.resolution_scene_ms,scale,floor,1000./self.resolution_target){(self.resolution_gpu_ms,behind,latency)}else{(f64::NAN,f64::NAN,f64::NAN)};
        let scale=self.resolution.run(17,interval_ms,cpu_ms,gpu,behind,latency);
        if scale!=self.adaptive_scale{self.adaptive_scale=scale;self.resize_scene_targets()?;}
        Ok(())
    }
    fn log_resolution(&self){
        let scale=effective_scale(self.size,self.render_scale,self.adaptive_scale);
        eprintln!("battle resolution: canvas-device={}x{} renderScale={} adaptive={} effective={} scene-device={}x{} target-fps={} GPU-ms={} scene-GPU-ms={} CPU-thread-ms={}",self.size[0],self.size[1],self.render_scale,self.adaptive_scale,scale,self.render_size[0],self.render_size[1],self.resolution_target,self.resolution_gpu_ms,self.resolution_scene_ms,self.resolution_cpu_ms);
    }

    /// The player's water choices (WATER QUALITY, OCEAN AND SKY, WATER AND
    /// STEAM), applied as the client applied them: live.
    pub fn set_water_settings(&mut self, settings: &WaterSettings) -> Result<(), String> {
        if let Some(storm) = &mut self.storm { storm.enabled = settings.ocean == "enhanced"; }
        if let Some(flyers) = &mut self.flyers { flyers.set_enhanced(settings.ocean == "enhanced"); }
        match &mut self.water {
            Some(host) => host.set_settings(&mut self.bridge, &mut self.resources, settings),
            None => Ok(()),
        }
    }
    fn resize_scene_targets(&mut self) -> Result<(), String> {
        let size=render_extent(self.size,self.render_scale,self.adaptive_scale);
        if size == self.render_size { return Ok(()); }
        self.bridge.r.core.state.reset();
        let far = far_extent(size, self.reduced_far_terrain);
        self.resources.source_desc(self.far_source, &frame_image(far))?;
        self.resources.resize_target(self.far_target, &frame_target(far, self.far_sample, false, self.frame.as_ref().map_or(0, |f| f.far_samples)))?;
        if let Some(frame) = &self.frame {
            self.resources.source_desc(frame.source, &frame_image(size))?;
            self.resources.resize_target(frame.target, &frame_target(size, frame.sample, true, frame.samples))?;
        }
        for (source, format, data_type) in [(self.depth_source, 1027, 1020), (self.depth_color_source, 1028, 1009)] {
            self.resources.source_desc(source, &serde_json::json!({"width":size[0],"height":size[1],"depth":1,"target":glow::TEXTURE_2D,"format":format,"dataType":data_type,"srgb":false,"renderTarget":true}).to_string())?;
        }
        self.resources.resize_target(self.depth_target, &serde_json::json!({"width":size[0],"height":size[1],"color":self.depth_color,"depth":self.depth_sample,"depthBuffer":true,"stencil":true,"samples":0,"resolveDepth":false,"resolveStencil":false,"cube":false}).to_string())?;
        self.image_step[bfe_render::post::STEP_WIDTH] = size[0] as f64;
        self.image_step[bfe_render::post::STEP_HEIGHT] = size[1] as f64;
        self.bridge.r.replace_sample_texture(self.depth_texture as usize, self.resources.texture(self.depth_sample)?)?;
        for &(source, sample, texture, slot, factor) in &self.post_images {
            use bfe_render::post_chain::layout::{facts as pf, resource as pr};
            let extent=if factor<0.{self.size}else{size.map(|v|(v as f64*factor).ceil().max(1.)as i32)};
            self.resources.source_desc(source, &post_image(extent,slot))?;
            self.resources.upload(sample)?;
            self.bridge.r.replace_sample_texture(texture as usize, self.resources.texture(sample)?)?;
            let at = pf::RESOURCES + slot * pr::STRIDE;
            self.bridge.r.post_chain.input[at + pr::WIDTH] = extent[0] as f64;
            self.bridge.r.post_chain.input[at + pr::HEIGHT] = extent[1] as f64;
        }
        if let Some(host) = &mut self.water { host.resize(&mut self.bridge, &mut self.resources, size)?; }
        self.render_size = size;
        self.bridge.r.core.state.reset();
        self.log_resolution();
        Ok(())
    }
    /// Graphics.applyAntialiasing for the antialiasing setting: the near
    /// scene's target takes the setting's samples (as many as the driver
    /// has), and the far scene's the same unless the temporal upscaler is
    /// chosen, whose jittered history antialiases the far scene's few
    /// silhouettes by itself. FXAA follows every choice but "off" while
    /// the upscaler is not the temporal one (Graphics.render), so it runs
    /// over the multisampled choices too.
    pub fn set_antialiasing(&mut self, choice: &str, upscaler: &str) -> Result<(), String> {
        use bfe_render::post_chain::layout::aa;
        let Some(frame) = &mut self.frame else {
            return Ok(());
        };
        // ANTIALIASING; an unknown choice is the default's four.
        let wanted = match choice {
            "off" | "fxaa-fast" | "fxaa" => 0,
            "2x" => 2,
            "8x" => 8,
            _ => 4,
        };
        let samples = wanted.min(unsafe { self.bridge.r.core.gl.get_parameter_i32(glow::MAX_SAMPLES) }.max(0));
        let temporal = !matches!(upscaler, "off" | "bilinear" | "spatial");
        let far_samples = if temporal { 0 } else { samples };
        frame.aa = match choice {
            _ if temporal => aa::OFF,
            "off" => aa::OFF,
            "fxaa-fast" => aa::FAST,
            _ => aa::FXAA,
        };
        if samples != frame.samples {
            frame.samples = samples;
            self.resources.resize_target(frame.target, &frame_target(self.render_size, frame.sample, true, samples))?;
        }
        if far_samples != frame.far_samples {
            frame.far_samples = far_samples;
            self.resources.resize_target(self.far_target, &frame_target(far_extent(self.render_size, self.reduced_far_terrain), self.far_sample, false, far_samples))?;
        }
        self.bridge.r.core.state.reset();
        self.configure_post()
    }
    /// Graphics.filter for the anisotropy setting: every content image takes
    /// the new value at once, with no decode and no upload. A value that is
    /// no number, or below 1, is 1.
    pub fn set_anisotropy(&mut self, value: f64) -> Result<(), String> {
        let value = if value.is_finite() { value.max(1.) } else { 1. };
        if value == self.anisotropy {
            return Ok(());
        }
        self.anisotropy = value;
        let sampling = content_sampling(value);
        for &(sample,compressed) in &self.filtered {
            let sampling=if compressed{sampling.replace("\"mipmaps\":true","\"mipmaps\":false")}else{sampling.clone()};
            self.resources.sampling(sample, &sampling)?;
        }
        self.bridge.r.core.state.reset();
        Ok(())
    }
    pub fn resize(
        &mut self,
        new_size: [i32; 2],
        mut load: impl FnMut() -> glow::Context,
    ) -> Result<(), String> {
        if new_size[0] <= 0 || new_size[1] <= 0 || new_size == self.size {
            return Ok(());
        }
        self.size = new_size;
        self.render_size = [0, 0];
        self.bridge.r.core.state.reset();
        self.canvas = Canvas::new(load(), new_size)?;
        self.resources.canvas = Some(self.canvas.framebuffer);
        self.bridge.r.core.canvas_framebuffer = Some(self.canvas.framebuffer);
        self.resources.source_desc(
            self.far_source,
            &serde_json::json!({"width":new_size[0],"height":new_size[1],"depth":1,"target":glow::TEXTURE_2D,"format":1023,"dataType":1009,"srgb":false,"renderTarget":true}).to_string(),
        )?;
        self.resources.resize_target(
            self.far_target,
            &frame_target(new_size, self.far_sample, false, self.frame.as_ref().map_or(0, |frame| frame.far_samples)),
        )?;
        if let Some(frame) = &self.frame {
            self.resources.source_desc(frame.source, &frame_image(new_size))?;
            self.resources.resize_target(frame.target, &frame_target(new_size, frame.sample, true, frame.samples))?;
        }
        self.image_step[bfe_render::post::STEP_WIDTH] = new_size[0] as f64;
        self.image_step[bfe_render::post::STEP_HEIGHT] = new_size[1] as f64;
        let mut projection = Matrix4::default();
        let top = 2. * (60.0f64.to_radians() * 0.5).tan();
        let right = top * new_size[0] as f64 / new_size[1] as f64;
        projection.make_perspective(
            -right,
            right,
            top,
            -top,
            2.,
            self.far_reach * self.distance_scale,
            CoordinateSystem::WebGL,
        );
        self.far_projection = projection.elements;
        self.bridge.r.hud.input[bfe_render::hud::IN_VIEWPORT..bfe_render::hud::IN_VIEWPORT + 2]
            .copy_from_slice(&[new_size[0] as f64, new_size[1] as f64]);
        if let Some(online) = &mut self.online {
            online.hud.set_size(new_size);
        }
        self.resize_scene_targets()?;
        Ok(())
    }
    /// A direct join publishes all live owners together before the renderer
    /// borrows their frame banks. The window supplies only its elapsed clock.
    // Presentation borrows an already decoded session. Keeping this boundary
    // separate from socket pumping also lets recorded network sessions exercise
    // the complete native frame without constructing a transport or a World.
    pub fn present_network(&mut self, receiver:&crate::online::receiver::Receiver,
        replay:&crate::online::replay::Replay, history:&crate::online::replay::History,
        camera:&mut MapCamera,dt:f64) {
        let timer=self.bridge.r.core.profile.clock();
        if let Some(shadows) = &mut self.shadows { shadows.frame_time(dt); }
        if let Some(resources)=&mut self.online {
            crate::app::bench::apply_camera(camera);
            let mut world=Matrix4::default();
            world.elements.copy_from_slice(&camera.snapshot().values[16..32]);world.invert();
            resources.items.hide_piloted_remote = receiver.own.remote.is_some()
                && (camera.first_person || receiver.own.remote_zoom_step != 0);
            resources.animation_view(camera);
            let replica_timer = self.bridge.r.core.profile.clock();
            resources.frame(receiver,receiver.clock.shown,dt.min(8./60.),camera.position,
                replay,history,&mut self.bridge);
            self.bridge.r.core.profile.cpu("replica presentation", replica_timer);
            let camera_timer = self.bridge.r.core.profile.clock();
            let rider_eye = resources.prepare_camera(receiver, replay, &world.elements);
            let drawn = crate::online::cameras::DrawnDeath {
                held_camera: resources.animated.held_camera(receiver.own.id as u32, receiver.own.generation as u32),
                chest: resources.drawn_chest(receiver),
                killer_feet: resources
                    .cameras
                    .killer()
                    .and_then(|(id, generation)| resources.drawn_feet(id, generation)),
            };
            let cue = resources.cameras.frame(
                receiver,
                replay,
                &mut resources.hulls,
                &mut resources.turrets,
                camera,
                rider_eye,
                drawn,
                dt,
            );
            // Fixed comparison views must select the same lights and effects
            // as their drawn camera, after the player's camera phase too.
            crate::app::bench::apply_camera(camera);
            world.elements.copy_from_slice(&camera.snapshot().values[16..32]);world.invert();
            resources.first_person.enabled = resources.cameras.eye_view;
            let scope_view = if receiver.own.remote.is_some() {
                camera.first_person
            } else {
                resources.cameras.eye_view
            };
            self.bridge.hud_model.input[bfe_render::hud_model::layout::HudFrame::SCOPE] =
                if resources.hud.scoped && scope_view {
                    resources.hud.scope as f64
                } else {
                    0.
                };
            resources.animation_view(camera);
            self.bridge.r.core.profile.cpu("camera preparation", camera_timer);
            resources.finish_frame(receiver, replay, &world.elements, cue, &mut self.bridge.r.core.profile);
            resources.capture_shot_scene(receiver, camera);
            let ordnance_timer = self.bridge.r.core.profile.clock();
            resources.present_ordnance(receiver, replay, dt.min(8./60.), &mut self.bridge);
            self.bridge.r.core.profile.cpu("ordnance presentation", ordnance_timer);
            let effects_timer = self.bridge.r.core.profile.clock();
            resources.present_arcs(receiver, replay, dt.min(8./60.), &world, &camera.snapshot().values[..16], self.size[1] as f64, &mut self.bridge);
            resources.present_shields(dt.min(8./60.), camera.position, &camera.snapshot().values, self.render_size, self.depth_texture, &mut self.bridge);
            resources.present_weather(dt.min(8./60.), camera.position, &camera.snapshot().values, self.render_size, &mut self.resources, &mut self.bridge);
            resources.present_holograms(receiver, dt.min(8./60.), camera.position, &camera.snapshot().values, &world.elements, &mut self.bridge);
            self.bridge.r.core.profile.cpu("environment effects", effects_timer);
            let hud_timer = self.bridge.r.core.profile.clock();
            resources.lights.frame(dt.min(8./60.), camera.position, camera.far());
            camera.set_lights(&resources.lights.packed, resources.lights.count);
            let mounted = resources.cameras.mounted_aim(camera);
            // Seated, the target search looks from the camera along the
            // seat's view; on foot, from the gun along its aim.
            let ray = match mounted {
                Some(aim) => Some((camera.position, std::array::from_fn(|k| aim.point[k] - camera.position[k]))),
                None => resources.aim_ray(receiver),
            };
            resources.hud.target(&mut self.bridge, receiver, replay, ray);
            let reticle = mounted.or_else(||
                resources.reticle_point(receiver).map(|point| crate::online::hud::Aim { point, project_x: false }));
            resources.hud.aim(&mut self.bridge, camera.snapshot(), reticle, &resources.hulls);
            resources.sound.bleed = [resources.hud.teams[1][3], resources.hud.teams[2][3]];
            resources.sound.own_feet = resources.own_feet(receiver);
            std::mem::swap(&mut resources.sound.turret_seats, &mut resources.hulls.sound_turrets);
            resources.sound.frame(receiver,&world.elements,dt,replay,resources.hud.audio_facts(),&resources.hulls);
            self.bridge.r.core.profile.cpu("HUD and sound presentation", hud_timer);
        }
        self.bridge.r.core.profile.cpu("presentation",timer);
    }
    pub fn set_flight_hud_key(&mut self, key: &str) {
        if let Some(online) = &mut self.online {
            online.hud.set_flight_key(&mut self.bridge, key);
        }
    }
    pub fn online_frame(&mut self,client:&mut crate::online::client::Client,dt:f64){
        self.present_network(&client.receiver,&client.replay,&client.sender.history,&mut client.camera,dt);
        client.receiver.publications.clear();
    }
    pub fn draw(&mut self, view: &View) -> Result<(), String> {
        self.draw_selected(view, Component::All)
    }
    fn draw_selected(&mut self, view: &View, component: Component) -> Result<(), String> {
        if self.profile_start.is_none() && self.frame.is_some() && std::env::var_os("BFE_GRAPHICS_TRACE").is_some() {
            self.bridge.r.core.start_profile();
            self.profile_start=Some(std::time::Instant::now());
            self.profile_memory("battle start");
        }
        if !self.profile_rss_after && self.profile_start.is_some_and(|s|s.elapsed().as_secs()>=60) {
            self.profile_rss_after=true;self.profile_memory("after 60 seconds");
        }
        self.resolution_gpu_begin();
        let build_timer=self.bridge.r.core.profile.clock();
        use bfe_render::frame_plan::layout::{
            binding as b, facts as f, kind as k, meta as m, role as r, stats as t,
        };
        let mut frame_view = *view;
        let view = &mut frame_view;
        if !self.shadows_enabled { set(view, "shadowStrength", &[0.]); }
        set(view, "bump", &[self.bump_mapping as u8 as f64]);
        set(view, "terrainShade", &[self.terrain_detail as u8 as f64]);
        set(view, "resolution", &[self.render_size[0] as f64, self.render_size[1] as f64]);
        self.bridge.frame_begin_frame();
        self.bridge.r.frames.plan.facts.fill(0.);
        self.bridge.r.frames.plan.facts[f::MAIN] = 1.;
        if self.online.is_some(){self.bridge.r.frames.plan.facts[f::HUD_KINDS]=k::HUD as f64;}
        // Target rows are numeric facts, not a host-selected draw sequence.
        // Renderer::battle_frame owns binds, clears, composites and the tail.
        for role in [r::FAR, r::NEAR, r::IMAGE, r::TRANSPARENT, r::HUD0] {
            let row = &mut self.bridge.r.frames.bindings[role * b::LEN..(role + 1) * b::LEN];
            row.fill(0.);
            row[b::TARGET] = match (role, &self.frame) {
                (r::FAR, _) => self.far_target as f64,
                // The HUD draws on the canvas, where the post chain leaves
                // the frame bound.
                (r::HUD0, _) | (_, None) => -1.,
                (_, Some(frame)) => frame.target as f64,
            };
            row[b::SAMPLE] = -1.;
            let size = match role { r::HUD0 => self.size, r::FAR => far_extent(self.render_size, self.reduced_far_terrain), _ => self.render_size };
            row[b::WIDTH] = size[0] as f64;
            row[b::HEIGHT] = size[1] as f64;
            row[b::SCISSOR + 2] = size[0] as f64;
            row[b::SCISSOR + 3] = size[1] as f64;
            row[b::CLEAR..b::CLEAR + 4].copy_from_slice(&[0.04, 0.06, 0.09, 1.]);
            self.bridge.r.frames.meta[role * m::LEN + m::BOUND] = 1.;
        }
        // FountainWater.capture: the opaque scene's colour and depth, copied
        // for the basins' water before the first blend.
        if let Some((target, size)) = self.online.as_ref().and_then(|online| online.fountains.as_ref()).and_then(|f| f.scene_copy()) {
            let row = &mut self.bridge.r.frames.bindings[r::BASIN * b::LEN..(r::BASIN + 1) * b::LEN];
            row.fill(0.);
            row[b::TARGET] = f64::from(target);
            row[b::SAMPLE] = -1.;
            row[b::WIDTH] = f64::from(size[0]);
            row[b::HEIGHT] = f64::from(size[1]);
            row[b::SCISSOR + 2] = f64::from(size[0]);
            row[b::SCISSOR + 3] = f64::from(size[1]);
            self.bridge.r.frames.meta[r::BASIN * m::LEN + m::BOUND] = 1.;
            self.bridge.r.frames.plan.facts[f::FOUNTAIN_COPY] = 1.;
        }
        // ParticleDepth: with soft particles on, the frame plan copies the
        // opaque scene's depth into the depth target before the first blend.
        if self.temporal_motion.is_some()||self.online.as_ref().is_some_and(|online| online.soft_particles()) {
            let row = &mut self.bridge.r.frames.bindings[r::DEPTH * b::LEN..(r::DEPTH + 1) * b::LEN];
            row.fill(0.);
            row[b::TARGET] = self.depth_target as f64;
            row[b::SAMPLE] = -1.;
            row[b::WIDTH] = self.render_size[0] as f64;
            row[b::HEIGHT] = self.render_size[1] as f64;
            self.bridge.r.frames.meta[r::DEPTH * m::LEN + m::BOUND] = 1.;
            self.bridge.r.frames.plan.facts[f::DEPTH_LATE] = 1.;
        }
        if let Some(frame) = &self.frame {
            use bfe_render::post_chain::layout::{facts as pf, resource as pr};
            view.values[frame.slots[0]] = frame.samples as f64;
            view.values[frame.slots[1]] = frame.far_samples as f64;
            // The scene can be smaller than the canvas; the final copy or
            // FXAA stretches it while HUD/page geometry stays full size.
            let (width, height) = (self.size[0] as f64, self.size[1] as f64);
            let facts = &mut self.bridge.r.post_chain.input;
            facts[pf::MODE] = self.post_mode as f64;
            facts[pf::BLOOM] = self.bloom as u8 as f64;
            facts[pf::SOFT_FOCUS] = self.soft_focus as u8 as f64;
            facts[pf::AA] = frame.aa as f64;
            facts[pf::INPUT_WIDTH] = self.render_size[0] as f64;
            facts[pf::INPUT_HEIGHT] = self.render_size[1] as f64;
            for at in [pf::OUTPUT_WIDTH, pf::CANVAS_WIDTH] {
                facts[at] = width;
                facts[at + 1] = height;
            }
            let scene = &mut facts[pf::RESOURCES + pr::SCENE * pr::STRIDE..][..pr::STRIDE];
            scene[pr::TEXTURE] = frame.texture as f64;
            scene[pr::WIDTH] = self.render_size[0] as f64;
            scene[pr::HEIGHT] = self.render_size[1] as f64;
            facts[pf::PROJECTION..pf::PROJECTION+16].copy_from_slice(&view.values[..16]);facts[pf::VIEW..pf::VIEW+16].copy_from_slice(&view.values[16..32]);
            let mut world=Matrix4::default();world.elements.copy_from_slice(&view.values[16..32]);world.invert();facts[pf::WORLD..pf::WORLD+16].copy_from_slice(&world.elements);
            facts[pf::RESOURCES+pr::DEPTH*pr::STRIDE+pr::TEXTURE]=self.depth_texture as f64;
            self.bridge.post_chain_begin_frame();
            if self.temporal_motion.is_some(){use bfe_render::post_chain::layout::state as ps;
                view.values[8]-=self.bridge.r.post_chain.state[ps::SHIFT];view.values[9]-=self.bridge.r.post_chain.state[ps::SHIFT+1];
                self.bridge.r.frames.plan.facts[f::TEMPORAL]=1.;
            }
        }
        let canopy_timer = self.bridge.r.core.profile.clock();
        if let Some(canopies)=&self.canopies{canopies.frame(&mut self.bridge,&mut self.resources,view,self.shadows_enabled)?;}
        self.bridge.r.core.profile.cpu("canopy preparation", canopy_timer);
        self.bridge.r.core.input[..shader::FRAME_FLOATS].copy_from_slice(&view.values);
        let static_timer = self.bridge.r.core.profile.clock();
        crate::static_scene::place_skies(&self.static_content, &mut self.bridge, &view.values, true);
        if matches!(component, Component::All | Component::Sky) {
            let far = self.far_reach * self.distance_scale;
            if let Some(flyers) = &mut self.flyers {
                if flyers.frame(&mut self.bridge, view, far) {
                    let probe = flyers.probe;
                    crate::water_host::WaterHost::camera(&mut self.bridge, &view.values, far, false, 0., [0.;2], 0.);
                    self.sky_probe(view, probe);
                }
            }
        } else if let Some(flyers) = &self.flyers { flyers.hide(&mut self.bridge.r.far.input); }
        self.lamps.begin();
        if matches!(component, Component::All | Component::Water) { self.water_frame(view)?; }
        self.bridge.r.core.profile.cpu("sky and water preparation", static_timer);
        if matches!(component, Component::All | Component::Sky) {
            // Far terrain shares the displayed lens. Its separate clipping
            // planes cannot replace a mounted or scoped camera's field of view.
            // An isolated sky capture retains these far clipping planes too.
            let mut projection = self.far_projection;
            projection[0] = view.values[0];
            projection[5] = view.values[5];
            self.bridge.r.core.input[..16].copy_from_slice(&projection);
        }
        for id in &self.near_world_ids {
            self.bridge
                .r
                .world
                .world_shown(*id, component == Component::All);
        }
        for sky in &self.static_content.skies {
            self.bridge.r.far.input[sky.slot] =
                if matches!(component, Component::All | Component::Sky) && !self.storm.as_ref().is_some_and(|storm| storm.enabled) {
                    1.
                } else {
                    0.
                };
        }
        for slot in &self.far_world_slots {
            self.bridge.r.far.input[*slot + 1] = self.reduced_far_terrain as u8 as f64;
            self.bridge.r.far.input[*slot] = if component == Component::All { 1. } else { 0. };
        }
        self.bridge.r.far.input[bfe_render::far::FAR_CLEAR..bfe_render::far::FAR_CLEAR + 4]
            .copy_from_slice(&[0.04, 0.06, 0.09, 1.]);
        self.bridge.r.far.input[bfe_render::far::FAR_DRAW] = 1.;
        self.publish(r::FAR, k::FAR, 0, 0, 0);
        self.bridge.r.core.input[..shader::FRAME_FLOATS].copy_from_slice(&view.values);
        let cull_timer=self.bridge.r.core.profile.clock();
        let mut records = if matches!(component, Component::All | Component::Props) {
            let room=if component==Component::All{self.online.as_ref().map_or(0,|r|(r.record_budget()+r.hidden_shadow_budget()).min(bfe_render::instances::MAX_RECORDS))}else{0};
            self.static_content.select_limit(&mut self.bridge,bfe_render::instances::MAX_RECORDS-room)
        } else {
            0
        };
        if matches!(component, Component::All | Component::Foliage) {
            self.prepare_foliage(view);
        } else {
            self.bridge.foliage_clear_frame();
        }
        let mut bones = self.bridge.r.core.input[bfe_render::instances::IN_RECORDS..]
            .chunks_exact(bfe_render::instances::REC_STRIDE)
            .take(records)
            .map(|row| {
                (row[bfe_render::instances::REC_BONE_AT] + row[bfe_render::instances::REC_BONES])
                    as usize
            })
            .max()
            .unwrap_or(0);
        if component==Component::All{if let Some(online)=&mut self.online{(records,bones)=online.append(&mut self.bridge,records,bones);}}
        self.bridge.r.core.profile.cpu("instance list and culling",cull_timer);
        let lamp_timer = self.bridge.r.core.profile.clock();
        let online=&self.online;
        self.lamps.instances(&mut self.bridge.r.core,records,false,&mut |from,to|online.as_ref().is_none_or(|online|online.lamp_visible(from,to)));
        if !self.lamp_worlds.is_empty(){let values=self.lamps.select((1u64<<49,0,0,0),[0.;3],&mut |from,to|online.as_ref().is_none_or(|online|online.lamp_visible(from,to)));for &id in &self.lamp_worlds{self.bridge.world_lamps(id,&values);}}
        self.bridge.r.core.profile.cpu("lamp selection", lamp_timer);
        let motion_timer = self.bridge.r.core.profile.clock();
        if let Some(native)=&mut self.temporal_motion{
            use bfe_render::{motion as M,post_chain::layout::{facts as pf,resource as pr,state as ps}};
            let motion=&mut self.bridge.r.motion;let f=&self.bridge.r.post_chain.input;let state=&self.bridge.r.post_chain.state;
            motion.input[M::IN_PROJECTION..M::IN_PROJECTION+16].copy_from_slice(&view.values[..16]);motion.input[M::IN_VIEW..M::IN_VIEW+16].copy_from_slice(&view.values[16..32]);
            motion.input[M::IN_VIEW_VIEW..M::IN_VIEW_VIEW+16].copy_from_slice(&view.values[16..32]);
            motion.input[M::IN_CURRENT..M::IN_CURRENT+16].copy_from_slice(&state[ps::CURRENT..ps::CURRENT+16]);motion.input[M::IN_PREVIOUS..M::IN_PREVIOUS+16].copy_from_slice(&state[ps::PREVIOUS..ps::PREVIOUS+16]);
            motion.input[M::IN_RANGE..M::IN_RANGE+2].copy_from_slice(&[0.,1.]);motion.input[M::IN_TARGET]=f[pf::RESOURCES+pr::VELOCITY*pr::STRIDE+pr::TEXTURE];motion.input[M::IN_SIZE..M::IN_SIZE+2].copy_from_slice(&self.render_size.map(|v|v as f64));motion.input[M::IN_DEPTH]=self.depth_texture as f64;motion.input[M::IN_CLEAR]=1.;
            native.prepare(&self.bridge.r.core,motion,records,state[ps::VALID]!=0.);
        }
        self.bridge.r.core.profile.cpu("motion preparation", motion_timer);
        let shadow_timer=self.bridge.r.core.profile.clock();
        if self.shadows_enabled { if let Some(shadows) = &mut self.shadows {
            let shadow_records = self.online.as_ref().map_or(records, |online| {
                online.append_hidden_shadows(&mut self.bridge, records, bones)
            });
            shadows.publish(&mut self.bridge, view, shadow_records)?;
        }}
        self.bridge.r.core.profile.cpu("shadow preparation",shadow_timer);
        let packet_timer = self.bridge.r.core.profile.clock();
        self.bridge.r.core.input[..shader::FRAME_FLOATS].copy_from_slice(&view.values);
        self.publish_near(records, bones);
        self.bridge.r.post.input[..bfe_render::post::STEP].copy_from_slice(&self.image_step);
        self.bridge.r.post.states[..bfe_render::post::STATE].copy_from_slice(&self.image_state);
        self.publish(r::IMAGE, k::SCREEN, 0, 0, 1);
        if let Some(online)=&self.online{self.bridge.r.frames.meta[r::TRANSPARENT*m::LEN+m::FX_WORDS]=online.combat.particles.words as f64;}
        self.publish(r::TRANSPARENT, k::TRANSPARENT, records, bones, 0);
        if component == Component::All {
            if let Some(online) = &mut self.online {
                online.hulls.draw_tethers(&view.values, &mut self.bridge)?;
            }
        }
        if let Some(online)=&self.online{let (view_records,view_bones)=online.first_person.append(&online.units,&mut self.bridge);if view_records!=0{
                // Graphics.render gives the view camera the drawn camera's own
                // field of view and aspect each frame (near 0.3, far 10), so the
                // arms narrow with the first-person lens instead of keeping 60 degrees.
                let mut projection = Matrix4::default();
                let top = 0.3 / view.values[5];
                let right = 0.3 / view.values[0];
                projection.make_perspective(-right, right, top, -top, 0.3, 10., CoordinateSystem::WebGL);
                projection.elements[8]=view.values[8];projection.elements[9]=view.values[9];
                let source=&self.online;self.lamps.instances(&mut self.bridge.r.core,view_records,true,&mut |from,to|source.as_ref().is_none_or(|online|online.lamp_visible(from,to)));
                if let Some(native)=&mut self.temporal_motion{use bfe_render::motion as M;self.bridge.r.motion.input[M::IN_VIEW_PROJECTION..M::IN_VIEW_PROJECTION+16].copy_from_slice(&projection.elements);native.view(&self.bridge.r.core,&mut self.bridge.r.motion,view_records);}
                self.bridge.r.core.input[..16].copy_from_slice(&projection.elements);self.bridge.r.frames.plan.facts[f::VIEW_SHOWN]=1.;
                // The view scene packs its own lights: only the copies syncViewLights makes.
                if let Some(online)=&self.online{let l=&online.lights;let n=l.view_count;self.bridge.r.core.input[shader::FRAME_LIGHT_COUNT]=n as f64;self.bridge.r.core.input[shader::FRAME_LIGHTS..shader::FRAME_LIGHTS+n*8].copy_from_slice(&l.view_packed[..n*8]);}self.publish(r::VIEW,k::VIEW,view_records,view_bones,0);}}
        if let Some(native)=&self.temporal_motion{let count=native.count;self.publish(r::MOTION,k::MOTION,count,0,0);}
        if self.online.is_some(){self.publish(r::HUD0,k::HUD,0,0,0);}
        self.water_screens(component);
        self.bridge.r.core.profile.cpu("frame packet publication", packet_timer);
        self.bridge.r.core.profile.cpu("draw list building",build_timer);
        let submit_timer=self.bridge.r.core.profile.clock();
        let gpu_frame = self.bridge.r.core.profile.begin_frame(&self.bridge.r.core.gl);
        let success = self.bridge.battle_frame(&mut self.resources);
        self.bridge.r.core.profile.end(&self.bridge.r.core.gl, "GPU frame", gpu_frame);
        if let Some(native)=&mut self.temporal_motion{native.complete(success);}
        self.bridge.r.core.profile.cpu("GL submit",submit_timer);
        self.bridge.r.core.profile.frame(&self.bridge.r.core.gl);
        if !success {
            return Err(self
                .bridge
                .frame_error_frame()
                .unwrap_or("native map frame failed".into()));
        }
        self.draws += self
            .bridge
            .r
            .frames
            .stats
            .chunks_exact(t::LEN)
            .map(|row| row[t::PASS_DRAWS..].iter().sum::<f64>() as u64)
            .sum::<u64>();
        Ok(())
    }
    // Water.update and Water.reflect for this frame: the host's rows and
    // facts, then the mirrored far scene when the water asks for a new image.
    fn water_frame(&mut self, view: &mut View) -> Result<(), String> {
        use bfe_render::frame_plan::layout::{binding as b, facts as f, kind as k, meta as m, role as r};
        let Some(host) = self.water.as_mut() else {
            return Ok(());
        };
        let clock = view.values[shader::FRAME_OFFSETS[shader::FRAME_MEMBERS.iter().position(|(n, _)| *n == "time").unwrap_or(0)]];
        // The frame step is the elapsed battle time, as the client's frame
        // delta was: capped where the client capped it.
        let dt = if self.water_time.is_nan() { 0. } else { (clock - self.water_time).clamp(0., 8. / 60.) };
        self.water_time = clock;
        // Without a battle nothing else builds the effect list.
        if self.online.is_none() {
            self.bridge.r.effects.input[bfe_render::effects::IN_DRAWS] = 0.;
        }
        let light = self.storm.as_mut().and_then(|storm| storm.frame(&mut self.bridge, view, host.time() + dt));
        host.storm_light(light);
        if let Some(online) = &self.online {
            let mut soldiers = std::mem::take(&mut self.water_soldiers);
            online.soldiers_on_foot(&mut soldiers);
            host.soldiers(dt, &soldiers);
            self.water_soldiers = soldiers;
        }
        host.frame(&mut self.bridge, &mut self.resources, &view.values, dt, self.water_data, self.online.as_ref().map(|online| online.water_collision()))?;
        if host.plan.probe {
            if let Some(probe) = host.ocean().and_then(|o| o.probe) { self.sky_probe(view, probe); }
        }
        let Some(host) = self.water.as_mut() else {
            return Ok(());
        };
        let facts = &mut self.bridge.r.frames.plan.facts;
        if let Some(target) = host.plan.surface {
            // The scene copy: the near view's colour and depth before the
            // first pass that reads them (the surface binding's target).
            facts[f::SURFACE_COPY] = f64::from(host.plan.copy);
            let row = &mut self.bridge.r.frames.bindings[r::SURFACE * b::LEN..(r::SURFACE + 1) * b::LEN];
            row.fill(0.);
            row[b::TARGET] = f64::from(target);
            row[b::SAMPLE] = -1.;
            row[b::WIDTH] = f64::from(self.render_size[0]);
            row[b::HEIGHT] = f64::from(self.render_size[1]);
            row[b::SCISSOR + 2] = f64::from(self.render_size[0]);
            row[b::SCISSOR + 3] = f64::from(self.render_size[1]);
            self.bridge.r.frames.meta[r::SURFACE * m::LEN + m::BOUND] = 1.;
        }
        if !host.plan.mirror {
            return Ok(());
        }
        // The reflected far scene (BattleFrame.mirror): the sky layers first,
        // through the plain projection, then the rest of the far scene over
        // it through the one clipped at the surface, into the mirror's target.
        let (target, size, near) = (host.plan.target, host.plan.size, host.plan.near);
        let (plain, clipped) = (std::mem::take(&mut host.plan.plain), std::mem::take(&mut host.plan.clipped));
        let hidden = host.hide_far(&mut self.bridge.r.far.input);
        let sky = !self.static_content.skies.is_empty();
        self.bridge.r.frames.plan.facts[f::MIRROR_SKY] = f64::from(sky);
        for role in [r::MIRROR0_SKY, r::MIRROR0_FAR] {
            let row = &mut self.bridge.r.frames.bindings[role * b::LEN..(role + 1) * b::LEN];
            row.fill(0.);
            row[b::TARGET] = f64::from(target);
            row[b::SAMPLE] = -1.;
            row[b::WIDTH] = f64::from(size[0]);
            row[b::HEIGHT] = f64::from(size[1]);
            row[b::SCISSOR + 2] = f64::from(size[0]);
            row[b::SCISSOR + 3] = f64::from(size[1]);
            self.bridge.r.frames.meta[role * m::LEN + m::BOUND] = 1.;
        }
        self.bridge.r.far.input[bfe_render::far::FAR_DRAW] = 1.;
        if sky {
            for layer in &self.static_content.skies {
                self.bridge.r.far.input[layer.slot] = 1.;
            }
            for &at in &self.far_world_slots {
                self.bridge.r.far.input[at] = 0.;
            }
            crate::static_scene::place_skies(&self.static_content, &mut self.bridge, &plain, false);
            self.bridge.r.core.input[..shader::FRAME_FLOATS].copy_from_slice(&plain);
            self.publish(r::MIRROR0_SKY, k::FAR, 0, 0, 0);
            for layer in &self.static_content.skies {
                self.bridge.r.far.input[layer.slot] = 0.;
            }
        }
        for &at in &self.far_world_slots {
            self.bridge.r.far.input[at] = 1.;
        }
        crate::static_scene::place_skies(&self.static_content, &mut self.bridge, &clipped, false);
        self.bridge.r.core.input[..shader::FRAME_FLOATS].copy_from_slice(&clipped);
        self.publish(r::MIRROR0_FAR, k::FAR, 0, 0, 0);
        if let Some((target, texture)) = near { self.water_mirror_near(&clipped, size, target, texture); }
        // The main view again.
        for layer in &self.static_content.skies {
            self.bridge.r.far.input[layer.slot] = 1.;
        }
        crate::static_scene::place_skies(&self.static_content, &mut self.bridge, &view.values, true);
        self.bridge.r.core.input[..shader::FRAME_FLOATS].copy_from_slice(&view.values);
        if let Some(host) = self.water.as_mut() {
            host.show_far(&mut self.bridge.r.far.input, hidden);
            host.plan.plain = plain;
            host.plan.clipped = clipped;
        }
        Ok(())
    }
    // Ultra's second reflected scene uses full-detail architecture and live
    // bodies. Its separate far image supplies the background and fade reads.
    fn water_mirror_near(&mut self, clipped: &[f64], size: [i32; 2], target: u32, texture: u32) {
        use bfe_render::{effects as fx, instances as i, post,
            frame_plan::layout::{binding as b, kind as k, meta as m, role as r}};
        let mut view = View { values: clipped.try_into().expect("mirror frame block") };
        set(&mut view, "resolution", &[size[0] as f64, size[1] as f64]);
        let soft = self.bridge.r.effects.input[fx::IN_SOFT];
        self.bridge.r.effects.input[fx::IN_SOFT] = 0.;
        set(&mut view, "aaSamples", &[0.]);
        set(&mut view, "farAaSamples", &[0.]);
        self.bridge.r.core.input[..shader::FRAME_FLOATS].copy_from_slice(&view.values);
        self.bridge.set_far_map(texture);
        let count = self.bridge.r.effects.input[fx::IN_DRAWS] as usize;
        self.bridge.r.effects.input[fx::IN_DRAWS] = self.water.as_ref().map_or(count, |host| host.draws_before_water(count)) as f64;
        let room = self.online.as_ref().map_or(0, |online| online.record_budget()).min(i::MAX_RECORDS);
        let mut records = self.static_content.select_limit(&mut self.bridge, i::MAX_RECORDS - room);
        let mut bones = self.bridge.r.core.input[i::IN_RECORDS..].chunks_exact(i::REC_STRIDE).take(records)
            .map(|row| (row[i::REC_BONE_AT] + row[i::REC_BONES]) as usize).max().unwrap_or(0);
        if let Some(online) = &mut self.online { (records, bones) = online.append(&mut self.bridge, records, bones); }
        let online = &self.online;
        self.lamps.instances(&mut self.bridge.r.core, records, false,
            &mut |from, to| online.as_ref().is_none_or(|online| online.lamp_visible(from, to)));
        self.prepare_foliage(&view);
        for role in [r::MIRROR0_NEAR, r::MIRROR0_IMAGE, r::MIRROR0_TRANSPARENT] {
            let row = &mut self.bridge.r.frames.bindings[role * b::LEN..][..b::LEN];
            row.fill(0.);
            row[b::TARGET] = target as f64;
            row[b::SAMPLE] = -1.;
            row[b::WIDTH] = size[0] as f64;
            row[b::HEIGHT] = size[1] as f64;
            row[b::SCISSOR + 2] = size[0] as f64;
            row[b::SCISSOR + 3] = size[1] as f64;
            self.bridge.r.frames.meta[role * m::LEN + m::BOUND] = 1.;
        }
        self.publish(r::MIRROR0_NEAR, k::SCENE, records, bones, 0);
        let mut image = self.image_step;
        image[post::STEP_WIDTH] = size[0] as f64;
        image[post::STEP_HEIGHT] = size[1] as f64;
        image[post::STEP_FIRST + 2] = texture as f64;
        self.bridge.r.post.input[..post::STEP].copy_from_slice(&image);
        self.bridge.r.post.states[..post::STATE].copy_from_slice(&self.image_state);
        self.publish(r::MIRROR0_IMAGE, k::SCREEN, 0, 0, 1);
        if let Some(online) = &self.online {
            self.bridge.r.frames.meta[r::MIRROR0_TRANSPARENT * m::LEN + m::FX_WORDS] = online.combat.particles.words as f64;
        }
        self.publish(r::MIRROR0_TRANSPARENT, k::TRANSPARENT, records, bones, 0);
        self.bridge.r.effects.input[fx::IN_DRAWS] = count as f64;
        self.bridge.r.effects.input[fx::IN_SOFT] = soft;
        self.bridge.set_far_map(self.image_step[post::STEP_FIRST + 2] as u32);
    }
    // SkyProbe.update's capture: the map's sky in the six faces of a cube
    // around the eye, drawn as the far scene draws it, which an Enhanced sea
    // reflects. The plan decides when one is due.
    fn sky_probe(&mut self, view: &View, probe: crate::ocean_host::Probe) {
        use bfe_render::{
            cameras::layout as cl,
            frame_plan::layout::{binding as b, kind as k, meta as m, role as r},
        };
        let (target, sample, size) = (probe.target, probe.sample, probe.size);
        self.bridge.camera_probe_frame(0);
        // The sky alone: no far scenery, no water.
        if let Some(host) = &self.water { host.hide_all_far(&mut self.bridge.r.far.input); }
        if let Some(flyers) = &self.flyers { flyers.hide(&mut self.bridge.r.far.input); }
        let mut block = [0.; shader::FRAME_FLOATS];
        for &at in &self.far_world_slots {
            self.bridge.r.far.input[at] = 0.;
        }
        for layer in &self.static_content.skies {
            self.bridge.r.far.input[layer.slot] = 1.;
        }
        self.bridge.r.far.input[bfe_render::far::FAR_CLEAR..bfe_render::far::FAR_CLEAR + 4].copy_from_slice(&[0.04, 0.06, 0.09, 1.]);
        self.bridge.r.far.input[bfe_render::far::FAR_DRAW] = 1.;
        for face in 0..6 {
            let role = r::PROBE0 + face;
            let row = &mut self.bridge.r.frames.bindings[role * b::LEN..(role + 1) * b::LEN];
            row.fill(0.);
            row[b::TARGET] = f64::from(target);
            row[b::FACE] = face as f64;
            row[b::SAMPLE] = f64::from(sample);
            row[b::WIDTH] = f64::from(size);
            row[b::HEIGHT] = f64::from(size);
            row[b::SCISSOR + 2] = f64::from(size);
            row[b::SCISSOR + 3] = f64::from(size);
            self.bridge.r.frames.meta[role * m::LEN + m::BOUND] = 1.;
            // The face's camera, from the camera owner's probe views.
            let at = (cl::role::PROBE0 + face) * cl::camera::LEN;
            let camera = &self.bridge.r.cameras.output[at..at + cl::camera::LEN];
            block.copy_from_slice(&view.values);
            block[0..16].copy_from_slice(&camera[cl::camera::PROJECTION..cl::camera::PROJECTION + 16]);
            block[16..32].copy_from_slice(&camera[cl::camera::INVERSE..cl::camera::INVERSE + 16]);
            // A cube face keeps its local position at zero: the eye is the
            // translation of its world matrix.
            block[32..35].copy_from_slice(&camera[cl::camera::WORLD + 12..cl::camera::WORLD + 15]);
            crate::static_scene::place_skies(&self.static_content, &mut self.bridge, &block, true);
            self.bridge.r.core.input[..shader::FRAME_FLOATS].copy_from_slice(&block);
            self.publish(role, k::FAR, 0, 0, 0);
        }
        // The main view again: its own far scene sets the far input anew.
        for &at in &self.far_world_slots {
            self.bridge.r.far.input[at] = 1.;
        }
        crate::static_scene::place_skies(&self.static_content, &mut self.bridge, &view.values, true);
        self.bridge.r.core.input[..shader::FRAME_FLOATS].copy_from_slice(&view.values);
        if let Some(flyers) = &mut self.flyers { flyers.place(&mut self.bridge, view, self.far_reach*self.distance_scale, 0.); }
    }
    // The near phase's packet. The sea's near surface is a far-pass item the
    // near phase draws (FarPass.drawNear), so that packet shows it alone.
    fn publish_near(&mut self, records: usize, bones: usize) {
        use bfe_render::frame_plan::layout::{kind as k, role as r};
        let sea = self.water.as_ref().and_then(|host| host.plan.sea.map(|slot| (host, slot)));
        let Some((host, slot)) = sea else {
            self.publish(r::NEAR, k::SCENE, records, bones, 0);
            return;
        };
        let far = &mut self.bridge.r.far.input;
        for layer in &self.static_content.skies {
            far[layer.slot] = 0.;
        }
        for &at in &self.far_world_slots {
            far[at] = 0.;
        }
        host.hide_all_far(far);
        if let Some(storm) = &self.storm { storm.hide(far); }
        if let Some(flyers) = &self.flyers { flyers.hide(far); }
        far[slot] = 1.;
        far[bfe_render::far::FAR_NEAR] = 1.;
        // The sea draws over what the phase drew: the far pass would clear it.
        far[bfe_render::far::FAR_KEEP] = 1.;
        self.publish(r::NEAR, k::SCENE, records, bones, 0);
        let far = &mut self.bridge.r.far.input;
        far[bfe_render::far::FAR_NEAR] = 0.;
        far[bfe_render::far::FAR_KEEP] = 0.;
        far[slot] = 0.;
    }
    // Water.drawUnderwater and the Enhanced underwater fog: screen packets
    // the frame plan draws after the post chain and after the transparent
    // list, when the eye is under the surface.
    fn water_screens(&mut self, component: Component) {
        use bfe_render::frame_plan::layout::{binding as b, kind as k, meta as m, role as r};
        let Some(host) = self.water.as_ref().filter(|_| matches!(component, Component::All | Component::Water)) else {
            return;
        };
        let (underwater, fog, time) = (host.plan.underwater, host.plan.fog, host.time());
        for (role, active, with_fog) in [(r::UNDERWATER, underwater, false), (r::FOG, fog, true)] {
            if !active {
                continue;
            }
            self.water.as_ref().map(|host| host.screen(&mut self.bridge, with_fog, time));
            let row = &mut self.bridge.r.frames.bindings[role * b::LEN..(role + 1) * b::LEN];
            row.fill(0.);
            row[b::TARGET] = if with_fog { self.frame.as_ref().map_or(-1., |frame| f64::from(frame.target)) } else { -1. };
            row[b::SAMPLE] = -1.;
            let size = if with_fog { self.render_size } else { self.size };
            row[b::WIDTH] = f64::from(size[0]);
            row[b::HEIGHT] = f64::from(size[1]);
            row[b::SCISSOR + 2] = f64::from(size[0]);
            row[b::SCISSOR + 3] = f64::from(size[1]);
            self.bridge.r.frames.meta[role * m::LEN + m::BOUND] = 1.;
            self.bridge.r.post.input[bfe_render::post::STEP_WIDTH] = size[0] as f64;
            self.bridge.r.post.input[bfe_render::post::STEP_HEIGHT] = size[1] as f64;
            self.publish(role, k::SCREEN, 0, 0, 1);
        }
    }
    fn publish(&mut self, role: usize, kind: usize, records: usize, bones: usize, steps: usize) {
        use bfe_render::frame_plan::layout::meta as m;
        let row = &mut self.bridge.r.frames.meta[role * m::LEN..(role + 1) * m::LEN];
        row[m::KIND] = kind as f64;
        row[m::RECORDS] = records as f64;
        row[m::BONES] = bones as f64;
        row[m::STEPS] = steps as f64;
        // FrameBank clips a full packet and retains its fault marker, just
        // as the client producer does. Capacity never terminates the window.
        self.bridge.frame_publish_frame(role as u32);
    }
    fn prepare_foliage(&mut self, view: &View) {
        let input = &mut self.bridge.r.foliage.input;
        input[bfe_render::foliage::IN_PROJECTION..bfe_render::foliage::IN_PROJECTION + 16]
            .copy_from_slice(&view.values[..16]);
        input[bfe_render::foliage::IN_VIEW..bfe_render::foliage::IN_VIEW + 16]
            .copy_from_slice(&view.values[16..32]);
        let mut world = Matrix4::default();
        world.elements.copy_from_slice(&view.values[16..32]);
        world.invert();
        input[bfe_render::foliage::IN_WORLD..bfe_render::foliage::IN_WORLD + 16]
            .copy_from_slice(&world.elements);
        input[bfe_render::foliage::IN_EYE..bfe_render::foliage::IN_EYE + 3]
            .copy_from_slice(&view.values[32..35]);
        input[bfe_render::foliage::IN_END] = view.values[bfe_render::cameras::layout::shader_frame::FADE + 1];
        input[bfe_render::foliage::IN_TIME] = view.values[35];
        self.bridge.foliage_frame();
    }
    /// Probe visibility uses the same packets and battle_frame as the window.
    /// Only admitted object prefixes change; no alternate pass order exists.
    pub fn draw_component(&mut self, view: &View, name: &str) -> Result<(), String> {
        let component = match name {
            "all" => Component::All,
            "props" => Component::Props,
            "foliage" => Component::Foliage,
            "water" => Component::Water,
            "sky" => Component::Sky,
            _ => return Err("unknown static component".into()),
        };
        self.draw_selected(view, component)
    }
    pub fn frame_trace(&self) -> &[f64] {
        use bfe_render::frame_plan::layout::command as c;
        &self.bridge.r.frames.plan.trace[..self.bridge.r.frames.plan.count * c::LEN]
    }
    /// Cold diagnostics for the isolated-view tool; never read by the frame loop.
    pub fn bindings(&self) -> serde_json::Value {
        use bfe_render::instances as I;
        let rigs=self.static_content.records.iter().map(|r|{let start=r[I::REC_BONE_AT] as usize*16;let len=r[I::REC_BONES] as usize*16;serde_json::json!({"world":&r[I::REC_WORLD..I::REC_WORLD+16],"bind":r[I::REC_BIND_INVERSE..I::REC_BIND_INVERSE+16].iter().map(|v|*v as f32 as f64).collect::<Vec<_>>(),"bones":self.static_content.bones[start..start+len].iter().map(|v|*v as f64).collect::<Vec<_>>()})}).collect::<Vec<_>>();
        let uniforms = serde_json::Map::<String, serde_json::Value>::new();
        serde_json::json!({"rigs":rigs,"uniforms":uniforms})
    }
    pub fn hash(&self) -> Result<String, String> {
        use sha2::{Digest, Sha256};
        Ok(format!("{:x}", Sha256::digest(self.pixels()?)))
    }
    pub fn pixels(&self) -> Result<Vec<u8>, String> {
        let gl = &self.bridge.r.core.gl;
        let mut bytes = vec![0; self.size[0] as usize * self.size[1] as usize * 4];
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.canvas.framebuffer));
            gl.read_pixels(
                0,
                0,
                self.size[0],
                self.size[1],
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut bytes)),
            );
            let error = gl.get_error();
            if error != glow::NO_ERROR {
                return Err(format!("native scene GL error {error}"));
            }
        }
        Ok(bytes)
    }
}

// Sky layers have their own admission; every nonempty world material is drawn.
fn inspectable(mesh: &bfe_content_native::loader::Mesh) -> bool {
    mesh.geometry.vertices != 0
        && !mesh.geometry.indices.is_empty()
        && mesh.material.kind != MaterialKind::Sky
}

// materials.js::setLighting/setFog read only authored map content. The inspector
// uses these values without fabricating point lights, shadow facts or actors.
fn environment(view: &mut View, map: &serde_json::Value) {
    let l = &map["lighting"];
    for (uniform, key) in [
        ("ambient", "ambient"),
        ("ambientTop", "top"),
        ("ambientBottom", "bottom"),
        ("characterAmbient", "character"),
        ("sunColor", "sunColor"),
    ] {
        let color = if uniform == "characterAmbient" && l[key].is_null() {
            &l["ambient"]
        } else {
            &l[key]
        };
        let values = std::array::from_fn::<_, 3, _>(|i| color[i].as_f64().unwrap_or(128.) / 255.);
        set(view, uniform, &values);
    }
    let mut sun = Vector3::new(0., -1., 0.);
    if let Ok(v) = serde_json::from_value::<[f64; 3]>(l["sunDirection"].clone()) {
        sun.set(v[0], v[1], v[2]).normalize();
    }
    set(view, "sunDirection", &[sun.x, sun.y, sun.z]);
    set(view, "shadowStrength", &[1.]);
    let low = &map["distance"]["lowRes"];
    if let Ok(fog) = serde_json::from_value::<[f64; 3]>(low["fog"].clone()) {
        set(view, "fogColor", &fog.map(|v| v / 255.));
    }
    let range = serde_json::from_value::<[f64; 2]>(low["fogRange"].clone())
        .ok()
        .filter(|r| !low["fog"].is_null() && r[0] >= 0. && r[0] < r[1])
        .unwrap_or([1e9, 2e9]);
    set(view, "fogRange", &range);
}

#[cfg(test)]
mod camera_tests {
    use super::*;

    #[test]
    fn saved_cockpit_perspective_applies_only_when_boarding() {
        let mut camera = MapCamera::new([0.; 3], [1280, 720]).unwrap();
        camera.set_flight_perspective(true);
        assert!(!camera.first_person);
        camera.set_flight_mode(true);
        assert!(camera.first_person);
        camera.set_flight_mode(false);
        assert!(camera.first_person);
    }

    #[test]
    fn exiting_keeps_the_view_and_ground_toggles_preserve_the_flight_setting() {
        let mut camera = MapCamera::new([0.; 3], [1280, 720]).unwrap();
        camera.set_flight_mode(true);
        camera.first_person = true;
        camera.set_flight_mode(false);
        assert!(camera.first_person);
        camera.first_person = false;
        camera.set_flight_mode(true);
        assert!(camera.first_person);
        camera.set_flight_mode(false);
        camera.first_person = true;
        camera.set_flight_mode(true);
        camera.first_person = false;
        camera.set_flight_mode(false);
        assert!(!camera.first_person);
    }

    #[test]
    fn following_a_subject_preserves_the_camera_phase_pose() {
        use bfe_presentation::layout::camera_output as o;
        let mut camera = MapCamera::new([0.; 3], [1280, 720]).unwrap();
        let mut output = [0.; o::STRIDE];
        output[o::POSITION..o::POSITION + 3].copy_from_slice(&[4., 8., 12.]);
        output[o::PROJECTION..o::PROJECTION + 16].copy_from_slice(&Matrix4::default().elements);
        let mut inverse = Matrix4::default();
        inverse.make_rotation_y(0.4);
        output[o::INVERSE..o::INVERSE + 16].copy_from_slice(&inverse.elements);
        camera.accept_camera(&output);
        let displayed = *camera.snapshot();
        camera.follow([100., 200., 300.], 0.5, -0.2, 0.1);
        camera.present(0.5);
        assert_eq!(camera.position, [4., 8., 12.]);
        assert_eq!(camera.subject_eye(), [100., 200., 300.]);
        assert_eq!(&camera.snapshot().values[..32], &displayed.values[..32]);
        assert!((camera.aim()[0] - 0.5).abs() < 1e-14);
        assert_eq!(camera.aim()[1], -0.2);
        assert_eq!(camera.snapshot().values[camera.slots[2]], 0.1);
    }
    #[test]
    fn render_scale_applies_once_to_device_pixels_and_keeps_the_adaptive_floor(){
        assert_eq!(render_extent([2512,1500],1.5,1.),[3768,2250]);
        assert_eq!(render_extent([2512,1500],1.5,0.5),[1884,1125]);
        assert_eq!(render_extent([2512,1500],0.25,0.5),[628,375]);
        // The adaptive floor: 720 lines or half the window, never below the player's own scale.
        assert_eq!(render_extent([1920,1080],1.,0.5),[1280,720]);
        assert_eq!(render_extent([1920,996],0.35,0.5),[672,349]);
        assert_eq!(render_extent([3840,2160],1.,0.5),[1920,1080]);
        assert_eq!(render_extent([1280,720],1.,0.5),[1280,720]);
    }
    #[test]
    fn dynamic_resolution_shrinks_only_when_the_scene_is_what_costs(){
        // 60 fps budget; a 1080p window drawn at full scale, floor 2/3.
        let b=1000./60.;let floor=2./3.;
        // The scene is most of a slow frame: shrinking can reach the line.
        assert!(resolution_can_help(20.,16.,1.,floor,b));
        // A slow frame made of window-size work (an interface layer, the stretch): it cannot.
        assert!(!resolution_can_help(20.,6.,1.,floor,b));
        // Sixty is out of reach, but the floor takes a third off the frame: worth it.
        assert!(resolution_can_help(28.,16.,1.,floor,b));
        // Already at the floor: nothing left to take.
        assert!(!resolution_can_help(20.,16.,floor,floor,b));
        // Under the line, or no split measured: the controller decides as before.
        assert!(resolution_can_help(10.,2.,1.,floor,b));
        assert!(resolution_can_help(20.,f64::NAN,1.,floor,b));
    }
    #[test]
    fn the_reduced_far_image_is_half_the_scene_and_never_empty(){
        assert_eq!(far_extent([1920,1080],true),[960,540]);
        assert_eq!(far_extent([3840,2160],true),[1920,1080]);
        // Never fewer than 540 lines: Minimum's 960x540 scene keeps a full-size far image.
        assert_eq!(far_extent([1280,720],true),[960,540]);
        assert_eq!(far_extent([960,540],true),[960,540]);
        assert_eq!(far_extent([480,270],true),[480,270]);
        assert_eq!(far_extent([1,1],true),[1,1]);
        assert_eq!(far_extent([1920,1080],false),[1920,1080]);
    }

}
