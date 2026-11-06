use super::*;
#[derive(Clone, Copy)]
pub(super) struct Icon {
    pub row: usize,
    pub size: f64,
    pub texture: u32,
    pub color: Color,
}
impl HudModel {
    // Team facts arrive once per update. Unauthored teams use the same neutral,
    // locals and unlimited-ticket defaults as the former callback readers.
    fn team_fact(&self, team: f64, field: usize) -> f64 {
        if team >= 0. && team < MAX_TEAMS as f64 {
            self.input[TEAMS + team as usize * T::WORDS + field]
        } else if field == T::TICKETS { f64::INFINITY }
        else if field == T::LIVING { f64::NAN }
        else if field == T::LOCALS { (team > 2.) as u8 as f64 }
        else if team == 0. || self.input[H::VIEWER_TEAM] == 0. { 0. }
        else if team == self.input[H::VIEWER_TEAM] { 1. } else { -1. }
    }
    // RadarUnits.state: numeric handles are assigned at entity construction.
    // A generation change resets both discovery and shot flashes together.
    fn radar_state(&mut self, id: usize, generation: f64) -> Option<&mut RadarState> {
        let state = self.radar.get_mut(id)?;
        if state.generation != generation {
            *state = RadarState { generation, ..RadarState::default() };
        }
        Some(state)
    }
    // RadarUnits.shot retains overlapping flashes in a bounded ring. At the
    // game's maximum fire rate, 32 entries outlive every simultaneous shot.
    pub(super) fn shot(&mut self, id: usize, generation: f64, delay: f64) {
        let now = self.time;
        let Some(state) = self.radar_state(id, generation) else {
            return;
        };
        state.alpha = 1.;
        let duration = delay.max(1. / 60.);
        let mut slot = 0;
        for i in 0..32 {
            if state.shots[i].0 <= now {
                slot = i;
                break;
            }
            if state.shots[i].0 < state.shots[slot].0 {
                slot = i;
            }
        }
        state.shots[slot] = (now + duration, duration);
    }
    // RadarUnits.update/radarSensors: count living soldiers before applying
    // marker eligibility; seated units still count for the ticket reveal.
    // Sensors include the controllable first, then friendly transmitters in
    // source order, with a strict horizontal scan and inclusive 3D transmit.
    pub(super) fn update_radar(&mut self, dt: f64) {
        // RadarUnits.update counts living non-hero soldiers before marker filtering.
        // Vehicles and seated/unmapped bodies may still provide transmitters or game
        // facts; they must not be confused with the later 450-marker display budget.
        // The source rows retain their order across sensor selection and discovery.
        let count = (self.input[H::UNITS] as usize).min(MAX_UNITS);
        let now = self.time;
        let mut living = [0.; 32];
        let mut sensors = [[0.; 4]; 64];
        let mut sensor_count = 0;
        for i in 0..count {
            let o = UNITS + i * U::WORDS;
            let kind = self.input[o + U::KIND];
            if self.input[o + U::ALIVE] != 0.
                && self.input[o + U::HERO] == 0.
                && (kind.is_nan() || kind == 0. || kind == 1. || kind == 3.)
            {
                let team = self.input[o + U::TEAM] as usize;
                if team < 32 {
                    living[team] += 1.;
                }
            }
        }
        let own = self.input[H::CONTROLLABLE];
        if self.input[H::HAS_VIEWER] != 0. && self.input[H::HAS_CONTROL] != 0. {
            let x = self.input[H::CONTROL_X];
            let y = self.input[H::CONTROL_Y];
            let z = self.input[H::CONTROL_Z];
            let scan = self.input[H::CONTROL_SCAN];
            if scan > 0. {
                sensors[0] = [x, z, scan, 0.];
                sensor_count = 1;
            }
            // radarSensors: a controlled remote may not be a registry marker.
            // Its own sensor is still present; other transmitters reach that
            // actual controlled position, and belong to the viewer's team.
            for i in 0..count {
                if sensor_count >= 64 {
                    break;
                }
                let o = UNITS + i * U::WORDS;
                if self.input[o + U::ID] == own
                    || self.input[o + U::TEAM] != self.input[H::VIEWER_TEAM]
                    || self.input[o + U::ALIVE] == 0.
                {
                    continue;
                }
                let scan = self.input[o + U::SCAN];
                let transmit = self.input[o + U::TRANSMIT];
                let dx = self.input[o + U::X] - x;
                let dy = self.input[o + U::Y] - y;
                let dz = self.input[o + U::Z] - z;
                if scan > 0. && transmit > 0. && dx * dx + dy * dy + dz * dz <= transmit * transmit {
                    sensors[sensor_count] = [self.input[o + U::X], self.input[o + U::Z], scan, 0.];
                    sensor_count += 1;
                }
            }
        }
        // The visible icon vector retains capacity. A marker below zero alpha still
        // consumes an eligibility slot, because the original stops after 450 candidate
        // icons rather than after 450 revealed ones. Hidden units retain their state
        // for a future shot, sensor pass or ticket-count reveal.
        self.radar_icons.clear();
        let mut considered = 0;
        for i in 0..count {
            let o = UNITS + i * U::WORDS;
            if self.input[o + U::ID] == self.input[H::VIEWER]
                || self.input[o + U::REMOVED] != 0.
                || self.input[o + U::SEATED] != 0.
                || (self.input[o + U::ALIVE] == 0. && self.input[o + U::DEAD] == 0.)
                || self.input[o + U::HAS_POSITION] == 0.
            {
                continue;
            }
            let kind = self.input[o + U::KIND];
            let soldier = kind == 0. || kind == 1. || kind == 3. || self.input[o + U::SOLDIER] != 0.;
            let texture = if self.input[o + U::HAS_MAP_TEXTURE] == 0. && soldier {
                A::TROOP_ICON
            } else {
                self.input[o + U::TEXTURE] as u32
            };
            if texture == 0 {
                continue;
            }
            considered += 1;
            if considered > 450 {
                break;
            }
            let id = self.input[o + U::ID] as usize;
            let generation = self.input[o + U::GENERATION];
            let Some(state) = self.radar_state(id, generation) else {
                continue;
            };
            let mut flash: f64 = 0.;
            for &(until, duration) in &state.shots {
                if until > now {
                    flash = flash.max(clamp((until - now) / duration));
                }
            }
            let previous = state.alpha;
            let team = self.input[o + U::TEAM] as usize;
            let living = if self.team_fact(self.input[o + U::TEAM], T::LIVING).is_nan() {
                *living.get(team).unwrap_or(&0.)
            } else {
                self.team_fact(self.input[o + U::TEAM], T::LIVING)
            };
            let mut revealed = false;
            for sensor in &sensors[..sensor_count] {
                let dx = sensor[0] - self.input[o + U::X];
                let dz = sensor[1] - self.input[o + U::Z];
                if dx * dx + dz * dz < sensor[2] * sensor[2] {
                    revealed = true;
                    break;
                }
            }
            // radarAlpha gives friendship, Easy and dwindling tickets full visibility.
            // Otherwise a scan or shot flash supplies the immediate lower bound, while
            // previous discovery decays at 32/255 each second. Visual marker position is
            // never involved in this choice.
            let alpha = if self.team_fact(self.input[o + U::TEAM], T::RELATION) > 0.
                || self.input[H::EASY] != 0.
                || self.team_fact(self.input[o + U::TEAM], T::TICKETS) <= living
            {
                1.
            } else {
                (if revealed { 1. } else { clamp(flash) }).max(clamp(previous - dt.max(0.) * 32. / 255.))
            };
            self.radar[id].alpha = alpha;
            if alpha <= 0. {
                continue;
            }
            // unitColor shades dead bodies before applying the white shot flash.
            // Locals retain their own yellow relationship colour, even when allied with
            // another army. Opacity is the discovery alpha, independent of RGB flash.
            self.heading(o, id);
            let team = self.input[o + U::TEAM];
            let rel = if team == 0. {
                0.
            } else if team != self.input[H::VIEWER_TEAM] && self.team_fact(self.input[o + U::TEAM], T::LOCALS) != 0. {
                3.
            } else if self.team_fact(self.input[o + U::TEAM], T::RELATION) > 0. {
                1.
            } else if self.team_fact(self.input[o + U::TEAM], T::RELATION) < 0. {
                2.
            } else {
                0.
            };
            let mut color = relation(rel);
            for c in &mut color[..3] {
                if self.input[o + U::ALIVE] == 0. {
                    *c = (*c + 64. / 255.) / 2.;
                }
                *c += (1. - *c) * clamp(flash);
            }
            color[3] = clamp(alpha);
            let default_scale = if soldier {
                if kind == 3. {
                    1.6
                } else {
                    1.4
                }
            } else {
                1.
            };
            let scale = self.input[o + U::SCALE];
            let scale = if scale.is_finite() { scale } else { default_scale };
            self.radar_icons.push(Icon { row: i, size: 6. * scale.max(0.), texture, color });
        }
    }
    // mapHeading: a vertical vehicle nose retains the last horizontal yaw.
    // Rendering samples the visual pose again after page bodies are posed.
    pub(super) fn heading(&mut self, o: usize, id: usize) -> f64 {
        let previous = self.radar[id].yaw;
        let yaw = self.input[o + U::YAW];
        let fallback = if previous.is_finite() {
            previous
        } else if yaw.is_finite() {
            yaw
        } else {
            0.
        };
        let yaw = if self.input[o + U::FORWARD] != 0. {
            let (x, z) = if self.input[o + U::HAS_QUATERNION] != 0. {
                let q = &self.input[o + U::QUATERNION..o + U::QUATERNION + 4];
                // Vector3.applyQuaternion on the local forward (0,0,1), with
                // the same cross-product operations as the original mapHeading.
                let tx = 2. * (q[1] * 1. - q[2] * 0.);
                let ty = 2. * (q[2] * 0. - q[0] * 1.);
                let tz = 2. * (q[0] * 0. - q[1] * 0.);
                (0. + q[3] * tx + q[1] * tz - q[2] * ty, 1. + q[3] * tz + q[0] * ty - q[1] * tx)
            } else {
                (self.input[o + U::FORWARD_X], self.input[o + U::FORWARD_Z])
            };
            if x.is_finite() && z.is_finite() && x * x + z * z > 1e-8 {
                x.atan2(z)
            } else {
                fallback
            }
        } else if yaw.is_finite() {
            yaw
        } else {
            fallback
        };
        self.radar[id].yaw = yaw;
        yaw
    }
}
