//! The destruction and weather presentation phase. The page writes scalar
//! commands (a birth, an arc), the world matrices of the objects arcs hang
//! from and the frame's time into fixed blocks; one call per frame advances
//! the pooled state and leaves instance records the renderer draws, in
//! memory the page reads through typed-array views. Block layouts come from
//! layout.json (tools/environment-schema.mjs).

pub mod atmosphere;
pub mod common;
pub mod content;
pub mod admission;
pub mod publication;
#[allow(dead_code)]
mod effect_layout;
pub mod cover;
pub mod destruction;
pub mod debris;
pub mod environment;
pub mod fx;
pub mod layout;
pub mod random;
pub mod sky;
pub mod solver;
pub mod weather;
pub mod snow;
pub mod surfaces;
pub mod surface_policy;
pub mod water_draw;
pub mod shore;
pub mod surface_math;
pub mod wakes;
pub mod storm;

pub use destruction::Destruction;
pub use fx::Fx;

/// The atmosphere phase and the blocks it shares with the page.
// The atmosphere phase and the blocks it shares with the page: the page writes
// the configuration (once) and the camera (each frame) and reads each batch's
// records and the camera axes.
pub struct AtmospherePhase {
    pub config: Vec<f64>,
    pub instances: Vec<f64>,
    pub view: Vec<f64>,
    pub result: Vec<f64>,
    pub camera: common::Camera,
    pub atmosphere: Option<atmosphere::Atmosphere>,
}

impl AtmospherePhase {
    /// Blocks for a fog (or not) and `classes` dust classes with `instances`
    /// objects in all; the page fills config and instances, then `start`s.
    pub fn new(classes: usize, instances: usize, fog: bool) -> AtmospherePhase {
        let batches = classes + usize::from(fog);
        AtmospherePhase {
            config: vec![0.0; layout::atmosphere_config::STRIDE + classes * layout::dust_config::STRIDE],
            instances: vec![0.0; instances * 3],
            view: vec![0.0; layout::view::STRIDE],
            result: vec![0.0; layout::atmosphere_stats::STRIDE + batches * layout::square_batch::STRIDE],
            camera: common::Camera::new(),
            atmosphere: None,
        }
    }

    pub fn start(&mut self) {
        self.atmosphere = Some(atmosphere::Atmosphere::new(&self.config, &self.instances));
    }

    /// The frame's phase (Atmosphere.update). `ground` answers the terrain's
    /// height under a point of the grid (NaN for none).
    pub fn update(&mut self, ground: &mut dyn FnMut(f64, f64) -> f64) {
        self.camera.read(&self.view);
        self.atmosphere.as_mut().expect("start first").update(&self.camera, ground, &mut self.result);
    }
}

/// The snow phase and the blocks it shares with the page.
// The snow phase and its blocks.
pub struct SnowPhase {
    pub config: Vec<f64>,
    pub view: Vec<f64>,
    pub result: Vec<f64>,
    pub camera: common::Camera,
    pub snow: Option<snow::Snow>,
}

impl SnowPhase {
    pub fn new() -> SnowPhase {
        SnowPhase {
            config: vec![0.0; layout::snow_config::STRIDE],
            view: vec![0.0; layout::view::STRIDE],
            result: vec![0.0; layout::snow_result::STRIDE],
            camera: common::Camera::new(),
            snow: None,
        }
    }

    pub fn start(&mut self) {
        self.snow = Some(snow::Snow::new(&self.config));
    }

    /// The frame's phase (Snow.update). `cast` answers a cover cell's height.
    pub fn update(&mut self, cast: &mut dyn FnMut(f64, f64, f64) -> f64) {
        self.camera.read(&self.view);
        let position = self.camera.position;
        self.snow.as_mut().expect("start first").update(&self.camera, position, cast, &mut self.result);
    }
}

impl Default for SnowPhase {
    fn default() -> Self {
        Self::new()
    }
}

/// The sky's flyers phase and the blocks it shares with the page.
// The sky's flyers phase and its blocks.
pub struct SkyPhase {
    time: f64,
    pub config: Vec<f64>,
    pub frame: Vec<f64>,
    pub result: Vec<f64>,
    pub sky: Option<sky::Sky>,
}

impl SkyPhase {
    pub fn new(classes: usize) -> SkyPhase {
        SkyPhase {
            time: 0.0,
            config: vec![0.0; layout::sky_config::STRIDE + classes * layout::sky_class::STRIDE],
            frame: vec![0.0; layout::sky_frame::STRIDE],
            result: vec![0.0; classes * layout::sky_result::STRIDE],
            sky: None,
        }
    }

    pub fn start(&mut self) {
        self.sky = Some(sky::Sky::new(&self.config));
    }

