//! MovieBackdrop/SideMovie. Each side keeps playing
//! beneath every page; a screen without its own movie must not clear it.
use bfe_shell::{
    data::{MovieClip, ScreenMovie},
    movies::{MovieLibrary, MoviePlayer},
    page::FrameTexture,
};

// SIDES and CROSS. The existing decoder and compositor own
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
    /// Movies passed over since one last played. At the length of the sequence (all of them
    /// broken) the side rests on the plain backdrop until `retry`.
    failures: usize,
    /// The layer that showed the side's last frame.
    shown: Option<u64>,
}
#[derive(Default)]
pub(super) struct PageBackdrop {
    sides: [Side; 2],
}
impl PageBackdrop {
    pub fn start(&mut self, movies: &MovieLibrary, left: &mut MoviePlayer, right: &mut MoviePlayer) {
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
                .filter(|clip| movies.resolve(&clip.url).is_some())
                .collect();
            if let Some(clip) = side.clips.first() {
                Self::play(player, movies, clip.clone());
            }
        }
    }
    fn play(player: &mut MoviePlayer, movies: &MovieLibrary, clip: MovieClip) {
        player.play(
            movies,
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
        movies: &MovieLibrary,
        dt: f64,
        left: &mut MoviePlayer,
        right: &mut MoviePlayer,
    ) {
        for (side, player) in self.sides.iter_mut().zip([left, right]) {
            player.update(movies, dt);
            // A movie that cannot play is passed over, as SideMovie's error handler loads the next.
            let failed = std::mem::take(&mut player.failures) as usize;
            if failed > 0 {
                side.failures += failed;
                side.queued_after = None;
            }
            let retry = side.failures < side.clips.len();
            if player.idle() && retry {
                side.at = (side.at + 1) % side.clips.len();
                Self::play(player, movies, side.clips[side.at].clone());
            }
            let front = player
                .layers
                .iter()
                .flatten()
                .filter(|layer| layer.frame.is_some())
                .max_by_key(|layer| layer.serial);
            if let Some(layer) = front {
                let frame = layer.frame.as_ref().unwrap();
                side.aspect = f64::from(frame.width) / f64::from(frame.height.max(1));
                if side.shown != Some(layer.serial) {
                    side.shown = Some(layer.serial);
                    side.failures = 0;
                }
                if side.clips.len() > 1
                    && retry
                    && frame.duration - frame.pts <= CROSS
                    && side.queued_after != Some(layer.serial)
                {
                    side.queued_after = Some(layer.serial);
                    side.at = (side.at + 1) % side.clips.len();
                    Self::play(player, movies, side.clips[side.at].clone());
                }
            }
        }
    }
    /// A new screen tries a side whose movies all failed again (a file may be back).
    pub fn retry(&mut self) {
        for side in &mut self.sides {
            side.failures = 0;
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

#[cfg(test)]
mod tests {
    use super::*;
    use bfe_shell::movies::Source;
    use std::path::PathBuf;

    const PATTERN: &[u8] = include_bytes!("../../../../tests/fixtures/video/pattern.bik");

    /// A library naming each side movie; `broken` ones point at a file that is not there.
    fn library(dir: &std::path::Path, broken: &[&str]) -> MovieLibrary {
        let good = dir.join("pattern.bik");
        std::fs::write(&good, PATTERN).unwrap();
        let mut library = MovieLibrary::default();
        for name in SIDES.concat() {
            let path = if broken.contains(&name) { dir.join("missing.bik") } else { good.clone() };
            library.insert(name, Source { path: path.clone(), offset: 0, length: PATTERN.len() as u64 });
        }
        library
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bfe-backdrop-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Steps the backdrop for `seconds` of real time (the decoders run on their own threads).
    fn run(b: &mut PageBackdrop, movies: &MovieLibrary, players: &mut [MoviePlayer; 2], seconds: f64) {
        let start = std::time::Instant::now();
        let mut last = start;
        while start.elapsed().as_secs_f64() < seconds {
            std::thread::sleep(std::time::Duration::from_millis(5));
            let now = std::time::Instant::now();
            let [left, right] = players;
            b.update(movies, now.duration_since(last).as_secs_f64(), left, right);
            last = now;
        }
    }

    /// A movie of the sequence that cannot be read is passed over: the side keeps playing.
    #[test]
    fn a_broken_movie_in_the_sequence_does_not_darken_the_side() {
        let dir = scratch("broken");
        let movies = library(&dir, &["republic_base", "rebel_base"]);
        let mut b = PageBackdrop::default();
        let mut players = [MoviePlayer::default(), MoviePlayer::default()];
        {
            let [left, right] = &mut players;
            b.start(&movies, left, right);
        }
        run(&mut b, &movies, &mut players, 4.0);
        for (side, player) in players.iter().enumerate() {
            assert!(player.has_frame(), "side {side} went dark");
        }
        // The decoders may still hold the files for a moment; Windows does not remove an open file.
        drop(players);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Every movie of a side unreadable: the side rests on the plain backdrop without trying
    /// again each frame, and the next screen tries again, playing once the files are back.
    #[test]
    fn a_side_whose_movies_all_fail_tries_again_on_the_next_screen() {
        let dir = scratch("gone");
        let movies = library(&dir, &[]);
        let good = dir.join("pattern.bik");
        let away = dir.join("away.bik");
        std::fs::rename(&good, &away).unwrap();
        let mut b = PageBackdrop::default();
        let mut players = [MoviePlayer::default(), MoviePlayer::default()];
        {
            let [left, right] = &mut players;
            b.start(&movies, left, right);
        }
        run(&mut b, &movies, &mut players, 1.0);
        for (side, player) in players.iter().enumerate() {
            assert!(player.idle() && !player.has_frame(), "side {side}");
            assert_eq!(b.sides[side].failures, 3);
        }
        std::fs::rename(&away, &good).unwrap();
        run(&mut b, &movies, &mut players, 0.5);
        assert!(players.iter().all(MoviePlayer::idle), "tried again before a screen change");
        b.retry();
        run(&mut b, &movies, &mut players, 1.5);
        for (side, player) in players.iter().enumerate() {
            assert!(player.has_frame(), "side {side} did not try again");
        }
        // The decoders may still hold the files for a moment; Windows does not remove an open file.
        drop(players);
        let _ = std::fs::remove_dir_all(dir);
    }
}
