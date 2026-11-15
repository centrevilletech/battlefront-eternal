//! The effects the map's placed objects carry (environment-effects.js):
//! torches, campfires, smoke, mist, waterspouts, steam vents and fountains.
//! Each plays its stock effect while it is in view, out to the near scene's
//! end, and is killed when it leaves; on entering view it is stepped ahead
//! by up to its particles' lifetime, so a torch is already burning.
//! The browser's Enhanced forms of fountains and steam vents (water and
//! volume shaders) are not drawn natively; those play their stock effect in
//! both modes.
use super::effects::Spawner;
use bfe_content_native::BattleContent;

struct Instance {
    group: usize,
    position: [f64; 3],
    turn: [f64; 4],
    // The sphere tested against the view: smoke and mist rise well above
    // where they start.
    center: [f64; 3],
    handle: Option<u32>,
    // Seconds stepped at once when it comes into view.
    warmup: f64,
    // A steam vent's Enhanced volume (environment-effects.js), none for the
    // effects that have no Enhanced form.
    volume: Option<super::steam::Volume>,
    // A fountain's slot and pool levels, for the Enhanced jets.
    fountain: Option<super::fountains::Fountain>,
}

pub struct MapEffects {
    instances: Vec<Instance>,
    reach: f64,
    // The last frame's projection then view matrix, and its eye.
    camera: Option<([f64; 32], [f64; 3])>,
    /// The environmentEffects setting: Enhanced draws the vents' steam as
    /// volumes (steam.rs) instead of their stock effect.
    pub enhanced: bool,
    /// Whether the Enhanced fountains can be drawn here (fountains.rs).
    pub jets: bool,
    /// The vents in view this frame, for the Enhanced steam.
    pub volumes: Vec<super::steam::Volume>,
    /// The fountains in view this frame, for the Enhanced jets.
    pub fountains: Vec<super::fountains::Fountain>,
    /// Map effects started so far, and those in view in the last update.
    pub started: u64,
    pub visible: usize,
    // BFE_BENCH fx=name,name,...: each named effect group in turn, every 3 s,
    // 6 m ahead of the camera and 2.5 m right, for side-by-side captures with the browser.
    show: Vec<(String, usize)>,
    show_next: usize,
    show_clock: f64,
    show_handle: Option<u32>,
    // With the bench's fx key present (even empty), a count line every 10 s.
    pub report: Option<f64>,
}

impl MapEffects {
    pub fn new(content: &BattleContent, names: &[(String, usize)], warmups: &[f64]) -> Self {
        let mut instances = Vec::new();
        for item in &content.environment_instances {
            let Some(&(_, group)) = names.iter().find(|(n, _)| *n == item.effect) else {
                continue;
            };
            let mut matrix = three::Matrix4::default();
            matrix.from_array(&item.matrix, 0);
            let (mut position, mut turn, mut scale) = (three::Vector3::default(), three::Quaternion::default(), three::Vector3::default());
            matrix.decompose(&mut position, &mut turn, &mut scale);
            let position = [position.x, position.y, position.z];
            let volume = super::steam::is_steam(&item.effect).then(|| {
                let pipe = item.effect.ends_with("/prop_steampipe");
                let mut direction = if pipe { three::Vector3::new(0., 0., 1.) } else { three::Vector3::new(0., 1., 0.) };
                direction.apply_quaternion(&turn);
                let big = !pipe && ["hp_steam_1", "hp_steam_5"].contains(&item.hardpoint.as_str());
                super::steam::Volume {
                    position,
                    direction: [direction.x, direction.y, direction.z],
                    extent: if pipe { [2.5, 4., 2.5] } else if big { [7., 6., 7.] } else { [6., 5.5, 6.] },
                    kind: pipe as u8 as f64,
                }
            });
            let fountain = item.effect.ends_with("/waterfountain").then(|| {
                let slot = content.environment_instances.iter().take_while(|other| !std::ptr::eq(*other, item)).filter(|other| other.effect.ends_with("/waterfountain")).count();
                let (level, low) = super::fountains::levels(content, position);
                super::fountains::Fountain { slot, position, level, low }
            });
            instances.push(Instance {
                fountain,
                volume,
                group,
                position,
                turn: [turn.x, turn.y, turn.z, turn.w],
                center: [position[0], position[1] + 8., position[2]],
                handle: None,
                warmup: warmups.get(group).copied().unwrap_or(0.),
            });
        }
        // `Math.max(90, lighting.fade.value.y)`: the near scene's end.
        let range = &content.map["distance"]["nearScene"];
        let end = range[3].as_f64().filter(|v| v.is_finite() && *v > 0.3).unwrap_or(500.);
        let wanted = crate::app::bench::fx();
        let show = wanted.iter().flatten().filter_map(|name| names.iter().find(|(n, _)| n == name).cloned()).collect();
        if wanted.is_some() {
            eprintln!("fx-map placed={} of {} reach={}", instances.len(), content.environment_instances.len(), end.max(90.));
        }
        Self { instances, reach: end.max(90.), camera: None, enhanced: true, jets: false, volumes: Vec::with_capacity(32), fountains: Vec::with_capacity(8), started: 0, visible: 0, show, show_next: 0, show_clock: 0., show_handle: None, report: wanted.map(|_| 0.) }
    }

