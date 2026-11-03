//! Serving a frame's boundary from its capture while a Rust port replays it:
//! the port asks for each call and each draw as it makes them, and the log
//! hands back what the JavaScript got, provided the port asks for the same
//! thing at the same point. A port's boundary trait (plan section 2.2,
//! item 8) is implemented over a `Log` in its replay tests.

use crate::diff::diff;
use crate::frame::{Call, Draw, Event, Frame};
use crate::value::{same_number, show_number, V};

/// A replay that parted from the capture: where, and how.
#[derive(Clone, Debug, PartialEq)]
pub struct Mismatch(pub String);

impl std::fmt::Display for Mismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Mismatch {}

/// A frame's events, served in order.
pub struct Log<'f> {
    pub frame: &'f Frame,
    next: usize,
}

fn describe(event: Option<&Event>) -> String {
    match event {
        None => "nothing (the frame made no more calls or draws)".into(),
        Some(Event::Draw(d)) => format!("a draw from {} ({})", d.stream, show_number(d.value)),
        Some(Event::Call(c)) => format!("a call to {}", c.name),
    }
}

impl<'f> Log<'f> {
    pub fn new(frame: &'f Frame) -> Log<'f> {
        Log { frame, next: 0 }
    }

    /// What comes next.
    pub fn peek(&self) -> Option<&'f Event> {
        self.frame.events.get(self.next)
    }

    fn mismatch(&self, wanted: String) -> Mismatch {
        Mismatch(format!("{} event {}: the port {wanted}, the capture has {}", self.frame.place(), self.next, describe(self.peek())))
    }

    /// The next event must be a draw from `stream` (a stream's label, or a
    /// prefix ending in ':' such as "seeded:"): the number the JavaScript drew.
    pub fn draw(&mut self, stream: &str) -> Result<f64, Mismatch> {
        match self.peek() {
            Some(Event::Draw(Draw { stream: s, value })) if s == stream || (stream.ends_with(':') && s.starts_with(stream)) => {
                self.next += 1;
                Ok(*value)
            }
            _ => Err(self.mismatch(format!("draws from {stream}"))),
        }
    }

    /// The next event must be a call to `name`: the call, for the port to
    /// check its arguments (`check_*`) and take its result.
    pub fn call(&mut self, name: &str) -> Result<&'f Call, Mismatch> {
        match self.peek() {
            Some(Event::Call(call)) if call.name == name => {
                self.next += 1;
                Ok(call)
            }
            _ => Err(self.mismatch(format!("calls {name}"))),
        }
    }

    /// Every event the frame made has been asked for.
    pub fn finish(&self) -> Result<(), Mismatch> {
        if self.next == self.frame.events.len() {
            Ok(())
        } else {
            Err(Mismatch(format!("{}: the port stopped after {} events; the capture has {} more, next {}", self.frame.place(), self.next, self.frame.events.len() - self.next, describe(self.peek()))))
        }
    }
}

/// A number the port computed against the capture's, bit for bit (as `Object.is`).
pub fn check_num(what: &str, captured: V, mine: f64) -> Result<(), Mismatch> {
    match captured.opt_num() {
        Some(x) if same_number(x, mine) => Ok(()),
        _ => Err(Mismatch(format!("{what}: the capture has {}, the port {}", captured.show(), show_number(mine)))),
    }
}

/// A Vector3 the port computed against the capture's, every component bit for bit.
pub fn check_vec3(what: &str, captured: V, mine: [f64; 3]) -> Result<(), Mismatch> {
    match captured.vec3() {
        Some(v) if v.iter().zip(mine).all(|(a, b)| same_number(*a, b)) => Ok(()),
        _ => Err(Mismatch(format!(
            "{what}: the capture has {}, the port ({}, {}, {})",
            captured.show(),
            show_number(mine[0]),
            show_number(mine[1]),
            show_number(mine[2])
        ))),
    }
}

/// Whether the capture's value is null (or undefined) where the port has none.
pub fn check_nullish(what: &str, captured: V, mine_is_nullish: bool) -> Result<(), Mismatch> {
    if captured.is_nullish() == mine_is_nullish {
        Ok(())
    } else {
        Err(Mismatch(format!("{what}: the capture has {}, the port {}", captured.show(), if mine_is_nullish { "none" } else { "one" })))
    }
}

/// A boolean the port computed against the capture's.
pub fn check_bool(what: &str, captured: V, mine: bool) -> Result<(), Mismatch> {
    match captured.opt_bool() {
        Some(b) if b == mine => Ok(()),
        _ => Err(Mismatch(format!("{what}: the capture has {}, the port {mine}", captured.show()))),
    }
}

/// A whole value the port built (Arena builders) against the capture's (`diff`).
pub fn check_value(what: &str, captured: V, mine: V) -> Result<(), Mismatch> {
    match diff(captured, mine) {
        None => Ok(()),
        Some(d) => Err(Mismatch(format!("{what}: {d} (capture | port)"))),
    }
}
