//! Load-time unit descriptors. Names are resolved by the asset host; frame
//! code sees numeric clip/bank handles and immutable tables shared by actors.
use bfe_pose::{
    display::{DisplayClip, Tables, NONE},
    rig::skeleton::Skeleton,
    Clip,
};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Bone {
    pub name: String,
    pub parent: i32,
    pub pose: [f64; 10],
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipInput {
    pub frames: usize,
    pub fps: f64,
    pub root_motion: bool,
    pub tracks: Vec<i32>,
    pub data: Vec<f32>,
    pub duration: f64,
    pub speed: f64,
    pub idle: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableInput {
    pub sets: [[Option<usize>; 9]; 3],
    pub alert: [Option<usize>; 9],
    pub transitions: [[Option<usize>; 3]; 3],
    pub jump: Option<usize>,
    pub fall: Option<usize>,
    pub land_soft: Option<usize>,
    pub land_hard: Option<usize>,
    pub dive: Option<usize>,
    pub jet: Option<usize>,
    pub thrown: [[Option<usize>; 4]; 2],
    pub upper: Vec<Vec<usize>>,
}
#[derive(Deserialize)]
pub struct Input {
    pub bones: Vec<Bone>,
    pub clips: Vec<ClipInput>,
    pub tables: TableInput,
    pub walker: Option<WalkerInput>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WalkerInput {
    pub by_mode: [Option<usize>; 14],
    pub pivot: [f64; 3],
    pub roll_center: [f64; 3],
}

pub struct Content {
    pub skeleton: Skeleton,
    pub clips: Vec<DisplayClip>,
    pub tables: Tables,
    pub walker: Option<WalkerInput>,
}
impl Input {
    pub fn load(self) -> Content {
        let skeleton = load_skeleton(self.bones, false);
        let clips = self
            .clips
            .into_iter()
            .map(|c| DisplayClip {
                sample: Clip {
                    frames: c.frames,
                    fps: c.fps,
                    root_motion: c.root_motion,
                    tracks: c.tracks,
                    data: c.data,
                },
                duration: c.duration,
                speed: c.speed,
                idle: c.idle,
            })
            .collect();
        let t = self.tables;
        let resolve = |x: Option<usize>| x.unwrap_or(NONE);
        Content {
            skeleton,
            clips,
            walker: self.walker,
            tables: Tables {
                sets: t.sets.map(|row| row.map(resolve)),
                alert: t.alert.map(resolve),
                transitions: t.transitions.map(|row| row.map(resolve)),
                jump: resolve(t.jump),
                fall: resolve(t.fall),
                land_soft: resolve(t.land_soft),
                land_hard: resolve(t.land_hard),
                dive: resolve(t.dive),
                jet: resolve(t.jet),
                thrown: t.thrown.map(|row| row.map(resolve)),
                upper: t.upper,
            },
        }
    }
}

/// First-person rigs share clips and skeleton construction, with the normal
/// Euler callback kept and the camera offset retained as a native matrix.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirstPersonInput {
    pub bones: Vec<Bone>,
    pub clips: Vec<ClipInput>,
    pub sets: Vec<[Option<usize>; 9]>,
    pub forced_set: Option<usize>,
    pub offset: [f64; 16],
}
impl ClipInput {
    pub fn load(self) -> DisplayClip {
        DisplayClip {
            sample: Clip {
                frames: self.frames,
                fps: self.fps,
                root_motion: self.root_motion,
                tracks: self.tracks,
                data: self.data,
            },
            duration: self.duration,
            speed: self.speed,
            idle: self.idle,
        }
    }
}
impl FirstPersonInput {
    pub fn load(
        self,
    ) -> (
        bfe_pose::first_person::Rig,
        bfe_pose::first_person::Tables,
        Vec<DisplayClip>,
    ) {
        let skeleton = load_skeleton(self.bones, true);
        let mut offset = three::Matrix4::default();
        offset.from_array(&self.offset, 0);
        (
            bfe_pose::first_person::Rig::new(&skeleton, offset),
            bfe_pose::first_person::Tables {
                sets: self
                    .sets
                    .into_iter()
                    .map(|r| r.map(|x| x.unwrap_or(NONE)))
                    .collect(),
                forced_set: self.forced_set,
            },
            self.clips.into_iter().map(ClipInput::load).collect(),
        )
    }
}

fn load_skeleton(bones: Vec<Bone>, euler_sync: bool) -> Skeleton {
    let names = bones.iter().map(|b| b.name.clone()).collect();
    let parents = bones
        .iter()
        .enumerate()
        .map(|(i, b)| if !euler_sync && i == 0 { -1 } else { b.parent })
        .collect();
    let bind: Vec<_> = bones
        .iter()
        .map(|b| {
            let p = b.pose;
            (
                three::Vector3::new(p[0], p[1], p[2]),
                three::Quaternion::new(p[3], p[4], p[5], p[6]),
                three::Vector3::new(p[7], p[8], p[9]),
            )
        })
        .collect();
    Skeleton::from_bind(names, parents, &bind, euler_sync)
}
