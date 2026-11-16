//! Background world/connection admission, followed by cooperative GL setup.
//! The window owns every display resource and continues the authored timeline.
use super::{
    battle::{BattleController, InstantAdmission, OnlineAdmission},
    settings::AppSettingsStore,
    BattleLoadTarget,
};
use crate::audio::SharedAudioRouter;
use std::{
    cell::RefCell,
    future::Future,
    rc::Rc,
    path::Path,
    pin::Pin,
    sync::{mpsc, Arc},
    task::Poll,
};

/// What the admission worker sends, in this order for Instant Action: the
/// scene's content as soon as it is read, then the admitted world.
enum Message {
    Scene(Result<crate::scene::Prepared, String>),
    Instant(Result<InstantAdmission, String>),
    Online(Result<OnlineAdmission, String>),
}
type Opening = Pin<Box<dyn Future<Output = Result<BattleController, String>>>>;
/// Display steps run until this much of a frame is used (the loading screen
/// draws in the rest of a 60 Hz frame on a slow PC).
const STEP_BUDGET: std::time::Duration = std::time::Duration::from_millis(10);
type WorldSlot = Rc<RefCell<Option<Result<InstantAdmission, String>>>>;

#[derive(Default)]
pub struct Work {
    started: bool,
    rx: Option<mpsc::Receiver<Message>>,
    opening: Option<Opening>,
    /// The admitted world, once it arrives, for an opening already under way.
    world: WorldSlot,
    pub ready: Option<Box<BattleController>>,
    handover: u8,
    display_ms: f64,
    /// When loading began, to say how long before the screen's end it was ready.
    began: Option<std::time::Instant>,
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
        mut load: impl FnMut() -> glow::Context + 'static,
    ) -> Result<(), String> {
        if !self.started {
            let started = std::time::Instant::now();
            self.began = Some(started);
            super::page_battle::PageBattle::prefetch();
            let (tx, rx) = mpsc::sync_channel(2);
            let content_root = root.to_path_buf();
            let target = target.clone();
            let server_target = server_target.clone();
            let snapshot = settings.clone();
            let textures=crate::scene::TextureOptions::discover(&load(),snapshot.interface.get_bool("textureCompression"));
            std::thread::Builder::new()
                .name("bfe-battle-admission".into())
                .stack_size(8 << 20)
                .spawn(move || {
                    let started = std::time::Instant::now();
                    let message = match target {
                        BattleLoadTarget::InstantAction {
                            request,
                            playlist_index,
                        } => {
                            let scene = tx.clone();
                            Message::Instant(BattleController::prepare_instant_action(
                                &content_root,
                                request,
                                playlist_index,
                                &super::battle::BotSettings::read(&snapshot.interface),
                                &snapshot.player.profile_name,
                                textures,
                                move |prepared| {
                                    let _ = scene.send(Message::Scene(prepared));
                                },
                            ))
                        }
                        BattleLoadTarget::Online { launch } => Message::Online(BattleController::prepare_online(
                            &content_root,
                            launch,
                            server_target,
                            &snapshot,
                            textures,
                        )),
                    };
                    eprintln!(
                        "battle loading: admission worker finished in {:.0} ms",
                        started.elapsed().as_secs_f64() * 1000.
                    );
                    let _ = tx.send(message);
                })
                .map_err(|e| format!("cannot start battle admission: {e}"))?;
            self.rx = Some(rx);
            self.started = true;
            eprintln!("battle loading: admission worker started ({:.0} ms)", started.elapsed().as_secs_f64() * 1000.);
        }
        let mut load = Some(load);
        while let Some(rx) = &self.rx {
            let message = match rx.try_recv() {
                Ok(message) => message,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return Err("battle admission worker stopped".into()),
            };
            let root = root.to_path_buf();
            let settings = settings.clone();
            let audio = Arc::clone(&audio);
            match message {
                Message::Scene(prepared) => {
                    let prepared = prepared?;
                    let load = load.take().ok_or("battle display opened twice")?;
                    let slot = Rc::clone(&self.world);
                    self.opening = Some(Box::pin(async move {
                        let world = async move {
                            loop {
                                if let Some(world) = slot.borrow_mut().take() {
                                    return world;
                                }
                                crate::loading_work::idle().await;
                            }
                        };
                        BattleController::open_instant_action(&root, prepared, world, size, &settings, load).await
                    }));
                    eprintln!("battle loading: scene content read; preparing display resources");
                }
                Message::Instant(admitted) => {
                    self.rx = None;
                    if self.opening.is_none() {
                        admitted?;
                        return Err("battle admission sent no scene".into());
                    }
                    *self.world.borrow_mut() = Some(admitted);
                    eprintln!("battle loading: world admitted");
                }
                Message::Online(admitted) => {
                    self.rx = None;
                    let admitted = admitted?;
                    let load = load.take().ok_or("battle display opened twice")?;
                    self.opening = Some(Box::pin(async move {
                        BattleController::open_online(&root, admitted, size, &settings, &audio, load).await
                    }));
                    eprintln!("battle loading: server admitted; preparing display resources");
                }
            }
        }
        // As many display steps as fit in this frame's budget, then the frame.
        let frame_started = std::time::Instant::now();
        while let Some(opening) = &mut self.opening {
            let started = std::time::Instant::now();
            crate::loading_work::take_idle();
            let polled = crate::loading_work::poll(opening.as_mut());
            let from = crate::loading_work::resumed_at();
            let ms = started.elapsed().as_secs_f64() * 1000.;
            self.display_ms += ms;
            if ms > 33. {
                eprintln!("battle loading: display step {ms:.0} ms from {from}");
            }
            let Poll::Ready(result) = polled else {
                if crate::loading_work::take_idle() || frame_started.elapsed() >= STEP_BUDGET {
                    break;
                }
                continue;
            };
            {
                // The shell shows the reason in a dialog; the log keeps it too.
                let result = result.inspect_err(|err| eprintln!("battle loading: display admission failed: {err}"));
                self.ready = Some(Box::new(result?));
                self.opening = None;
                eprintln!(
                    "battle loading: display admission complete ({:.0} ms of display steps); ready {:.2} s after loading began",
                    self.display_ms,
                    self.began.map_or(0., |began| began.elapsed().as_secs_f64())
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
