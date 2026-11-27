use super::*;
use bfe_shell::{
    loading2::{prepared::PreparedBackdrop, MapLoadPhase, MapLoadSequence, TICKS_PER_SECOND},
    render::{
        bitmap_tip_lines, draw_loading2, BackdropDraw, BackdropTextPlacement, BackdropTextures,
    },
};
use std::io::Read;

pub(super) struct MapLoad {
    data: PreparedBackdrop,
    sequence: MapLoadSequence,
    ticks: u32,
    word: Vec<u16>,
    lines: Vec<Vec<Vec<u16>>>,
    handles: Vec<glow::Texture>,
    textures: BackdropTextures,
    overlay: Option<(BackdropTextPlacement, Option<usize>)>,
}

impl ShellHost {
    pub(super) fn release_map_load(&mut self) {
        if let Some(load) = self.map_load.take() {
            for texture in load.handles {
                unsafe { self.core.gl.delete_texture(texture) };
            }
        }
    }

    pub(crate) fn prepare_instant_loading(
        &mut self,
        planned: bfe_shell::launch::PlannedBattle<'_>,
        audio: &SharedAudioRouter,
    ) -> Result<(), String> {
        self.prepare_family_loading(planned.family, planned.map, audio)
    }

    /// The loading screen of `map`. A 2005 map's comes from its prepared game data: when that
    /// cannot be read the error is returned and the screen already shown stays.
    pub(crate) fn prepare_family_loading(
        &mut self, family: GameFamily, map: &str, audio: &SharedAudioRouter,
    ) -> Result<(), String> {
        if family == GameFamily::Swbf1 {
            self.release_map_load();
            self.prepare_map_loading(map, audio);
            return Ok(());
        }
        let root = self
            .content_dir
            .as_deref()
            .ok_or("missing prepared content directory")?;
        let root = crate::app::instant::content_root(root, family);
        let lib = Library::new(root.as_ref(), false, 2048, 0)?;
        let path = lib.path("common/shell/load2.json")?;
        let data = PreparedBackdrop::read(
            std::fs::File::open(path).map_err(|e| format!("2005 loading manifest: {e}"))?,
            map,
            None,
        )?;
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos();
        let sequence = MapLoadSequence::new(
            data.duration,
            data.tip_ticks,
            data.pictures.len(),
            data.tips.len(),
            seed,
        )?;
        let MapLoadPhase::Showing(frame) = sequence.phase(false, 0.) else {
            return Err("empty loading sequence".into());
        };
        let lines = data
            .tips
            .iter()
            .map(|tip| bitmap_tip_lines(&tip.text, &tip.line_ends).ok_or("invalid loading tip"))
            .collect::<Result<Vec<_>, _>>()?;
        let mut handles = Vec::with_capacity(3);
        let textures = BackdropTextures::load(
            self.textures.solid,
            frame.picture.map(|i| data.pictures[i].as_str()),
            data.fallback.as_deref(),
            data.panel.as_deref(),
            data.led.as_deref(),
            |name| {
                let path = lib.path(&format!("common/shell/{name}")).ok()?;
                let file = std::fs::File::open(path).ok()?;
                if file.metadata().ok()?.len() > 20 << 20 {
                    return None;
                }
                let mut bytes = Vec::new();
                file.take((20 << 20) + 1).read_to_end(&mut bytes).ok()?;
                if bytes.len() > 20 << 20 {
                    return None;
                }
                let img = bfe_render::texture_image::decode_png(&bytes, 2048).ok()?;
                let tex = Self::upload_rgba(
                    &self.core.gl,
                    img.width as i32,
                    img.height as i32,
                    &img.rgba,
                )
                .ok()?;
                handles.push(tex);
                Some((self.core.add_texture(tex), img.width, img.height))
            },
        );
        self.release_map_load();
        self.suspend_movies();
        audio.set_live(None);
        audio.set_shell(None);
        self.loading_overlay = None;
        self.battle_loading_kind = loading_screen_for(family);
        self.loading_time = 0.;
        self.loading_peak_ms = 0.;
        self.loading_frames = 0;
        self.battle_loading_started = Instant::now();
        eprintln!(
            "battle loading: {:?}/{} {} ticks, picture {}",
            family,
            map,
            data.duration.get(),
            textures.picture
        );
        self.map_load = Some(MapLoad {
            word: data.label.encode_utf16().collect(),
            data,
            sequence,
            ticks: 0,
            lines,
            handles,
            textures,
            overlay: None,
        });
        Ok(())
    }

    pub(super) fn draw_map_load(&mut self) {
        let load = self.map_load.as_mut().unwrap();
        self.loading_time = self.battle_loading_started.elapsed().as_secs_f64();
        self.loading_frames += 1;
        let clock = self.loading_time * f64::from(TICKS_PER_SECOND);
        let target = (clock as u32).min(load.data.duration.get());
        while load.ticks < target {
            load.sequence.tick();
            load.ticks += 1;
        }
        if let MapLoadPhase::Showing(state) = load.sequence.phase(false, clock.fract()) {
            let rows = state.tip.map_or(&[][..], |i| load.lines[i].as_slice());
            let show = BackdropDraw {
                textures: load.textures,
                state,
                tip: rows,
                border: load.data.border,
                max_tip_lines: load.data.max_lines,
                leds: load.data.bar,
                blink_rate: load.data.blink_rate,
                tip_height: rows.len() as f64 * (self.fonts.small.height + 2.),
                word_width: self.fonts.medium.width(&load.word, 1., 0.),
                ..Default::default()
            };
            let placement = draw_loading2(
                &mut self.batch,
                &self.fonts,
                &show,
                &load.data.title,
                &load.data.label,
                LookId::from_theme(Some(&self.app.settings.menu_theme)),
                self.layout_size[0] as f64,
                self.layout_size[1] as f64,
            );
            load.overlay = Some((placement, state.tip));
        } else {
            self.batch.begin();
            load.overlay = None;
        }
        self.upload_and_draw_batch();
    }

    pub(super) fn map_load_done(&self) -> Option<bool> {
        self.map_load
            .as_ref()
            .map(|load| load.ticks >= load.data.duration.get())
    }

    pub(crate) fn map_load_text(&self) -> Option<(BackdropTextPlacement, &str, &str, &str)> {
        let load = self.map_load.as_ref()?;
        let (placement, tip) = load.overlay?;
        Some((
            placement,
            &load.data.title,
            tip.map_or("", |i| &load.data.tips[i].text),
            &load.data.label,
        ))
    }
}
