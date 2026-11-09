#![cfg(target_os = "linux")]

#[path = "../src/app/x11_pointer.rs"]
mod x11_pointer;

use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};
use winit::{
    application::ApplicationHandler,
    event::{DeviceEvent, DeviceId, ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    platform::x11::EventLoopBuilderExtX11,
    window::{CursorGrabMode, Window, WindowId},
};

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[derive(Debug)]
enum AuditEvent {
    Done(bool),
    Timeout,
}

struct Audit {
    grab: Option<x11_pointer::Grab>,
    window: Option<Window>,
    position: [f64; 2],
    proxy: EventLoopProxy<AuditEvent>,
    phase: usize,
    counts: f64,
    clicks: usize,
    complete: bool,
    failed: bool,
}
impl Audit {
    fn grab_and_move(&mut self) {
        let window = self.window.as_ref().unwrap();
        window.set_cursor_grab(CursorGrabMode::Confined).unwrap();
        self.grab = x11_pointer::correct_grab(window).unwrap();
        let proxy = self.proxy.clone();
        thread::spawn(move || {
            // Separate requests keep the test independent of cooked-motion
            // coalescing. Raw displacement must still be conserved exactly.
            let mut ok = true;
            for _ in 0..8 {
                ok &= Command::new("xdotool")
                    .args(["mousemove_relative", "--", "100", "0"])
                    .status()
                    .is_ok_and(|status| status.success());
                thread::sleep(Duration::from_millis(10));
            }
            ok &= Command::new("xdotool")
                .args(["click", "1"])
                .status()
                .is_ok_and(|status| status.success());
            // The injector closes its X connection before this drain period;
            // the completion wake must follow the final server motion events.
            thread::sleep(Duration::from_millis(200));
            let _ = proxy.send_event(AuditEvent::Done(ok));
        });
    }
}
impl ApplicationHandler<AuditEvent> for Audit {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        let window = el
            .create_window(
                Window::default_attributes()
                    .with_title("Pointer input test")
                    .with_inner_size(winit::dpi::PhysicalSize::new(640, 480)),
            )
            .unwrap();
        self.window = Some(window);
        thread::spawn(|| {
            thread::sleep(Duration::from_millis(200));
            let _ = Command::new("xdotool")
                .args(["mousemove", "200", "200"])
                .status();
            let _ = Command::new("xdotool")
                .args([
                    "search",
                    "--sync",
                    "--name",
                    "Pointer input test",
                    "windowfocus",
                ])
                .status();
        });
    }
    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::Focused(true) if self.phase == 0 => {
                self.phase = 1;
                self.grab_and_move();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.position = [position.x, position.y];
                if let Some(grab) = &self.grab {
                    grab.keep_inside(self.window.as_ref().unwrap(), self.position, [640, 480])
                        .unwrap();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                ..
            } => self.clicks += 1,
            _ => {}
        }
    }
    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta: (dx, _) } = event {
            self.counts += dx;
        }
    }
    fn user_event(&mut self, el: &ActiveEventLoop, event: AuditEvent) {
        match event {
            AuditEvent::Done(ok) => {
                self.failed |= !ok;
                // A resume must replace the new winit owner-events grab again.
                // Release XI2 before clearing winit's tracked grab state.
                let window = self.window.as_ref().unwrap();
                self.grab.take();
                window.set_cursor_grab(CursorGrabMode::None).unwrap();
                if self.phase == 1 {
                    self.phase = 2;
                    self.grab_and_move();
                } else {
                    self.complete = true;
                    el.exit();
                }
            }
            AuditEvent::Timeout => el.exit(),
        }
    }
}

#[test]
fn confined_motion_is_not_duplicated_after_grab_or_resume() {
    if Command::new("xdotool").arg("--version").output().is_err() {
        eprintln!("skipping X11 pointer test: xdotool is unavailable");
        return;
    }
    let Ok(child) = Command::new("Xvfb")
        .args([
            "-displayfd",
            "1",
            "-screen",
            "0",
            "1280x720x24",
            "-nolisten",
            "tcp",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        eprintln!("skipping X11 pointer test: Xvfb is unavailable");
        return;
    };
    let mut server = Server(child);
    let mut display = String::new();
    BufReader::new(server.0.stdout.take().unwrap())
        .read_line(&mut display)
        .unwrap();
    let display: u16 = display.trim().parse().expect("Xvfb display number");
    // This test has its own process and server; no other window's input is moved.
    std::env::set_var("DISPLAY", format!(":{display}"));
    std::env::remove_var("WAYLAND_DISPLAY");
    std::env::remove_var("XAUTHORITY");
    let event_loop = EventLoop::<AuditEvent>::with_user_event()
        .with_any_thread(true)
        .with_x11()
        .build()
        .unwrap();
    let proxy = event_loop.create_proxy();
    let timeout = proxy.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(8));
        let _ = timeout.send_event(AuditEvent::Timeout);
    });
    let mut audit = Audit {
        grab: None,
        window: None,
        position: [0.; 2],
        proxy,
        phase: 0,
        counts: 0.,
        clicks: 0,
        complete: false,
        failed: false,
    };
    event_loop.run_app(&mut audit).unwrap();
    assert!(
        audit.complete && !audit.failed,
        "X11 injection did not complete"
    );
    assert_eq!(audit.counts, 1600., "two sets of eight 100-count reports");
    assert!((1.0..639.).contains(&audit.position[0]));
    assert!((1.0..479.).contains(&audit.position[1]));
    assert_eq!(
        audit.clicks, 2,
        "pointer buttons still reach the grabbed window"
    );
}
