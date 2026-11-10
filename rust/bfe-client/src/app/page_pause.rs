//! Battle blur target and active look state for page-look battle screens.
use super::settings::AppSettingsStore;
use crate::scene::Scene;

pub(super) struct PagePause {
    pub theme: String,
    pub blur: super::page_blur::PageBlur,
    pending: bool,
}

impl PagePause {
    pub fn set_theme(&mut self, index: usize) {
        let next = ["bf1", "bf2", "eternal"][index.min(2)];
        if self.theme != next {
            self.theme = next.into();
            self.pending = true;
        }
    }

    pub fn save(&mut self, settings: &mut AppSettingsStore) {
        if !self.pending {
            return;
        }
        self.pending = false;
        settings.player.shell.menu_theme = self.theme.clone();
        settings.sync_interface_from_player();
        if let Err(error) = settings.save_player() {
            eprintln!("cannot save menu look: {error}");
        }
    }

    pub fn admit(
        scene: &mut Scene,
        size: [i32; 2],
        settings: &AppSettingsStore,
    ) -> Result<Self, String> {
        let blur = super::page_blur::PageBlur::new(&scene.bridge.r.core.gl, size)?;
        scene.bridge.r.core.state.reset();
        Ok(Self {
            blur,
            theme: settings.player.shell.menu_theme.clone(),
            pending: false,
        })
    }
}
