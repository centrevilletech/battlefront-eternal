//! What the navigation knows about walking from a bot to an enemy.
//!
//! The hunt asks [`route`] about an enemy life when it first considers that
//! enemy for a bot life (at most [`super::controller`]'s few nearest new
//! pairs per choice) and again when the bot's approach clock runs out, and
//! keeps the answer for that pair. A walkable route ranks the enemy by its
//! length and sets the effort budget. A confirmed obstruction excludes the
//! pair for the search round. A declined query uses straight distance.
//!
//! A navigation provider supplies [`route`] for the soldier ground class
//! ([`SOLDIER_MASK`]) and the asking soldier’s admitted traversal radius. A missing navigation graph returns [`Route::Unknown`].

use crate::ai::control::world::{AiWorld, Characters};
use crate::ai::nav::{math::Vec3, units::traversal_radius};
use crate::ai::owned::{arsenals::Mounted, weapons::Changes, world::{Services, World}};

/// The ground class of a soldier on foot, the one traversal bit a route
/// question asks about.
pub const SOLDIER_MASK: u32 = 1;

/// A walk between two points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Route {
    /// The navigation cannot tell.
    Unknown,
    /// The navigation confirmed that there is no walkable route.
    NoRoute,
    /// A walkable route of this length in metres.
    Reachable { distance: f64 },
}

/// The navigation's answer for a soldier walking from `from` to `to`.
pub(super) fn route<C: Changes + ?Sized, M: Mounted, S: Services<C, M>>(
    world: &mut World<'_, '_, C, M, S>, character: usize, from: Vec3, to: Vec3,
) -> Route {
    let Some(body) = world.character_unit(character) else { return Route::Unknown };
    let soldier = world.unit(body);
    let radius = soldier.nav_radius.unwrap_or_else(|| traversal_radius(&soldier.nav_unit()));
    #[cfg(test)]
    if let Some(answer) = TEST_ROUTES.with(|r| r.get()).map(|f| f(from, to, radius)) {
        TEST_ROUTE_CALLS.with(|n| n.set(n.get().wrapping_add(1)));
        return answer;
    }
    navigation_route(world, from, to, radius)
}

fn navigation_route<C: Changes + ?Sized, M: Mounted, S: Services<C, M>>(
    world: &mut World<'_, '_, C, M, S>, from: Vec3, to: Vec3, radius: f64,
) -> Route {
    use crate::ai::nav::route::RouteStatus;
    let answer = world.nav_route_distance(from, to, radius, SOLDIER_MASK);
    match answer.status {
        RouteStatus::Reachable => Route::Reachable { distance: answer.distance },
        RouteStatus::Unreachable => Route::NoRoute,
        RouteStatus::Unknown => Route::Unknown,
    }
}

/// A test's answer for every route question.
#[cfg(test)]
type Routes = Option<fn(Vec3, Vec3, f64) -> Route>;

#[cfg(test)]
thread_local! {
    static TEST_ROUTES: std::cell::Cell<Routes> = const { std::cell::Cell::new(None) };
    static TEST_ROUTE_CALLS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Answer route questions on this thread with `routes` (tests only).
#[cfg(test)]
pub(crate) fn set_test_routes(routes: Routes) { TEST_ROUTES.with(|r| r.set(routes)); }

/// Route questions answered by a test's routes on this thread.
#[cfg(test)]
pub(crate) fn test_route_calls() -> u32 { TEST_ROUTE_CALLS.with(std::cell::Cell::get) }
