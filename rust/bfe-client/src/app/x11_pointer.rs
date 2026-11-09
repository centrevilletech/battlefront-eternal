//! X11 pointer-grab ownership for unmodified raw mouse counts.

use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use std::sync::OnceLock;
use winit::window::Window;
use x11_dl::{xinput2, xlib};

struct Api {
    xlib: xlib::Xlib,
    xi: xinput2::XInput2,
}
static API: OnceLock<Result<Api, String>> = OnceLock::new();

fn api() -> Result<&'static Api, String> {
    API.get_or_init(|| {
        Ok(Api {
            xlib: xlib::Xlib::open().map_err(|error| error.to_string())?,
            xi: xinput2::XInput2::open().map_err(|error| error.to_string())?,
        })
    })
    .as_ref()
    .map_err(Clone::clone)
}

// These handles are lent by a live window on its event-loop thread. Other
// backends return None before loading Xlib or issuing any X11 requests.
fn handles(window: &Window) -> Result<Option<(*mut xlib::Display, xlib::Window)>, String> {
    let display = window.display_handle().map_err(|error| error.to_string())?;
    let handle = window.window_handle().map_err(|error| error.to_string())?;
    let (RawDisplayHandle::Xlib(display), RawWindowHandle::Xlib(handle)) =
        (display.as_raw(), handle.as_raw())
    else {
        return Ok(None);
    };
    let display = display
        .display
        .ok_or("X11 display connection is unavailable")?;
    Ok(Some((display.as_ptr().cast(), handle.window)))
}

fn pointer(api: &Api, display: *mut xlib::Display, window: xlib::Window) -> Result<i32, String> {
    let mut device = 0;
    // SAFETY: the caller holds the live winit window and its borrowed connection.
    if unsafe { (api.xi.XIGetClientPointer)(display, window, &mut device) } == 0 {
        Err("X11 client pointer is unavailable".into())
    } else {
        Ok(device)
    }
}

/// Replace winit's owner-events core grab on its existing connection. XI2
/// button events must stay selected because winit consumes them, not core buttons.
/// The tracked winit confinement state is retained for its normal lifecycle.
pub(crate) fn correct_grab(window: &Window) -> Result<Option<Grab>, String> {
    let Some((display, window)) = handles(window)? else {
        return Ok(None);
    };
    let api = api()?;
    let device = pointer(api, display, window)?;
    let mut bits = [0u8; (xinput2::XI_LASTEVENT as usize / 8) + 1];
    for event in [
        xinput2::XI_ButtonPress,
        xinput2::XI_ButtonRelease,
        xinput2::XI_Motion,
        xinput2::XI_Enter,
        xinput2::XI_Leave,
    ] {
        bits[event as usize / 8] |= 1 << (event as usize % 8);
    }
    let mut mask = xinput2::XIEventMask {
        deviceid: device,
        mask_len: bits.len() as _,
        mask: bits.as_mut_ptr(),
    };
    // SDL's X11_SetWindowMouseGrab correction uses owner_events=false: a root
    // raw-motion listener with owner events receives physical motion twice.
    // Filtering equal timestamps/deltas also loses valid high-rate input.
    // SAFETY: all requests use the live window's own connection on its thread.
    // Release only our core grab, then acquire XI2 with exclusive delivery.
    let status = unsafe {
        (api.xlib.XUngrabPointer)(display, xlib::CurrentTime);
        (api.xi.XIGrabDevice)(
            display,
            device,
            window,
            xlib::CurrentTime,
            0,
            xlib::GrabModeAsync,
            xlib::GrabModeAsync,
            xlib::False,
            &mut mask,
        )
    };
    if status == xlib::GrabSuccess {
        Ok(Some(Grab { display, device }))
    } else {
        Err(format!("X11 pointer grab failed with status {status}"))
    }
}

/// Owned XI2 grab. Keep this before the window field so it is released while
/// the borrowed display connection is still alive, including on application exit.
pub(crate) struct Grab {
    display: *mut xlib::Display,
    device: i32,
}

impl Grab {
    /// XI2 has no confine-to rectangle. Recenter the hidden cursor at an edge
    /// so windowed relative input cannot run out of desktop space. A warp emits
    /// cooked motion only; all physical raw counts remain untouched.
    pub(crate) fn keep_inside(
        &self,
        window: &Window,
        position: [f64; 2],
        size: [i32; 2],
    ) -> Result<(), String> {
        if size[0] > 2
            && size[1] > 2
            && (position[0] <= 1.
                || position[1] <= 1.
                || position[0] >= f64::from(size[0] - 1)
                || position[1] >= f64::from(size[1] - 1))
        {
            window
                .set_cursor_position(winit::dpi::PhysicalPosition::new(size[0] / 2, size[1] / 2))
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

impl Drop for Grab {
    fn drop(&mut self) {
        let Ok(api) = api() else { return };
        // SAFETY: the owner drops this token before its winit window. Release
        // the exact master device acquired, even if the client pointer changes.
        unsafe {
            (api.xi.XIUngrabDevice)(self.display, self.device, xlib::CurrentTime);
            (api.xlib.XFlush)(self.display);
        }
    }
}
