//! Fixed-tick player input for developer mission recordings.
use super::runtime::{Engine, Runtime};
use crate::generated_input::{ClientCommand, ClientFlightCommand, ClientLook};
use bfe_sim::units::infantry::Controls;
use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

#[derive(Clone, Debug, serde::Deserialize)]
struct Deployment {
    team: u8,
    class_name: String,
    post: String,
}

#[derive(Clone, Debug, Default)]
pub struct Frame {
    held: u32,
    look: [f64; 2],
    controls: Controls,
    deployment: Option<Box<Deployment>>,
}

impl Frame {
    pub fn submit<E: Engine>(
        &self,
        runtime: &mut Runtime<E>,
        generation: f64,
    ) -> Result<(), String> {
        if let Some(choice) = &self.deployment {
            runtime.spawn_human(choice.team, &choice.class_name, &choice.post)?;
        }
        let next = runtime.tick() + 1.;
        let command = ClientCommand {
            tick: next,
            sequence: next,
            generation,
            held: self.held,
            look_count: 1,
            has_aspect: true,
            aspect: 16. / 9.,
            has_mouse_scale: true,
            mouse_scale: 1.,
            ..Default::default()
        };
        let look = ClientLook {
            x: self.look[0],
            y: self.look[1],
        };
        let flight = ClientFlightCommand {
            scale: 1.,
            curve: 1.,
            ..Default::default()
        };
        runtime.submit(&command, &look, &flight, runtime.tick(), None, false)?;
        runtime.infantry_input(next, self.controls)
    }
}

/// A tick's player controls and optional reliable spawn-screen selection.
pub fn read(path: &Path) -> Result<Vec<Frame>, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    read_rows(BufReader::new(file))
}

fn read_rows(reader: impl BufRead) -> Result<Vec<Frame>, String> {
    #[derive(serde::Deserialize)]
    struct Row(
        u32,
        u64,
        u64,
        u8,
        u8,
        u8,
        #[serde(default)] Option<Box<Deployment>>,
    );
    let mut frames = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|e| e.to_string())?;
        let Row(held, x, y, flags, melee_held, melee_pressed, deployment) =
            serde_json::from_str(&line)
                .map_err(|e| format!("mission input tick {}: {e}", frames.len() + 1))?;
        let (x, y) = (f64::from_bits(x), f64::from_bits(y));
        if !x.is_finite()
            || !y.is_finite()
            || flags > 63
            || deployment
                .as_ref()
                .is_some_and(|p| !(1..=2).contains(&p.team))
        {
            return Err(format!(
                "invalid mission input at tick {}",
                frames.len() + 1
            ));
        }
        frames.push(Frame {
            held,
            look: [x, y],
            deployment,
            controls: Controls {
                sprint: flags & 1 != 0,
                roll: flags & 2 != 0,
                block: flags & 4 != 0,
                hold_to_block: flags & 16 != 0,
                offhand_pressed: flags & 32 != 0,
                lock_pressed: false,
                block_pressed: flags & 8 != 0,
                melee_held,
                melee_pressed,
            },
        });
    }
    if frames.is_empty() {
        return Err("empty mission input recording".into());
    }
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spawn_choice_occupies_only_its_recorded_tick() {
        let rows = b"[0,0,0,0,0,0,[2,\"pilot\",\"north\"]]\n[0,0,0,0,0,0]";
        let frames = read_rows(&rows[..]).unwrap();
        let spawn = frames[0].deployment.as_ref().unwrap();
        assert_eq!(
            (spawn.team, spawn.class_name.as_str(), spawn.post.as_str()),
            (2, "pilot", "north")
        );
        assert!(frames[1].deployment.is_none());
        assert!(read_rows(&b"[0,0,0,0,0,0,[3,\"pilot\",\"north\"]]"[..]).is_err());
    }

    #[test]
    fn mission_recording_preserves_mouse_bits_and_button_edges() {
        let yaw = -3.125f64;
        let input = format!(
            "[1,{},{},13,1,2]\n[0,0,0,0,0,0]\n",
            yaw.to_bits(),
            (-0.0f64).to_bits()
        );
        let frames = read_rows(input.as_bytes()).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].held, 1);
        assert_eq!(
            frames[0].look.map(f64::to_bits),
            [yaw.to_bits(), (-0.0f64).to_bits()]
        );
        assert!(
            frames[0].controls.sprint
                && frames[0].controls.block
                && frames[0].controls.block_pressed
        );
        assert_eq!(
            (
                frames[0].controls.melee_held,
                frames[0].controls.melee_pressed
            ),
            (1, 2)
        );
        assert_eq!(frames[1].controls.melee_pressed, 0);

        let invalid = format!("[0,{},0,0,0,0]", f64::NAN.to_bits());
        assert!(read_rows(invalid.as_bytes()).is_err());
        assert!(read_rows(&b"[0,0,0,64,0,0]"[..]).is_err());
        assert!(read_rows(&b""[..]).is_err());
    }
}

/// A developer capture can replay the lead-in before it records the end at normal speed.
pub(super) struct Recording {
    frames: Vec<Frame>,
    pub(super) preroll: u64,
    generation: f64,
}

impl Recording {
    pub(super) fn for_mission() -> Result<Option<Self>, String> {
        if std::env::var_os("BFE_BENCH").is_none() {
            return Ok(None);
        }
        let Some(path) = std::env::var_os("BFE_BENCH_MATCH_INPUT") else {
            return Ok(None);
        };
        let frames = read(Path::new(&path))?;
        let preroll = std::env::var("BFE_BENCH_MATCH_PREROLL")
            .ok()
            .map(|v| v.parse::<u64>())
            .transpose()
            .map_err(|_| "invalid mission recording start")?
            .unwrap_or(0);
        if preroll > frames.len() as u64 {
            return Err("recording start exceeds its length".into());
        }
        Ok(Some(Self {
            frames,
            preroll,
            generation: 0.,
        }))
    }

    pub(super) fn tick<E: Engine>(
        &mut self,
        runtime: &mut Runtime<E>,
        slot: &mut super::thread::PublicationSlot,
    ) -> Result<f64, String> {
        let neutral = Frame::default();
        let input = self.frames.get(runtime.ticks as usize).unwrap_or(&neutral);
        input.submit(runtime, self.generation)?;
        let tick = runtime.step_into_slot(slot)?;
        self.generation = slot.infantry.map_or(0., |s| s.generation);
        Ok(tick)
    }
}
