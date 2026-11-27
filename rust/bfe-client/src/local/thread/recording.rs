//! Window playback of fixed-tick mission input. Every publication, including
//! the lead-in before a take, crosses the ordinary local receiver in order.
use super::{Engine, Runtime, SharedState, WorldMessage};
use crate::local::mission_input::Recording;
use std::{
    sync::{atomic::Ordering, Arc},
    thread,
    time::{Duration, Instant},
};

pub(super) fn run<E: Engine>(
    mut world: Box<Runtime<E>>,
    shared: Arc<SharedState>,
    mut recording: Recording,
) {
    let duration = Duration::from_secs_f64(1. / 60.);
    let mut deadline = Instant::now();
    let mut paused = shared.paused.load(Ordering::Acquire);
    let mut suspended = shared.suspended.load(Ordering::Acquire);
    'ticks: while !shared.shutdown.load(Ordering::Acquire) {
        let Ok(mut messages) = shared.commands.lock() else {
            break;
        };
        while let Some(message) = messages.pop() {
            match message {
                WorldMessage::Pause(on) => paused = on,
                WorldMessage::Suspend(on) => suspended = on,
                WorldMessage::Shutdown => break 'ticks,
                _ => {}
            }
        }
        drop(messages);
        if paused || suspended {
            thread::sleep(Duration::from_millis(8));
            deadline = Instant::now();
            continue;
        }
        let Ok(mut queue) = shared.publications.lock() else {
            break;
        };
        if queue.full() {
            drop(queue);
            thread::sleep(Duration::from_millis(1));
            deadline = Instant::now();
            continue;
        }
        let mut slot = queue.acquire_slot();
        drop(queue);
        let start = Instant::now();
        let result = recording.tick(&mut world, &mut slot);
        shared.record_tick_ns(start.elapsed().as_nanos() as u64, world.host_counters());
        let Ok(mut queue) = shared.publications.lock() else {
            break;
        };
        match result {
            Ok(tick) => {
                shared
                    .latest_tick_bits
                    .store(tick.to_bits(), Ordering::Release);
                queue.publish_slot(slot);
            }
            Err(error) => {
                queue.release_slot(slot);
                if let Ok(mut fault) = shared.fault.lock() {
                    *fault = Some(error);
                }
                break;
            }
        }
        drop(queue);
        if world.ticks < recording.preroll {
            deadline = Instant::now();
        } else {
            deadline += duration;
            let now = Instant::now();
            if deadline > now {
                thread::sleep(deadline - now);
            } else if now - deadline > duration * 8 {
                deadline = now;
            }
        }
    }
    world.close();
}
