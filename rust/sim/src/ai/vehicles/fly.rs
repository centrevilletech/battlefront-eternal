//! The flyer movement helper (src/ai/classic/vehicles/fly.js): navigation's
//! FlyGotoHelper (`crate::ai::nav::goto`, its goto, stop and tick) with what
//! the flyer agents and patterns add on top: an externally built path
//! (`follow_path`: the attack run, the climb out, the landing approach),
//! the landing request (`land`), and the path flags the flight steering
//! reads, kept in the control's steering bag.
//!
//! The helper reaches navigation through [`FlyHost`]: its unit (whose
//! `this.unit` attaches the helper's control to it, every time it is
//! read), navigation's own goto and stop (the JavaScript's `super`
//! calls), and the planner's path pool.

use three::Vector3;

use super::steering::steer_state;
use super::world::{Obj, World};

/// What the flyer helper asks of navigation.
pub trait FlyHost: World {
    /// The helper's `this.unit`: its controller's unit, the control
    /// attached to it when there is one.
    fn fly_unit(&mut self, helper: Obj) -> Option<Obj>;
    /// navigation's FlyGotoHelper goto(point, options) and stop().
    fn fly_super_goto(&mut self, helper: Obj, point: &Vector3, direct: bool);
    fn fly_super_stop(&mut self, helper: Obj);
    /// `this.nav?.planner` is there.
    fn fly_has_planner(&self, helper: Obj) -> bool;
    /// planner.cancel(control), planner.path(), path.prepend(x, y, z),
    /// path.release(), control.install(path, override, altitude).
    fn fly_planner_cancel(&mut self, helper: Obj);
    fn fly_path_new(&mut self, helper: Obj) -> u64;
    fn fly_path_prepend(&mut self, path: u64, x: f64, y: f64, z: f64) -> bool;
    fn fly_path_release(&mut self, path: u64);
    fn fly_install(&mut self, helper: Obj, path: u64, over: bool, altitude: bool);
    /// The helper's own fields followPath writes: destination, then
    /// failed, state and pending.
    fn fly_set_destination(&mut self, helper: Obj, destination: &Vector3);
    fn fly_set_followed(&mut self, helper: Obj, failed: bool, state: f64, pending: f64);
}

/// goto(point, options) (fly.js:115).
pub fn goto<H: FlyHost + ?Sized>(h: &mut H, helper: Obj, point: &Vector3, direct: bool) {
    set_flags(h, helper, false, false);
    h.fly_super_goto(helper, point, direct);
}

/// followPath(points, override, altitude) (fly.js:122): an externally
/// built path, built back to front as the 2004 game prepends its nodes.
pub fn follow_path<H: FlyHost + ?Sized>(h: &mut H, helper: Obj, points: &[[f64; 3]], over: bool, altitude: bool) -> bool {
    let unit = h.fly_unit(helper);
    if unit.is_none() || points.is_empty() {
        return false;
    }
    if !h.fly_has_planner(helper) {
        return false;
    }
    h.fly_planner_cancel(helper);
    let path = h.fly_path_new(helper);
    for i in (0..points.len()).rev() {
        let [x, y, z] = points[i];
        if !h.fly_path_prepend(path, x, y, z) {
            h.fly_path_release(path);
            return false;
        }
    }
    let last = points[points.len() - 1];
    h.fly_set_destination(helper, &Vector3::new(last[0], last[1], last[2]));
    h.fly_install(helper, path, over, altitude);
    set_flags(h, helper, over, altitude);
    h.fly_set_followed(helper, false, 4.0, 0.0);
    true
}

/// setFlags(override, altitude) (fly.js:147).
pub fn set_flags<H: FlyHost + ?Sized>(h: &mut H, helper: Obj, over: bool, altitude: bool) {
    let Some(unit) = h.fly_unit(helper) else { return };
    steer_state(h, unit, |st| {
        st.path_override = Some(over);
        st.path_altitude = Some(altitude);
    });
}

/// land(on) (fly.js:155).
pub fn land<H: FlyHost + ?Sized>(h: &mut H, helper: Obj, on: bool) {
    if let Some(unit) = h.fly_unit(helper) {
        steer_state(h, unit, |st| st.land = Some(on));
    }
}

/// stop() (fly.js:160).
pub fn stop<H: FlyHost + ?Sized>(h: &mut H, helper: Obj) {
    set_flags(h, helper, false, false);
    h.fly_super_stop(helper);
}
