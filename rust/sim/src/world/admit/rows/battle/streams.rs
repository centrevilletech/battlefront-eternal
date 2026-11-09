//! The F3 random-stream registry (f3-random.js F3Random.register). Every
//! owner that hands the simulation a random callback registers it first;
//! the id is the registration order, and a callback already registered
//! answers its earlier id. The host keys a callback by its generator and
//! stream name, so the same `RandomStreams` draw registered twice is one
//! stream, while two bare generators with equal seeds stay two. The native
//! twin keys by text: `streams:<name>` for a named draw of the battle's
//! `RandomStreams(SEED)`, `classic:<which>` for a bare generator.
//!
//! The ids matter bit for bit: F4Tail's setup, the AI's two streams and
//! both player bindings carry them, so each owner registers at the seam the
//! JavaScript did (game.js 1392 configure at attachF3's end, ai-world-load.js
//! 27 at ai_world_begin, f4-tail.js 44, f3-factory.js 75 at each bindPlayer).

/// `streams.draw("gameplay")`: combat's chunks, drops and the player's
/// gameplay draws (game.js 194).
pub const GAMEPLAY: &str = "streams:gameplay";
/// `streams.draw('grenade-spin')` (game.js 1392).
pub const GRENADE_SPIN: &str = "streams:grenade-spin";
/// `streams.draw("seat-weapons")` (game.js 197).
pub const SEAT_WEAPONS: &str = "streams:seat-weapons";
/// `streams.draw("player-weapons")`, the local human's spread (game.js 1582).
pub const PLAYER_WEAPONS: &str = "streams:player-weapons";
/// `game.randomStream('network-weapons')`, a connection's spread
/// (multiplayer-world.js 37).
pub const NETWORK_WEAPONS: &str = "streams:network-weapons";
/// `new ClassicRandom(SEED)` (game.js 1427): the AI's first stream and the
/// tail's `random`, the same generator through `uniformRandom`.
pub const WORLD_RANDOM: &str = "classic:world";
/// The AI's second bare generator (ai-world-load.js `world.randomB`).
pub const WORLD_RANDOM_B: &str = "classic:world-b";

/// The registered keys in id order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Streams { keys: Vec<String> }
impl Streams {
    /// F3Random.register: the existing id, else the next one.
    pub fn register(&mut self, key: &str) -> u32 {
        if let Some(id) = self.registered(key) { return id; }
        self.keys.push(key.to_string());
        (self.keys.len() - 1) as u32
    }
    /// The id a key already has, without registering it.
    pub fn registered(&self, key: &str) -> Option<u32> {
        self.keys.iter().position(|k| k == key).map(|i| i as u32)
    }
    pub fn len(&self) -> usize { self.keys.len() }
    pub fn is_empty(&self) -> bool { self.keys.is_empty() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registration_order_gives_ids_and_repeats_answer_the_first() {
        let mut streams = Streams::default();
        // game.js 1392: configure(gameplay, grenade-spin, gameplay, seat).
        assert_eq!(streams.register(GAMEPLAY), 0);
        assert_eq!(streams.register(GRENADE_SPIN), 1);
        assert_eq!(streams.register(GAMEPLAY), 0);
        assert_eq!(streams.register(SEAT_WEAPONS), 2);
        // ai-world-load.js: streamA, streamB; then the tail finds streamA again.
        assert_eq!(streams.register(WORLD_RANDOM), 3);
        assert_eq!(streams.register(WORLD_RANDOM_B), 4);
        assert_eq!(streams.register(WORLD_RANDOM), 3);
        assert_eq!(streams.register(PLAYER_WEAPONS), 5);
        assert_eq!(streams.register(NETWORK_WEAPONS), 6);
        assert_eq!(streams.registered("streams:never"), None);
        assert_eq!(streams.len(), 7);
    }
}
