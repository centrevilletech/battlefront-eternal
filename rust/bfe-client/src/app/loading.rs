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
    /// The online battle's map, as soon as the server has said which battle it runs.
    Map(bfe_family::GameFamily, String),
    Online(Result<OnlineAdmission, String>),
}
type Opening = Pin<Box<dyn Future<Output = Result<BattleController, String>>>>;
/// Display steps run until this much of a frame is used (the loading screen
/// draws in the rest of a 60 Hz frame on a slow PC).
const STEP_BUDGET: std::time::Duration = std::time::Duration::from_millis(10);
type WorldSlot = Rc<RefCell<Option<Result<InstantAdmission, String>>>>;

/// What loading hands the app: a battle once the loading screen's timeline
/// ends, or, at once, a 2005 battle this client sits out, with the launch
/// that carries on to the next battle.
pub enum Handover {
    Battle(Box<BattleController>),
    SitOut(Box<crate::online::sitout::SitOut>, bfe_shell::screens::MultiplayerLaunch),
}

#[derive(Default)]
pub struct Work {
    started: bool,
    rx: Option<mpsc::Receiver<Message>>,
    opening: Option<Opening>,
    /// The admitted world, once it arrives, for an opening already under way.
    world: WorldSlot,
    pub ready: Option<Box<BattleController>>,
    /// A 2005 battle this client sits out: no display to prepare.
    sitting: Option<(Box<crate::online::sitout::SitOut>, bfe_shell::screens::MultiplayerLaunch)>,
    handover: u8,
    display_ms: f64,
    /// When loading began, to say how long before the screen's end it was ready.
    began: Option<std::time::Instant>,
    /// The online battle's map once known, for the loading screen to show.
    pub map: Option<(bfe_family::GameFamily, String)>,
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
            let (tx, rx) = mpsc::sync_channel(4);
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
                        BattleLoadTarget::Online { launch } => {
                            let map = tx.clone();
                            Message::Online(BattleController::prepare_online(
                                &content_root,
                                launch,
                                server_target,
                                &snapshot,
                                textures,
                                move |family, id| {
                                    let _ = map.send(Message::Map(family, id.to_string()));
                                },
                            ))
                        }
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
                Message::Map(family, id) => self.map = Some((family, id)),
                Message::Online(admitted) => {
                    self.rx = None;
                    let admitted = match admitted?.sit_out() {
                        Ok((sitting, launch)) => {
                            self.sitting = Some((Box::new(sitting), launch));
                            eprintln!("battle loading: sitting this Battlefront II battle out");
                            continue;
                        }
                        Err(admitted) => admitted,
                    };
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
            let to = crate::loading_work::suspended_at();
            let ms = started.elapsed().as_secs_f64() * 1000.;
            self.display_ms += ms;
            if ms > 33. {
                eprintln!("battle loading: display step {ms:.0} ms from {from} to {to}");
            }
            let Poll::Ready(result) = polled else {
                if crate::loading_work::take_idle() || frame_started.elapsed() >= STEP_BUDGET {
                    break;
                }
                continue;
            };
            {
                // The shell shows the reason in a dialog; the log keeps it too.
                // The finished future is dropped first: polled again after a failure it would panic.
                self.opening = None;
                let result = result.inspect_err(|err| eprintln!("battle loading: display admission failed: {err}"));
                self.ready = Some(Box::new(result?));
                eprintln!(
                    "battle loading: display admission complete ({:.0} ms of display steps); ready {:.2} s after loading began",
                    self.display_ms,
                    self.began.map_or(0., |began| began.elapsed().as_secs_f64())
                );
            }
        }
        Ok(())
    }
    fn handover_frame(&mut self, timeline_done: bool, ready: bool) -> bool {
        if timeline_done && ready {
            self.handover += 1;
        }
        self.handover >= 2
    }

    pub fn handover(
        &mut self,
        timeline_done: bool,
        audio: &SharedAudioRouter,
    ) -> Option<Handover> {
        if let Some((sitting, launch)) = self.sitting.take() {
            return Some(Handover::SitOut(sitting, launch));
        }
        if self.handover_frame(timeline_done, self.ready.is_some()) {
            let mut battle = self.ready.take()?;
            battle.activate(audio);
            Some(Handover::Battle(battle))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bfe_shell::loading::{LoadingTimeline, LEAD};

    #[test]
    fn ready_planet_load_hands_over_on_the_released_tick_even_after_retarget() {
        // Both Instant Action and an initially unknown server map use this gate.
        for online in [false, true] {
            let mut timeline = LoadingTimeline::default();
            let mut work = Work::default();
            for tick in 0..=817 {
                if online && tick == 240 {
                    timeline.targets = vec![Some([0.1, 0.2, 0.3, 0.4]); 4];
                }
                let elapsed = tick as f64 / 60.0 - LEAD;
                assert_eq!(work.handover_frame(elapsed >= timeline.duration(), true), tick == 817);
            }
        }
    }
}
