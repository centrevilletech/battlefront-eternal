//! VIDEO OPTIONS RESOLUTION: the game's full-screen output at one of the monitor's own modes, as the 2004
//! game offered it, instead of the desktop's mode with a small scene stretched over it. On an old integrated
//! GPU drawing 1280x720 to a 1280x720 display mode is about 4 ms a frame cheaper than stretching it to 1080p
//! (our stretch, and the desktop compositor's copy of a smaller window).
//!
//! - X11 and Windows switch the mode through winit's exclusive full screen. Wayland gives a client no mode
//!   switching: there no modes are listed and the option offers only DESKTOP (RENDER SCALE still draws the
//!   scene smaller, and the compositor scales nothing extra). macOS is treated the same: a Mac game keeps the
//!   desktop's mode in its full screen Space (Mission Control, Command-Tab and the menu bar keep working, which
//!   a captured display with another mode takes away) and draws its scene smaller instead.
//! - The desktop mode comes back whenever the game leaves full screen, loses focus, or exits; Windows itself
//!   restores it if the process dies. On X11 nothing does that for a crashed client, so while a mode is set a
//!   small watcher (`sh` reading a pipe from the game) puts the desktop mode back with `xrandr` when the pipe
//!   closes, which happens however the game ends.
//! - A new choice must be confirmed within 15 seconds (OptionsController::display_trial) or it reverts, so a
//!   mode the monitor cannot show never leaves the player stuck.
use std::process::{Child, Command, Stdio};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Fullscreen, Window};

/// `"WxH"` as (width, height); `"desktop"` and anything else is `None`.
pub fn parse(choice: &str) -> Option<(u32, u32)> {
    let (w, h) = choice.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

/// Whether the full screen always keeps the desktop's own mode (Wayland, macOS).
fn wayland(window: &Window) -> bool {
    cfg!(target_os = "macos") || window.window_handle().is_ok_and(|h| matches!(h.as_raw(), RawWindowHandle::Wayland(_)))
}

/// The monitor's modes. winit asserts that Windows reports each mode's refresh rate; Wine (and some
/// virtual or remote displays) leave it out, and that assertion used to end the game at start. Such a
/// monitor offers no modes (the option keeps DESKTOP).
fn video_modes(monitor: &winit::monitor::MonitorHandle) -> Vec<winit::monitor::VideoModeHandle> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| monitor.video_modes().collect())).unwrap_or_default()
}

/// The monitor's modes the option offers, largest first (none where the platform cannot switch modes).
pub fn modes(window: &Window) -> Vec<(u32, u32)> {
    if wayland(window) {
        return vec![];
    }
    let Some(monitor) = window.current_monitor() else { return vec![] };
    let mut modes: Vec<(u32, u32)> = video_modes(&monitor)
        .into_iter()
        .map(|m| (m.size().width, m.size().height))
        .filter(|&(w, h)| w >= 640 && h >= 480)
        .collect();
    modes.sort_by(|a, b| (b.0 * b.1).cmp(&(a.0 * a.1)).then(b.0.cmp(&a.0)));
    modes.dedup();
    modes
}

/// The desktop's own mode (the monitor's current size before any change).
pub fn desktop(window: &Window) -> (u32, u32) {
    window.current_monitor().map_or((0, 0), |m| (m.size().width, m.size().height))
}

/// The full screen for a choice: the monitor's mode of that size (the desktop's refresh rate if it has it,
/// else the highest), or the desktop's own mode (borderless).
fn fullscreen_for(window: &Window, choice: &str) -> Fullscreen {
    let Some((w, h)) = parse(choice) else { return Fullscreen::Borderless(None) };
    let Some(monitor) = window.current_monitor() else { return Fullscreen::Borderless(None) };
    if wayland(window) {
        return Fullscreen::Borderless(None);
    }
    let desktop = monitor.refresh_rate_millihertz().unwrap_or(0);
    video_modes(&monitor)
        .into_iter()
        .filter(|m| m.size().width == w && m.size().height == h)
        .max_by_key(|m| (m.refresh_rate_millihertz() == desktop, m.refresh_rate_millihertz()))
        .map_or(Fullscreen::Borderless(None), Fullscreen::Exclusive)
}

#[derive(Default)]
pub struct DisplayMode {
    /// The choice in effect on the display ("desktop" until a mode is set).
    applied: String,
    /// X11: the watcher that restores the desktop mode if the game ends without doing it.
    watcher: Option<Child>,
}

impl DisplayMode {
    /// Every frame: the chosen mode while the game is full screen and focused, the desktop's otherwise.
    pub fn update(&mut self, window: &Window, fullscreen: bool, focused: bool, choice: &str) {
        let want = if fullscreen && focused && parse(choice).is_some() && !wayland(window) { choice } else { "desktop" };
        let applied = if self.applied.is_empty() { "desktop" } else { self.applied.as_str() };
        if want == applied {
            return;
        }
        if want != "desktop" && self.watcher.is_none() {
            self.watcher = watcher(window);
        }
        // Leaving full screen (F11) already gave the desktop its mode back (winit restores it).
        if fullscreen {
            window.set_fullscreen(Some(fullscreen_for(window, want)));
        }
        eprintln!("display mode: {want}");
        self.applied = want.to_owned();
        if want == "desktop" {
            self.stop_watcher();
        }
    }

    /// Puts the desktop's mode back now (exit, drop).
    pub fn restore(&mut self, window: &Window) {
        if !self.applied.is_empty() && self.applied != "desktop" {
            window.set_fullscreen(Some(Fullscreen::Borderless(None)));
            self.applied = "desktop".into();
        }
        self.stop_watcher();
    }

    fn stop_watcher(&mut self) {
        if let Some(mut child) = self.watcher.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// X11 only: `sh` waits on its standard input (a pipe the game holds) and then sets the output back to the
/// desktop's mode with `xrandr`. A normal exit kills it after restoring; a crash closes the pipe and it runs.
fn watcher(window: &Window) -> Option<Child> {
    if !window.window_handle().is_ok_and(|h| matches!(h.as_raw(), RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_))) {
        return None;
    }
    let monitor = window.current_monitor()?;
    let name = monitor.name()?;
    let size = monitor.size();
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.') {
        return None;
    }
    let script = format!("read _; exec xrandr --output {name} --mode {}x{}", size.width, size.height);
    Command::new("sh")
        .arg("-c")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn choices_read_as_width_and_height() {
        assert_eq!(super::parse("1280x720"), Some((1280, 720)));
        assert_eq!(super::parse("desktop"), None);
        assert_eq!(super::parse("12x"), None);
    }
}
