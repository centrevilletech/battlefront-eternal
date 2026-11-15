//! Cold scene admission cooperates with the window's frame loop. Numerical
//! gameplay stays on its world thread; GL objects stay on the event thread.
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, OnceLock},
    task::{Context, Poll, Wake, Waker},
};

struct Noop;
impl Wake for Noop {
    fn wake(self: Arc<Self>) {}
}

pub(crate) fn poll<T>(future: Pin<&mut dyn Future<Output = T>>) -> Poll<T> {
    static WAKER: OnceLock<Waker> = OnceLock::new();
    let waker = WAKER.get_or_init(|| Waker::from(Arc::new(Noop)));
    future.poll(&mut Context::from_waker(waker))
}

/// Legacy synchronous hosts drive the same admission steps to completion.
/// The standalone window instead polls once between loading-screen frames.
pub(crate) fn finish<T>(future: impl Future<Output = T>) -> T {
    let mut future = Box::pin(future);
    loop {
        if let Poll::Ready(value) = poll(future.as_mut()) {
            return value;
        }
    }
}

/// Yields to the window for one frame. The window logs a step that held it
/// too long with the place it resumed from (`resumed_at`).
#[track_caller]
pub(crate) fn frame() -> Frame {
    Frame { yielded: false, at: std::panic::Location::caller() }
}

pub(crate) struct Frame {
    yielded: bool,
    at: &'static std::panic::Location<'static>,
}

impl Future for Frame {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        if self.yielded {
            RESUMED.with(|r| r.set(Some(self.at)));
            Poll::Ready(())
        } else {
            self.yielded = true;
            Poll::Pending
        }
    }
}

thread_local! {
    static RESUMED: std::cell::Cell<Option<&'static std::panic::Location<'static>>> = const { std::cell::Cell::new(None) };
}

thread_local! {
    static IDLE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Yields to the window for one frame while waiting on another thread, so
/// the window does not spend the rest of its frame budget polling again.
#[track_caller]
pub(crate) fn idle() -> Frame {
    IDLE.with(|i| i.set(true));
    frame()
}

/// Whether the last step ended waiting (`idle`); clears the mark.
pub(crate) fn take_idle() -> bool {
    IDLE.with(|i| i.replace(false))
}

/// Where the last step started: the frame() it resumed from, or "start".
pub(crate) fn resumed_at() -> String {
    RESUMED.with(|r| r.get()).map_or_else(|| "start".into(), |at| format!("{}:{}", at.file(), at.line()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn admission_yields_to_two_distinct_window_frames() {
        let mut future = Box::pin(async {
            frame().await;
            frame().await;
            7
        });
        assert!(poll(future.as_mut()).is_pending());
        assert!(poll(future.as_mut()).is_pending());
        assert_eq!(poll(future.as_mut()), Poll::Ready(7));
    }
    #[test]
    fn a_waiting_step_is_marked_idle() {
        let mut future = Box::pin(async {
            frame().await;
            idle().await;
        });
        take_idle();
        assert!(poll(future.as_mut()).is_pending());
        assert!(!take_idle());
        assert!(poll(future.as_mut()).is_pending());
        assert!(take_idle());
    }
}
