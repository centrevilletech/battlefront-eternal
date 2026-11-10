//! Deterministic page screenshot tour over the real shell/controller widgets.
//! Runs only when explicitly requested with --script page-parity.
use super::{
    script::GateDriver,
    settings::AppSettingsStore,
    shell::{ShellHost, ShellTransition},
};
use bfe_shell::screens::{IaFocus, PopupState, ScreenId};
use std::time::Instant;
pub(super) struct PageProof {
    step: usize,
    started: Instant,
}
impl PageProof {
    pub fn new() -> Self {
        Self {
            step: 0,
            started: Instant::now(),
        }
    }
    pub fn step(
        &mut self,
        driver: &mut GateDriver,
        shell: &mut ShellHost,
        _settings: &mut AppSettingsStore,
    ) -> Result<Option<ShellTransition>, String> {
        // Admission may take seconds; start the gate's settling interval here.
        if self.step == 0 {
            self.step = 1;
            self.started = Instant::now();
            return Ok(None);
        }
        if self.started.elapsed().as_secs_f64() < if self.step == 1 { 3.0 } else { 2.0 } {
            return Ok(None);
        }
        let file = match self.step {
            1 => "page-01-gate.png",
            2 => "page-02-main.png",
            3 => "page-03-singleplayer.png",
            4 => "page-04-instant-action.png",
            5 => "page-05-era.png",
            6 => "page-06-playlist.png",
            7 => "page-07-options.png",
            8 => "page-08-game-options.png",
            9 => "page-09-profile.png",
            10 => "page-10-profile-form.png",
            11 => "page-11-multiplayer.png",
            12 => "page-12-theme-picker.png",
            13 => "page-13-bf1.png",
            14 => "page-14-bf2.png",
            15 => "page-15-exit-popup.png",
            16 => "page-16-online-notice.png",
            17 => "page-17-direct-connect.png",
            18 => "page-18-host.png",
            19 => "page-19-integrations.png",
            20 => "page-20-video-options.png",
            21 => "page-21-audio-options.png",
            22 => "page-22-controls.png",
            _ => {
                driver.page_capture_done = true;
                return Ok(Some(ShellTransition::Exit));
            }
        };
        driver.save_shell_shot(shell, file)?;
        match self.step {
            1 => shell.app.unlock_gate(),
            2 => shell.app.push_screen(ScreenId::Singleplayer),
            3 => shell.app.accept(&mut shell.browser),
            4 => {
                if let Some(index) = shell.app.data.maps.iter().position(|m| m.id == "nab2") {
                    shell.app.ia_focus = IaFocus::Available;
                    shell
                        .app
                        .move_vertical(index as isize - shell.app.ia_available_selected as isize);
                }
                shell.app.ia_focus = IaFocus::Add;
                shell.app.accept(&mut shell.browser);
            }
            5 => shell.app.accept(&mut shell.browser),
            6 => {
                shell.app.push_screen(ScreenId::Options);
                shell.options_ctrl.stack.clear();
            }
            7 => shell.options_ctrl.open_page("game"),
            8 => shell.app.push_screen(ScreenId::Profile),
            9 => shell.app.profile.begin_edit(""),
            10 => {
                shell.app.back();
                shell.app.push_screen(ScreenId::Multiplayer);
            }
            11 => shell.page.dropdown = true,
            12 => {
                shell.page.dropdown = false;
                shell.app.settings.menu_theme = "bf1".into();
                shell.app.push_screen(ScreenId::Main);
            }
            13 => shell.app.settings.menu_theme = "bf2".into(),
            14 => {
                shell.app.settings.menu_theme = "eternal".into();
                shell.app.popup = Some(PopupState::ConfirmExit {
                    message: "EXIT TO WINDOWS?".into(),
                    selected_yes: false,
                });
            }
            15 => {
                shell.app.back();
                shell.app.push_screen(ScreenId::Multiplayer);
                shell.app.popup = Some(bfe_shell::screens::ShellApp::online_notice());
            }
            16 => {
                shell.app.back();
                shell.browser_footer_action(2);
            }
            17 => {
                shell.app.back();
                shell.app.profile.begin_edit("");
                shell.app.profile.draft = "PARITY PILOT".into();
                shell.app.profile.submit();
                shell.app.open_host_dialog();
            }
            18 => {
                shell.app.back();
                shell.app.push_screen(ScreenId::Options);
                shell.options_ctrl.stack.clear();
                shell.sync_options_controller(_settings);
                shell.open_integrations(_settings);
            }
            19 => shell.options_ctrl.open_page("video"),
            20 => shell.options_ctrl.open_page("audio"),
            21 => shell.options_ctrl.open_page("controls"),
            _ => {}
        }
        self.step += 1;
        self.started = Instant::now();
        Ok(None)
    }
}
