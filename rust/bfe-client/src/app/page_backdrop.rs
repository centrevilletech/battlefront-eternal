//! MovieBackdrop/SideMovie from src/shell/backdrop.js. Each side keeps playing
//! beneath every page; a screen without its own movie must not clear it.
use bfe_shell::{
    data::{MovieClip, ScreenMovie},
    movies::MoviePlayer,
    page::FrameTexture,
};
use std::path::Path;

// SIDES and CROSS in backdrop.js. The existing decoder and compositor own
// frame recycling, sharpening and fades, independently of menu transitions.
const SIDES: [[&str; 3]; 2] = [
    ["republic_win", "republic_base", "cis_win"],
    ["imperial_win", "rebel_base", "rebel_win"],
];
const CROSS: f64 = 1.2;
#[derive(Default)]
struct Side {
    clips: Vec<MovieClip>,
    at: usize,
    queued_after: Option<u64>,
    aspect: f64,
}
#[derive(Default)]
pub(super) struct PageBackdrop {
    sides: [Side; 2],
}
impl PageBackdrop {
    pub fn start(&mut self, root: &Path, left: &mut MoviePlayer, right: &mut MoviePlayer) {
        for ((side, names), player) in self.sides.iter_mut().zip(SIDES).zip([left, right]) {
            *side = Side::default();
            // Missing optional movies are skipped exactly as SideMovie's error
            // handler advances to the next name. No still image is substituted.
            side.clips = names
                .iter()
                .map(|name| MovieClip {
                    url: format!("/common/shell/movies/{name}.webm"),
                    fade_in: CROSS,
                    fade_out: CROSS,
                })
                .filter(|clip| root.join(clip.url.trim_start_matches('/')).is_file())
                .collect();
            if let Some(clip) = side.clips.first() {
                Self::play(player, root, clip.clone());
            }
        }
    }
    fn play(player: &mut MoviePlayer, root: &Path, clip: MovieClip) {
        player.play(
            root,
            ScreenMovie {
                intro: None,
                loop_clip: Some(clip),
            },
            false,
            0.,
        );
    }
    pub fn update(
        &mut self,
        root: &Path,
        dt: f64,
        left: &mut MoviePlayer,
        right: &mut MoviePlayer,
    ) {
        for (side, player) in self.sides.iter_mut().zip([left, right]) {
            player.update(root, dt);
            let front = player
                .layers
                .iter()
                .flatten()
                .filter(|layer| layer.frame.is_some())
                .max_by_key(|layer| layer.serial);
            if let Some(layer) = front {
                let frame = layer.frame.as_ref().unwrap();
                side.aspect = f64::from(frame.width) / f64::from(frame.height.max(1));
                if side.clips.len() > 1
                    && frame.duration - frame.pts <= CROSS
                    && side.queued_after != Some(layer.serial)
                {
                    side.queued_after = Some(layer.serial);
                    side.at = (side.at + 1) % side.clips.len();
                    Self::play(player, root, side.clips[side.at].clone());
                }
            }
        }
    }
    /// Upscale at the movie's aspect, then crop as object-fit: cover does.
    /// This avoids stretching a 4:3 movie into a tall side panel.
    pub fn output_size(&self, side: usize, size: [i32; 2]) -> [i32; 2] {
        let height = size[1].max(1);
        [
            ((height as f64 * self.sides[side].aspect.max(1.)).round() as i32).max(1),
            height,
        ]
    }
    pub fn frames(&self, handles: [u32; 2], size: [i32; 2]) -> [FrameTexture; 2] {
        let panel_aspect = f64::from(size[0].max(1)) * 0.56 / f64::from(size[1].max(1));
        std::array::from_fn(|side| {
            let aspect = self.sides[side].aspect.max(1.);
            let visible = (panel_aspect / aspect).min(1.);
            FrameTexture {
                texture: handles[side],
                uv: [(1. - visible) / 2., 0., (1. + visible) / 2., 1.],
            }
        })
    }
}