    /// The frame's camera, once it is chosen: the next update tests
    /// visibility against it.
    pub fn view(&mut self, camera: &[f64], eye: [f64; 3]) {
        if (self.instances.is_empty() && self.show.is_empty()) || camera.len() < 32 {
            return;
        }
        self.camera = Some((std::array::from_fn(|i| camera[i]), eye));
    }

    /// Start the effects that came into view and kill the ones that left.
    pub fn update(&mut self, effects: &mut Spawner, dt: f64) {
        let Some((camera, eye)) = self.camera else {
            return;
        };
        if !self.show.is_empty() {
            self.show_clock += dt;
            // Each stops emitting after 1.3 s, so a lasting one (a burning
            // hull's flame) does not run on into the next.
            if self.show_clock >= 1.3 {
                if let Some(handle) = self.show_handle.take() {
                    effects.stop(handle);
                }
            }
            if self.show_clock >= 3. {
                self.show_clock = 0.;
                let (name, group) = &self.show[self.show_next % self.show.len()];
                self.show_next += 1;
                // The camera's forward (its view's third row, negated), level.
                let (x, z) = (-camera[16 + 2], -camera[16 + 10]);
                let length = x.hypot(z).max(1e-6);
                // 6 m ahead, 2.5 m right of the view (clear of the player's
                // own body), 1.5 m below the eye.
                let (fx, fz) = (x / length, z / length);
                let at = [eye[0] + fx * 6. - fz * 2.5, eye[1] - 1.5, eye[2] + fz * 6. + fx * 2.5];
                self.show_handle = effects.start(*group, at, [0.; 3], None, self.show_next as u32);
                let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |t| t.as_millis());
                eprintln!("fx-show {name} {ms} {:.2} {:.2} {:.2}", at[0], at[1], at[2]);
            }
        }
        let planes = frustum(&camera);
        let reach = self.reach * self.reach;
        self.visible = 0;
        self.volumes.clear();
        self.fountains.clear();
        for (at, i) in self.instances.iter_mut().enumerate() {
            let far = (0..3).map(|k| (i.position[k] - eye[k]).powi(2)).sum::<f64>();
            // Enhanced vents: a volume while in view out to 90 m, the sphere
            // 2 m along the vent and 7 m round; their stock effect stops.
            // Enhanced fountains: jets while in view out to 90 m (the sphere
            // 2 m above the nozzle, 9 m round).
            if let (true, Some(f)) = (self.enhanced && self.jets, i.fountain) {
                if let Some(handle) = i.handle.take() {
                    effects.kill(handle);
                }
                let center = [f.position[0], f.position[1] + 2., f.position[2]];
                if far < 90. * 90. && planes.iter().all(|p| p[0] * center[0] + p[1] * center[1] + p[2] * center[2] + p[3] >= -9.) {
                    self.visible += 1;
                    self.fountains.push(f);
                }
                continue;
            }
            if let (true, Some(v)) = (self.enhanced, i.volume) {
                if let Some(handle) = i.handle.take() {
                    effects.kill(handle);
                }
                let center: [f64; 3] = std::array::from_fn(|k| v.position[k] + v.direction[k] * 2.);
                if far < 90. * 90. && planes.iter().all(|p| p[0] * center[0] + p[1] * center[1] + p[2] * center[2] + p[3] >= -7.) {
                    self.visible += 1;
                    self.volumes.push(v);
                }
                continue;
            }
            let visible = far < reach && planes.iter().all(|p| p[0] * i.center[0] + p[1] * i.center[1] + p[2] * i.center[2] + p[3] >= -16.);
            self.visible += visible as usize;
            match (visible, i.handle) {
                (true, None) => {
                    i.handle = effects.start_turned(i.group, i.position, [0.; 3], i.turn, at as u32 + 1);
                    if let Some(handle) = i.handle {
                        effects.prewarm(handle, i.warmup);
                        self.started += 1;
                    }
                }
                (false, Some(handle)) => {
                    effects.kill(handle);
                    i.handle = None;
                }
                _ => {}
            }
        }
    }

    /// A new battle: each starts again when next in view.
    pub fn reset(&mut self, effects: &mut Spawner) {
        for i in &mut self.instances {
            if let Some(handle) = i.handle.take() {
                effects.kill(handle);
            }
        }
    }
}