    pub fn update(&mut self) {
        if self.frame[layout::sky_frame::DT] > 0.0 { self.time += self.frame[layout::sky_frame::DT]; }
        self.sky.as_mut().expect("start first").update(&self.frame, &mut self.result);
        for row in self.result.as_chunks_mut::<{ layout::sky_result::STRIDE }>().0 { row[layout::sky_result::TIME] = self.time; }
    }
}

/// The rain phase and the blocks it shares with the page.
// The rain phase and its blocks: the drops' seeds and the sheets' texture are
// made here once.
pub struct WeatherPhase {
    pub weather: weather::Weather,
    pub frame: Vec<f64>,
    pub result: Vec<f64>,
    /// The drops' seeds (`capacity` of them), the mist's, and the sheets'
    /// streak texture.
    pub seeds: Vec<f32>,
    pub mist: Vec<f32>,
    pub streaks: Vec<u8>,
}

impl WeatherPhase {
    pub fn new(capacity: usize, enhanced: bool, range: f64, size: usize) -> WeatherPhase {
        let mut phase = WeatherPhase {
            weather: weather::Weather::new(enhanced, range, size),
            frame: vec![0.0; layout::weather_frame::STRIDE],
            result: vec![0.0; layout::weather_result::STRIDE],
            seeds: vec![0.0; capacity * 4],
            mist: vec![0.0; layout::rain::MIST * 4],
            streaks: vec![0; layout::rain::STREAK_SIZE * layout::rain::STREAK_SIZE * 4],
        };
        weather::seeds(&mut phase.seeds);
        weather::seeds(&mut phase.mist);
        weather::streaks(&mut phase.streaks);
        phase
    }

    pub fn update(&mut self, height: &mut dyn FnMut(f64, f64, f64) -> f64, cast: &mut dyn FnMut(f64, f64, f64) -> weather::Impact) {
        self.weather.update(&self.frame, height, cast, &mut self.result);
    }
}

/// The placed effects' visibility phase and the blocks it shares with the page.
// The placed effects' visibility phase and its blocks.
pub struct EnvironmentPhase {
    pub wakes: wakes::Wakes,
    time: f64,
    mode: Option<bool>,
    pub handles: Vec<f64>,
    pub config: Vec<f64>,
    pub view: Vec<f64>,
    pub frame: Vec<f64>,
    pub result: Vec<f64>,
    pub camera: common::Camera,
    pub visibility: Option<environment::Visibility>,
    count: usize,
}

impl EnvironmentPhase {
    pub fn new(count: usize) -> EnvironmentPhase {
        EnvironmentPhase {
            wakes: wakes::Wakes::new(), time: 0.0, mode: None, handles: vec![0.0; count],
            config: vec![0.0; count * layout::environment_instance::STRIDE],
            view: vec![0.0; layout::view::STRIDE],
            frame: vec![0.0; layout::environment_frame::STRIDE],
            result: vec![0.0; layout::environment_result::STRIDE + count * 4],
            camera: common::Camera::new(),
            visibility: None,
            count,
        }
    }

    pub fn start(&mut self) {
        self.visibility = Some(environment::Visibility::new(&self.config, self.count));
    }

    pub fn update(&mut self) {
        self.camera.read(&self.view);
        let dt = self.frame[layout::environment_frame::DT].clamp(0.0, 0.1);
        self.time += dt;
        let (reach, enhanced) = (v8math::max(90.,self.frame[layout::environment_frame::REACH]), self.frame[layout::environment_frame::ENHANCED] != 0.0);
        self.visibility.as_ref().expect("start first").update(&self.camera, reach, enhanced, &mut self.result);
        let changed = self.mode != Some(enhanced);
        if changed { self.wakes.clear(); }
        let visibility = self.visibility.as_ref().expect("start first");
        self.wakes.frame[layout::wake_frame::DT] = dt;
        if !enhanced { self.wakes.frame[layout::wake_frame::ACTORS] = 0.0; }
        self.wakes.update(|p| visibility.positions.iter().any(|v| {
            let (x,y,z) = (v[0]-p[0],v[1]-p[1],v[2]-p[2]); x*x+y*y+z*z < 144.0
        }));
        self.mode = Some(enhanced);
        // EnvironmentEffects.update's play/kill decisions are records. The page
        // applies them to required particle resources in authored source order.
        for i in 0..self.count {
            let classic = self.config[i*layout::environment_instance::STRIDE + layout::environment_instance::CLASSIC] != 0.0;
            let reset = changed && !classic;
            let playing = self.handles[i] != 0.0 && !reset;
            let eligible = !enhanced || classic;
            let visible = self.result[layout::environment_result::STRIDE+i] != 0.0;
            self.result[layout::environment_result::STRIDE+3*self.count+i] =
                if eligible && visible && !playing { if reset { 2.0 } else { 1.0 } }
                else if (eligible && !visible && playing) || (reset && self.handles[i] != 0.0) { -1.0 } else { 0.0 };
        }
        // FountainWater.update derives its ambient sky in the enclosing phase.
        for (i,bias) in [0.15,0.16,0.2].into_iter().enumerate() {
            self.result[layout::environment_result::SKY+i]=v8math::min(1.,self.frame[layout::environment_frame::AMBIENT_TOP+i]*1.4+bias);
        }
        self.result[layout::environment_result::TIME] = self.time;
        self.result[layout::environment_result::DT] = dt;
        self.result[layout::environment_result::MODE_CHANGED] = f64::from(changed);
        self.result[layout::environment_result::ENHANCED]=f64::from(enhanced);
    }
}

