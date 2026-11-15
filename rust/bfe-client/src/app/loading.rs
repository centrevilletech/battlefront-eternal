//! Background world/connection admission, followed by cooperative GL setup.
//! The window owns every display resource and continues the authored timeline.
use super::{
    battle::{BattleController, InstantAdmission, OnlineAdmission},
    settings::AppSettingsStore,
    BattleLoadTarget,
};
use crate::audio::SharedAudioRouter;
use std::{
    future::Future,
    path::Path,
    pin::Pin,
    sync::{mpsc, Arc},
    task::Poll,
};

enum Admitted {
    Instant(InstantAdmission),
    Online(OnlineAdmission),
}
type Opening = Pin<Box<dyn Future<Output = Result<BattleController, String>>>>;

#[derive(Default)]
pub struct Work {
    started: bool,
    rx: Option<mpsc::Receiver<Result<Admitted, String>>>,
    opening: Option<Opening>,
    pub ready: Option<Box<BattleController>>,
    handover: u8,
    display_ms: f64,
}
impl Work {
    pub fn advance(
        &mut self,
        root: &Path,
        target: &BattleLoadTarget,
        size: [i32; 2],
        settings: &AppSettingsStore,
        audio: Arc<SharedAudioRouter>,
        server_target: &Option<String>,
        load: impl FnMut() -> glow::Context + 'static,
    ) -> Result<(), String> {
        if !self.started {
            super::page_battle::PageBattle::prefetch();
            let (tx, rx) = mpsc::sync_channel(1);
            let content_root = root.to_path_buf();
            let target = target.clone();
            let server_target = server_target.clone();
            let snapshot = settings.clone();
            std::thread::Builder::new()
                .name("bfe-battle-admission".into())
                .stack_size(8 << 20)
                .spawn(move || {
                    let started = std::time::Instant::now();
                    let result = match target {
                        BattleLoadTarget::InstantAction {
                            request,
                            playlist_index,
                        } => BattleController::prepare_instant_action(
                            &content_root,
                            request,
                            playlist_index,
                            &super::battle::BotSettings::read(&snapshot.interface),
                            &snapshot.player.profile_name,
                        )
                        .map(Admitted::Instant),
                        BattleLoadTarget::Online { launch } => BattleController::prepare_online(
                            &content_root,
                            launch,
                            server_target,
                            &snapshot,
                        )
                        .map(Admitted::Online),
                    };
                    eprintln!(
                        "battle loading: admission worker finished in {:.0} ms",
                        started.elapsed().as_secs_f64() * 1000.
                    );
                    let _ = tx.send(result);
                })
                .map_err(|e| format!("cannot start battle admission: {e}"))?;
            self.rx = Some(rx);
            self.started = true;
            eprintln!("battle loading: admission worker started");
        }
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(result) => {
                    let admitted = result?;
                    self.rx = None;
                    let root = root.to_path_buf();
                    let settings = settings.clone();
                    self.opening = Some(Box::pin(async move {
                        match admitted {
                            Admitted::Instant(value) => {
                                BattleController::open_instant_action(
                                    &root, value, size, &settings, &audio, load,
                                )
                                .await
                            }
                            Admitted::Online(value) => {
                                BattleController::open_online(
                                    &root, value, size, &settings, &audio, load,
                                )
                                .await
                            }
                        }
                    }));
                    eprintln!("battle loading: world admitted; preparing display resources");
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("battle admission worker stopped".into())
                }
            }
        }
        if let Some(opening) = &mut self.opening {
            let started = std::time::Instant::now();
            let polled = crate::loading_work::poll(opening.as_mut());
            let from = crate::loading_work::resumed_at();
            let ms = started.elapsed().as_secs_f64() * 1000.;
            self.display_ms += ms;
            if ms > 33. {
                eprintln!("battle loading: display step {ms:.0} ms from {from}");
            }
            if let Poll::Ready(result) = polled {
                // The shell shows the reason in a dialog; the log keeps it too.
                let result = result.inspect_err(|err| eprintln!("battle loading: display admission failed: {err}"));
                self.ready = Some(Box::new(result?));
                self.opening = None;
                eprintln!(
                    "battle loading: display admission complete ({:.0} ms of display steps)",
                    self.display_ms
                );
            }
        }
        Ok(())
    }
    pub fn handover(
        &mut self,
        timeline_done: bool,
        audio: &SharedAudioRouter,
    ) -> Option<Box<BattleController>> {
        if timeline_done && self.ready.is_some() {
            self.handover += 1;
        }
        if self.handover >= 2 {
            let mut battle = self.ready.take()?;
            battle.activate(audio);
            Some(battle)
        } else {
            None
        }
    }
}