// The six planes of a projection-then-view pair, normalised, as three's
// Frustum.setFromProjectionMatrix builds them from their product.
fn frustum(camera: &[f64; 32]) -> [[f64; 4]; 6] {
    let (p, v) = (&camera[..16], &camera[16..]);
    let mut m = [0.; 16];
    for c in 0..4 {
        for r in 0..4 {
            m[c * 4 + r] = (0..4).map(|k| p[k * 4 + r] * v[c * 4 + k]).sum();
        }
    }
    let row = |r: usize| [m[r], m[4 + r], m[8 + r], m[12 + r]];
    let (x, y, z, w) = (row(0), row(1), row(2), row(3));
    let mut planes = [[0.; 4]; 6];
    for (k, sign, axis) in [(0, -1., x), (1, 1., x), (2, 1., y), (3, -1., y), (4, -1., z), (5, 1., z)] {
        let plane: [f64; 4] = std::array::from_fn(|i| w[i] + sign * axis[i]);
        let length = (plane[0] * plane[0] + plane[1] * plane[1] + plane[2] * plane[2]).sqrt().max(1e-12);
        planes[k] = plane.map(|v| v / length);
    }
    planes
}

#[cfg(test)]
mod tests {
    #[test]
    fn frustum_keeps_what_is_ahead() {
        // A camera at the origin looking down -z (identity view), 90 degree lens.
        let mut camera = [0.; 32];
        let (near, far) = (0.5, 1000.);
        camera[0] = 1.;
        camera[5] = 1.;
        camera[10] = -(far + near) / (far - near);
        camera[11] = -1.;
        camera[14] = -2. * far * near / (far - near);
        for i in 0..4 {
            camera[16 + i * 5] = 1.;
        }
        let planes = super::frustum(&camera);
        let inside = |c: [f64; 3]| planes.iter().all(|p| p[0] * c[0] + p[1] * c[1] + p[2] * c[2] + p[3] >= -1.);
        assert!(inside([0., 0., -20.]));
        assert!(!inside([0., 0., 20.]));
        assert!(!inside([50., 0., -20.]));
    }
}
