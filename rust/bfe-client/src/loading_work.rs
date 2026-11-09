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

pub(crate) async fn frame() {
    struct Frame(bool);
    impl Future for Frame {
        type Output = ();
        fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
            if self.0 {
                Poll::Ready(())
            } else {
                self.0 = true;
                Poll::Pending
            }
        }
    }
    Frame(false).await
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
}