#[cfg(target_arch = "wasm32")]
// The module's flat interface. One instance of the module holds one owner's
// state, so each page owner makes its own instance and its memory is its own;
// nothing is allocated after it is built.
mod exports {
    //! The module's flat interface: one instance holds one environment.
    use super::*;
    use std::ptr::addr_of_mut;

    // One static a piece of state: a WebAssembly instance is single-threaded and
    // the page never re-enters a call.
    static mut SURFACES: Option<surfaces::Phase> = None;
    static mut PUBLICATION: Option<publication::Publication> = None;
    fn publication()->&'static mut publication::Publication{unsafe{(*addr_of_mut!(PUBLICATION)).get_or_insert_with(publication::Publication::new)}}
    #[no_mangle]pub extern "C" fn page_values_ptr(kind:u32,index:u32)->*mut f64{publication().values(kind,index as usize).as_mut_ptr()}
    #[no_mangle]pub extern "C" fn page_frame_ptr()->*mut f64{publication().input.as_mut_ptr()}
    #[no_mangle]pub extern "C" fn page_weather_mode_ptr()->*const f64{publication().weather_mode.as_ptr()}
    #[no_mangle]pub extern "C" fn page_sky_ptr()->*const f64{publication().sky.as_ptr().cast()}
    #[no_mangle]pub extern "C" fn page_fountain_models()->u32{let Ok(data)=content::input()else{return 1;};publication().fountain_models(&data);content::output(&serde_json::Value::Null);0}
    #[no_mangle]pub extern "C" fn page_fountain_models_ptr()->*const f64{publication().fountain_models.as_ptr().cast()}
    #[no_mangle]pub extern "C" fn page_fountain_mode(enabled:u32)->u32{u32::from(publication().fountain_mode(enabled!=0))}
    #[no_mangle]pub extern "C" fn page_sky_motion(frame:f64)->f64{publication().sky_motion(frame)}
    #[no_mangle]pub extern "C" fn page_weather_mode(enhanced:u32){
        publication().mode(enhanced!=0);let m=publication().weather_mode;rain().weather.set_mode(enhanced!=0,m[layout::weather_mode::RANGE],m[layout::weather_mode::SIZE] as usize);
    }
    #[no_mangle]pub extern "C" fn water_draw_prepare(slot:u32){water_draw::exports::prepare(slot as usize,&surfaces().result)}
    #[no_mangle]pub extern "C" fn page_admit(kind:u32)->u32{
        let Ok(data)=content::input()else{return 1;};
        let mut result=serde_json::Value::Null;
        use layout::environment_admission as admit;
        unsafe{if kind==admit::ATMOSPHERE as u32{let(p,r)=admission::atmosphere(&data);publication().atmosphere(&r);*addr_of_mut!(ATMOSPHERE)=Some(p);result=r;}
        else if kind==admit::SNOW as u32{let(p,r)=admission::snow(&data);let cell=p.snow.as_ref().unwrap().cover.cell;publication().snow(&r,cell);*addr_of_mut!(SNOW)=Some(p);result=r;}
        else if kind==admit::WEATHER as u32{let enhanced=data["enhanced"].as_bool().unwrap_or(false);let r=publication().weather(&data["data"],enhanced);let m=publication().weather_mode;*addr_of_mut!(WEATHER)=Some(WeatherPhase::new(m[layout::weather_mode::CAPACITY] as usize,enhanced,m[layout::weather_mode::RANGE],m[layout::weather_mode::SIZE] as usize));result=r;}
        else if kind==admit::SKY as u32{let(p,r)=admission::sky(&data);publication().sky=vec![[0.;layout::sky_publication::STRIDE];p.result.len()/layout::sky_result::STRIDE];for(k,row)in publication().sky.iter_mut().enumerate(){let count=p.config[layout::sky_config::STRIDE+k*layout::sky_class::STRIDE+layout::sky_class::COUNT];row[layout::sky_publication::SOLID_COUNT]=count;row[layout::sky_publication::FADING_COUNT]=count;}*addr_of_mut!(SKY)=Some(p);result=r;}
        else if kind==admit::SOURCES as u32{let(p,r)=admission::sources(&data);*addr_of_mut!(ENVIRONMENT)=Some(p);result=r;}
        else if kind==admit::FOUNTAIN as u32{result=publication().fountain(&data);}
        else if kind==admit::WATER_MODE as u32{result=water_draw::exports::mode(&data);}
        else if kind==admit::SHORE as u32{let quality=match data["quality"].as_str(){Some("ultra")=>2.,Some("high")=>1.,_=>0.};surfaces().frame[layout::surface_frame::SHORE_QUALITY]=quality;result=serde_json::json!({"visible":quality>0.});}}
        content::output(&result);0
    }
    static mut SHORE_INPUT: Vec<f64> = Vec::new();
    static mut SHORE_QUERY:[f64;layout::shore_query::STRIDE]=[0.;layout::shore_query::STRIDE];

    #[link(wasm_import_module = "env")]
    extern "C" {
        fn host_water_over(x:f64,z:f64)->u32;
        fn host_water_level()->f64;
        fn host_shore_tile(x: f64, z: f64, height: f64) -> u32;
        fn host_shore_exposed(x:f64,y:f64,z:f64,far:f64)->f64;
    }
    #[no_mangle]
    pub extern "C" fn surface_shore_query_ptr()->*const f64{addr_of_mut!(SHORE_QUERY).cast::<f64>()}
    struct ShoreQueries;
    impl shore::Queries for ShoreQueries {
        fn triangles(&mut self, x: f64, z: f64, height: f64, output: &mut [f64]) -> usize {
            // SAFETY: the callback writes only SHORE_INPUT, a disjoint input
            // block. No phase state is borrowed by or exposed to the host.
            // WaterShore.collect asks collision for native tile bounds. This
            // block is disjoint from the borrowed phase, just like triangles.
            let tx=x*8.;let tz=z*8.;
            unsafe{let row=&mut *addr_of_mut!(SHORE_QUERY);row[layout::shore_query::CENTER..layout::shore_query::CENTER+3].copy_from_slice(&[tx+4.,height,tz+4.]);row[layout::shore_query::RADIUS]=4.;row[layout::shore_query::BOUNDS..layout::shore_query::BOUNDS+6].copy_from_slice(&[tx,height-0.1,tz,tx+8.,height+0.1,tz+8.]);}
            let count = unsafe { host_shore_tile(x, z, height) } as usize;
            let n = count.min(layout::surface::SHORE_TRIANGLES) * layout::shore_triangle::STRIDE;
            unsafe { output[..n].copy_from_slice(&(&*addr_of_mut!(SHORE_INPUT))[..n]); }
            count
        }
        fn exposed(&mut self, x: f64, z: f64, height: f64) -> bool {
            // SAFETY: a scalar collision query with no phase re-entry.
            unsafe {host_water_over(x,z)!=0&&host_shore_exposed(x,height+12.,z,30.)<height-0.08}
        }
    }
    fn surfaces() -> &'static mut surfaces::Phase {
        // SAFETY: one single-threaded environment owner per module instance.
        unsafe { (*addr_of_mut!(SURFACES)).as_mut().expect("surface_new first") }
    }
    #[no_mangle]
    pub extern "C" fn surface_new(water: u32) {
        // SAFETY: called once during owner construction, before publication.
        unsafe { let mut phase = surfaces::Phase::new(water != 0); phase.prepare_math::<surface_math::Page>();
            *addr_of_mut!(SURFACES) = Some(phase);publication();water_draw::exports::water_draw_input();
            *addr_of_mut!(SHORE_INPUT) = vec![0.0; layout::surface::SHORE_TRIANGLES * layout::shore_triangle::STRIDE]; }
    }
    #[no_mangle]
    pub extern "C" fn surface_config_ptr() -> *mut f64 { surfaces().config.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn surface_frame_ptr() -> *mut f64 { surfaces().frame.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn surface_commands_ptr() -> *mut f64 { surfaces().commands.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn surface_result_ptr() -> *const f64 { surfaces().result.as_ptr() }
    #[no_mangle]
    pub extern "C" fn surface_ring_ptr(kind: u32) -> *const f32 {
        let s = surfaces(); match kind { 0 => s.positions.as_ptr(), 1 => s.uvs.as_ptr(), _ => s.alpha.as_ptr() }
    }
    #[no_mangle]
    pub extern "C" fn surface_storm_ptr(kind: u32) -> *const f32 {
        let storm = &surfaces().storm;
        match kind { 0 => storm.positions.as_ptr(), 1 => storm.tangents.as_ptr(), 2 => storm.edges.as_ptr(), 3 => storm.widths.as_ptr(), _ => storm.energies.as_ptr() }
    }
    #[no_mangle]
    pub extern "C" fn surface_shore_ptr(kind: u32) -> *const f32 {
        if kind == 0 { surfaces().shore.points.as_ptr() } else { surfaces().shore.phases.as_ptr() }
    }
    #[no_mangle]
    pub extern "C" fn surface_triangles_ptr() -> *mut f64 {
        // SAFETY: this separate block is written only during host_shore_tile.
        unsafe { (&mut *addr_of_mut!(SHORE_INPUT)).as_mut_ptr() }
    }
    fn surface_update() { surfaces().update::<surface_math::Page, _>(&mut ShoreQueries); }

    /// The complete world-surface state phase, independent of renderer passes.
    /// All installed owners consume their already written blocks in page order.
    #[no_mangle]
    pub extern "C" fn surface_frame() {
        surface_update();publication().shore_frame(surfaces());
        // SAFETY: the host is single-threaded; these tests borrow no owner
        // across the next call. Missing map systems simply have no instance.
        unsafe {
            if (*addr_of_mut!(WEATHER)).is_some() { weather_update(); publication().weather_frame(&rain().result); }
            if (*addr_of_mut!(SNOW)).is_some() { snow_update(); publication().snow_frame(&snow().result); }
            if (*addr_of_mut!(ATMOSPHERE)).is_some() { atm_update(); publication().atmosphere_frame(&atmosphere().result); }
            if (*addr_of_mut!(SKY)).is_some() { sky_update(); }
            if (*addr_of_mut!(ENVIRONMENT)).is_some() { vis_update(); publication().fountain_frame(placed()); }
        }
    }

    static mut STATE: Option<Destruction> = None;

    fn state() -> &'static mut Destruction {
        // SAFETY: WebAssembly instances are single-threaded and the page
        // never re-enters a call.
        unsafe { (*addr_of_mut!(STATE)).as_mut().expect("env_new first") }
    }

    /// The destruction effects: the pools of hot and smoke particles and the
    /// arcs, with the random stream seeded.
    #[no_mangle]
    pub extern "C" fn env_new(hot: u32, smoke: u32, arcs: u32, seed: u32) {
        // SAFETY: as in `state`.
        unsafe { *addr_of_mut!(STATE) = Some(Destruction::new(Fx::new(hot as usize, smoke as usize, arcs as usize), seed)) };
    }
    #[no_mangle]
    pub extern "C" fn env_fx_result_ptr() -> *const f64 { state().fx_result.as_ptr() }
    #[no_mangle]
    pub extern "C" fn env_fx_result_len() -> u32 { state().fx_result.len() as u32 }
    /// A pool's attribute: pool 0 the hot one, 1 the smoke.
    #[no_mangle]
    pub extern "C" fn env_pool_ptr(pool: u32, attribute: u32) -> *const f32 {
        let fx = &state().fx;
        let pool = if pool == 0 { &fx.hot } else { &fx.smoke };
        pool.attributes[attribute as usize].as_ptr()
    }
    /// The arcs' positions (0), tangents (1) and meta (2) as f32.
    #[no_mangle]
    pub extern "C" fn env_arc_ptr(kind: u32) -> *const f32 {
        let arcs = &state().fx.arcs;
        match kind { 0 => arcs.positions.as_ptr(), 1 => arcs.tangents.as_ptr(), _ => arcs.meta.as_ptr() }
    }
    #[no_mangle]
    pub extern "C" fn env_arc_indices_ptr() -> *const u16 { state().fx.arcs.indices.as_ptr() }
    /// The arcs' vertex capacity (positions are three floats each, meta four).
    #[no_mangle]
    pub extern "C" fn env_arc_vertices() -> u32 { (state().fx.arcs.positions.len() / 3) as u32 }
    #[no_mangle]
    pub extern "C" fn env_arc_indices_len() -> u32 { state().fx.arcs.indices.len() as u32 }

    // The destruction phase's blocks, by name: the page writes the first
    // group and reads the result.
    #[no_mangle]
    pub extern "C" fn dst_frame_ptr() -> *mut f64 { state().frame.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn dst_commands_ptr() -> *mut f64 { state().commands.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn dst_chunks_ptr() -> *mut f64 { state().chunks.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn dst_matrices_ptr() -> *mut f64 { state().matrices.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn dst_station_config_ptr() -> *mut f64 { state().station_config.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn dst_station_state_ptr() -> *mut f64 { state().station_state.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn dst_slot_static_ptr() -> *mut f64 { state().slot_static.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn dst_slot_state_ptr() -> *mut f64 { state().slot_state.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn dst_slot_matrices_ptr() -> *mut f64 { state().slot_matrices.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn dst_burst_ptr() -> *mut f64 { state().burst_blocks.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn dst_result_ptr() -> *const f64 { state().result.as_ptr() }
    #[no_mangle]
    pub extern "C" fn dst_result_len() -> u32 { state().result.len() as u32 }
    #[no_mangle]
    pub extern "C" fn dst_points_ptr() -> *mut f64 { state().shape_points.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn dst_watch(count: u32) { state().watch(count as usize) }
    /// A model's shape: its box centre and half size, its reach, its inverse
    /// inertia and its `n` extreme points (written to the points block).
    #[no_mangle]
    // A model's shape: its box centre and half size, its reach, its inverse inertia
    // and its extreme points (written to the points block first).
    pub extern "C" fn dst_shape(handle: u32, cx: f64, cy: f64, cz: f64, hx: f64, hy: f64, hz: f64, radius: f64, ix: f64, iy: f64, iz: f64, n: u32) {
        state().register_shape(handle as usize, [cx, cy, cz], [hx, hy, hz], radius, [ix, iy, iz], n as usize)
    }
    #[no_mangle]
    pub extern "C" fn dst_prepare() { state().prepare() }
    /// Runs the commands written so far, without a frame (see Destruction::apply).
    #[no_mangle]
    pub extern "C" fn dst_apply() {
        // SAFETY: the host imports are pure queries of the page's collision world.
        state().apply(&mut |origin, direction, far| unsafe {
            if host_cast(origin.x, origin.y, origin.z, direction.x, direction.y, direction.z, far) == 0.0 {
                None
            } else {
                Some(solver::Hit {
                    point: three::Vector3::new(host_cast_out(0), host_cast_out(1), host_cast_out(2)),
                    normal: three::Vector3::new(host_cast_out(3), host_cast_out(4), host_cast_out(5)),
                })
            }
        })
    }
    #[no_mangle]
    // The frame. The collision world is asked through two host imports (a cast and
    // its result), pure queries of the page.
    pub extern "C" fn dst_update() {
        // SAFETY: the host imports are pure queries of the page's collision world.
        state().update(&mut |origin, direction, far| unsafe {
            if host_cast(origin.x, origin.y, origin.z, direction.x, direction.y, direction.z, far) == 0.0 {
                None
            } else {
                Some(solver::Hit {
                    point: three::Vector3::new(host_cast_out(0), host_cast_out(1), host_cast_out(2)),
                    normal: three::Vector3::new(host_cast_out(3), host_cast_out(4), host_cast_out(5)),
                })
            }
        })
    }

    #[link(wasm_import_module = "env")]
    extern "C" {
        /// The page's terrain height under x, z (NaN for none).
        fn host_ground(x: f64, z: f64) -> f64;
        /// The page's rain cover for a cell: the height of what a cast
        /// straight down from (x, y, z) first hits, or of open water over it.
        fn host_cover(x: f64, y: f64, z: f64) -> f64;
        /// A splash's cast from (x, y, z): the height it ends at (-infinity
        /// for nothing); `host_impact_normal` then gives the normal (0..2)
        /// and whether it is water (3).
        fn host_impact(x: f64, y: f64, z: f64) -> f64;
        fn host_impact_normal(i: u32) -> f64;
        /// A cast of the page's collision world from (ox, oy, oz) along (dx,
        /// dy, dz) for `far` metres: 1 for a hit, 0 for none;
        /// `host_cast_out` then gives the point (0..2) and the normal (3..5).
        fn host_cast(ox: f64, oy: f64, oz: f64, dx: f64, dy: f64, dz: f64, far: f64) -> f64;
        fn host_cast_out(i: u32) -> f64;
    }

    static mut ATMOSPHERE: Option<AtmospherePhase> = None;

    fn atmosphere() -> &'static mut AtmospherePhase {
        // SAFETY: as in `state`.
        unsafe { (*addr_of_mut!(ATMOSPHERE)).as_mut().expect("atm_new first") }
    }

    #[no_mangle]
    pub extern "C" fn atm_new(classes: u32, instances: u32, fog: u32) {
        // SAFETY: as in `state`.
        unsafe { *addr_of_mut!(ATMOSPHERE) = Some(AtmospherePhase::new(classes as usize, instances as usize, fog != 0)) };
    }
    #[no_mangle]
    pub extern "C" fn atm_config_ptr() -> *mut f64 { atmosphere().config.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn atm_config_len() -> u32 { atmosphere().config.len() as u32 }
    #[no_mangle]
    pub extern "C" fn atm_instances_ptr() -> *mut f64 { atmosphere().instances.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn atm_instances_len() -> u32 { atmosphere().instances.len() as u32 }
    #[no_mangle]
    pub extern "C" fn atm_view_ptr() -> *mut f64 { atmosphere().view.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn atm_result_ptr() -> *const f64 { atmosphere().result.as_ptr() }
    #[no_mangle]
    pub extern "C" fn atm_result_len() -> u32 { atmosphere().result.len() as u32 }
    #[no_mangle]
    pub extern "C" fn atm_start() { atmosphere().start() }
    /// A batch's squares: center (0, four floats each) or spin (1, two).
    #[no_mangle]
    pub extern "C" fn atm_batch_ptr(batch: u32, kind: u32) -> *const f32 {
        let batch = &atmosphere().atmosphere.as_ref().expect("atm_start first").batches[batch as usize];
        if kind == 0 { batch.center.as_ptr() } else { batch.spin.as_ptr() }
    }
    fn atm_update() {
        // SAFETY: the host import is a pure query of the page's terrain.
        atmosphere().update(&mut |x, z| unsafe { host_ground(x, z) });
        for row in atmosphere().result[layout::atmosphere_stats::STRIDE..].chunks_exact_mut(layout::square_batch::STRIDE){row[layout::square_batch::VISIBLE]=f64::from(row[layout::square_batch::COUNT]>0.);}
    }

    static mut SNOW: Option<SnowPhase> = None;

    fn snow() -> &'static mut SnowPhase {
        // SAFETY: as in `state`.
        unsafe { (*addr_of_mut!(SNOW)).as_mut().expect("snow_new first") }
    }

    #[no_mangle]
    pub extern "C" fn snow_new() {
        // SAFETY: as in `state`.
        unsafe { *addr_of_mut!(SNOW) = Some(SnowPhase::new()) };
    }
    #[no_mangle]
    pub extern "C" fn snow_config_ptr() -> *mut f64 { snow().config.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn snow_view_ptr() -> *mut f64 { snow().view.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn snow_result_ptr() -> *const f64 { snow().result.as_ptr() }
    #[no_mangle]
    pub extern "C" fn snow_start() { snow().start() }
    /// The snow's blocks, once started: the flakes' texels (0), the boxes (1)
    /// and the cover's heights (2), as f32; and their lengths.
    #[no_mangle]
    pub extern "C" fn snow_data_ptr(kind: u32) -> *const f32 {
        let s = snow().snow.as_ref().expect("snow_start first");
        match kind { 0 => s.flake_data.as_ptr(), 1 => s.box_data.as_ptr(), _ => s.cover.heights.as_ptr() }
    }
    #[no_mangle]
    pub extern "C" fn snow_data_len(kind: u32) -> u32 {
        let s = snow().snow.as_ref().expect("snow_start first");
        (match kind { 0 => s.flake_data.len(), 1 => s.box_data.len(), _ => s.cover.heights.len() }) as u32
    }
    #[no_mangle]
    pub extern "C" fn snow_cover_cell() -> f64 { snow().snow.as_ref().expect("snow_start first").cover.cell }
    fn snow_update() {
        // SAFETY: the host import is a pure query of the page's collision world.
        snow().update(&mut |x, y, z| unsafe { let roof=host_cover(x,y,z);if host_water_over(x,z)!=0{v8math::max(roof,host_water_level())}else{roof} })
    }

    static mut SKY: Option<SkyPhase> = None;

    fn sky_phase() -> &'static mut SkyPhase {
        // SAFETY: as in `state`.
        unsafe { (*addr_of_mut!(SKY)).as_mut().expect("sky_new first") }
    }

    #[no_mangle]
    pub extern "C" fn sky_new(classes: u32) {
        // SAFETY: as in `state`.
        unsafe { *addr_of_mut!(SKY) = Some(SkyPhase::new(classes as usize)) };
    }
    #[no_mangle]
    pub extern "C" fn sky_config_ptr() -> *mut f64 { sky_phase().config.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn sky_config_len() -> u32 { sky_phase().config.len() as u32 }
    #[no_mangle]
    pub extern "C" fn sky_frame_ptr() -> *mut f64 { sky_phase().frame.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn sky_result_ptr() -> *const f64 { sky_phase().result.as_ptr() }
    #[no_mangle]
    pub extern "C" fn sky_result_len() -> u32 { sky_phase().result.len() as u32 }
    #[no_mangle]
    pub extern "C" fn sky_start() { sky_phase().start() }
    #[no_mangle]
    pub extern "C" fn sky_set_enhanced(on:u32)->u32{let sky=sky_phase().sky.as_mut().expect("sky_start first");let changed=sky.enhanced()!=(on!=0);sky.set_enhanced(on!=0);u32::from(changed)}
    #[no_mangle]pub extern "C" fn sky_probe_needed()->u32{sky_phase().sky.as_ref().map_or(0,|s|u32::from(s.probe_needed()))}
    #[no_mangle]
    pub extern "C" fn sky_update() { sky_phase().update();publication().sky_frame(sky_phase()); }
    #[no_mangle]
    pub extern "C" fn sky_count()->u32 {sky_phase().sky.as_ref().map_or(0,|s|s.count() as u32)}
    #[no_mangle]
    pub extern "C" fn sky_active(index:u32)->u32 {sky_phase().sky.as_ref().map_or(0,|s|u32::from(s.active(index as usize)))}
    /// A class's records, by the order the page names them: solid matrices and
    /// looks, fading ones, motion's matrices, previous matrices and shown flags.
    #[no_mangle]
    pub extern "C" fn sky_array_ptr(class: u32, which: u32) -> *const f32 { sky_phase().sky.as_ref().expect("sky_start first").array(class as usize, which as usize).as_ptr() }
    #[no_mangle]
    pub extern "C" fn sky_array_len(class: u32, which: u32) -> u32 { sky_phase().sky.as_ref().expect("sky_start first").array(class as usize, which as usize).len() as u32 }

    static mut WEATHER: Option<WeatherPhase> = None;

    fn rain() -> &'static mut WeatherPhase {
        // SAFETY: as in `state`.
        unsafe { (*addr_of_mut!(WEATHER)).as_mut().expect("weather_new first") }
    }

    #[no_mangle]
    pub extern "C" fn weather_new(capacity: u32, enhanced: u32, range: f64, size: u32) {
        // SAFETY: as in `state`.
        unsafe { *addr_of_mut!(WEATHER) = Some(WeatherPhase::new(capacity as usize, enhanced != 0, range, size as usize)) };
    }
    #[no_mangle]
    pub extern "C" fn weather_mode(enhanced: u32, range: f64, size: u32) { rain().weather.set_mode(enhanced != 0, range, size as usize) }
    #[no_mangle]
    pub extern "C" fn weather_frame_ptr() -> *mut f64 { rain().frame.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn weather_result_ptr() -> *const f64 { rain().result.as_ptr() }
    /// What the shaders and the renderer read, by kind: the cover's heights
    /// (0), the splashes' locations (1) and normals (2), the drops' seeds (3),
    /// the mist's (4), as f32; and their lengths.
    #[no_mangle]
    pub extern "C" fn weather_data_ptr(kind: u32) -> *const f32 {
        let r = rain();
        match kind { 0 => r.weather.cover.heights.as_ptr(), 1 => r.weather.impacts.locations.as_ptr(), 2 => r.weather.impacts.normals.as_ptr(), 3 => r.seeds.as_ptr(), _ => r.mist.as_ptr() }
    }
    #[no_mangle]
    pub extern "C" fn weather_data_len(kind: u32) -> u32 {
        let r = rain();
        (match kind { 0 => r.weather.cover.cells(), 1 => r.weather.impacts.locations.len(), 2 => r.weather.impacts.normals.len(), 3 => r.seeds.len(), _ => r.mist.len() }) as u32
    }
    #[no_mangle]
    pub extern "C" fn weather_streaks_ptr() -> *const u8 { rain().streaks.as_ptr() }
    #[no_mangle]
    pub extern "C" fn weather_streaks_len() -> u32 { rain().streaks.len() as u32 }
    #[no_mangle]
    pub extern "C" fn weather_cover_cell() -> f64 { rain().weather.cover.cell }
    fn weather_update() {
        // SAFETY: the host imports are pure queries of the page's collision world.
        rain().update(
            &mut |x, y, z| unsafe { let roof=host_cover(x,y,z);if host_water_over(x,z)!=0{v8math::max(roof,host_water_level())}else{roof} },
            &mut |x, y, z| unsafe {
                let roof=host_impact(x,y,z);let level=if host_water_over(x,z)!=0{host_water_level()}else{f64::NEG_INFINITY};let water=level>roof;
                weather::Impact {y:if water{level}else{roof},normal:[host_impact_normal(0),host_impact_normal(1),host_impact_normal(2)],water}
            },
        )
    }

    static mut ENVIRONMENT: Option<EnvironmentPhase> = None;

    fn placed() -> &'static mut EnvironmentPhase {
        // SAFETY: as in `state`.
        unsafe { (*addr_of_mut!(ENVIRONMENT)).as_mut().expect("vis_new first") }
    }

    #[no_mangle]
    pub extern "C" fn vis_new(count: u32) {
        // SAFETY: as in `state`.
        unsafe { *addr_of_mut!(ENVIRONMENT) = Some(EnvironmentPhase::new(count as usize)) };
    }
    #[no_mangle]
    pub extern "C" fn vis_config_ptr() -> *mut f64 { placed().config.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn vis_config_len() -> u32 { placed().config.len() as u32 }
    #[no_mangle]
    pub extern "C" fn vis_view_ptr() -> *mut f64 { placed().view.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn vis_frame_ptr() -> *mut f64 { placed().frame.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn vis_result_ptr() -> *const f64 { placed().result.as_ptr() }
    #[no_mangle]
    pub extern "C" fn vis_result_len() -> u32 { placed().result.len() as u32 }
    #[no_mangle]
    pub extern "C" fn vis_handles_ptr() -> *mut f64 { placed().handles.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn vis_wake_frame_ptr() -> *mut f64 { placed().wakes.frame.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn vis_wake_actors_ptr() -> *mut f64 { placed().wakes.actors.as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn vis_wake_result_ptr() -> *const f64 { placed().wakes.result.as_ptr() }
    #[no_mangle]
    pub extern "C" fn vis_wake_points_ptr() -> *const f64 { placed().wakes.points.as_ptr() }
    #[no_mangle]
    pub extern "C" fn vis_wake_motion_ptr() -> *const f64 { placed().wakes.motion.as_ptr() }
    #[no_mangle]
    pub extern "C" fn vis_wake_apply() { placed().wakes.apply(); }
    #[no_mangle]
    pub extern "C" fn vis_start() { placed().start() }
    fn vis_update() { placed().update() }
}
