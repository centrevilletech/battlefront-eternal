//! The game a map, its rules and its interface belong to.
//!
//! Every map, mission, side and class id names its family: `swbf1/hot1` is Hoth
//! in Star Wars: Battlefront (2004), `swbf2/hot1` is Hoth in Battlefront II
//! (2005). A bare id means `swbf1`, so ids written before the family existed
//! keep their meaning. See `MULTIPLAYER.md` ("Two games from day one") and
//! `docs/battlefront-ii.md`.

use std::fmt;

use serde::{Deserialize, Serialize};

#[cfg(bfe_dev_online)]
mod dev_online;
pub mod owner;
pub mod swbf2;
pub mod tag;
pub use tag::FamilyTag;

/// The mode of every 2004 battle, and the mode an entry that names none plays.
pub const DEFAULT_MODE: &str = "conquest";

/// The 2005 game's availability gate: false until the game is switched on.
/// [`Gate::BUILT`] follows it, so setting it opens the 2005 game everywhere at
/// once.
pub const SWBF2_AVAILABLE: bool = false;

/// Which games a [`Gate`] lets a build play.
pub trait Availability: Sync {
    /// Whether a battle of `family` can be played.
    fn plays(&self, family: GameFamily) -> bool;
    /// A name for logs and test failures.
    fn name(&self) -> &'static str;
}

/// What this build was compiled to play: every 2004 battle, and a 2005 battle
/// only when [`SWBF2_AVAILABLE`] is set.
struct Built;

impl Availability for Built {
    fn plays(&self, family: GameFamily) -> bool {
        match family {
            GameFamily::Swbf1 => true,
            GameFamily::Swbf2 => SWBF2_AVAILABLE,
        }
    }
    fn name(&self) -> &'static str {
        "built"
    }
}

/// The availability gate as a value. Each check of a battle's game takes the
/// gate of whoever started that work: a server's from its configuration, a
/// client's from the application, a simulation admission's from its options.
/// Shipped code only ever holds [`Gate::BUILT`]; no shipped program contains
/// a gate that opens the 2005 game (see `docs/network-two-games.md`, "The
/// gate").
#[derive(Clone, Copy)]
pub struct Gate(&'static dyn Availability);

impl Gate {
    /// The gate this build was compiled with.
    pub const BUILT: Gate = Gate(&Built);

    /// A gate that follows `availability`.
    pub const fn new(availability: &'static dyn Availability) -> Self {
        Self(availability)
    }

    /// Whether a battle of `family` can be played.
    #[inline]
    pub fn plays(self, family: GameFamily) -> bool {
        self.0.plays(family)
    }

    /// Verifies that a battle of `family` can be played, returning
    /// [`UnavailableFamily`] with the one message per family when it cannot.
    #[inline]
    pub fn require(self, family: GameFamily) -> Result<(), UnavailableFamily> {
        if self.plays(family) {
            Ok(())
        } else {
            Err(UnavailableFamily::not_available(family))
        }
    }
}

/// The gate a program plays online under: the dedicated server's for the
/// battles it hosts, the client's for the battles it joins. It is
/// [`Gate::BUILT`]; only a developer build reads the developer switch as well
/// (`--cfg bfe_dev_online`, which no release build sets).
pub fn online_gate() -> Gate {
    #[cfg(bfe_dev_online)]
    if let Some(gate) = dev_online::gate() {
        return gate;
    }
    Gate::BUILT
}

impl Default for Gate {
    fn default() -> Self {
        Self::BUILT
    }
}

/// Two gates are equal when they let a build play the same games.
impl PartialEq for Gate {
    fn eq(&self, other: &Self) -> bool {
        GameFamily::ALL.into_iter().all(|family| self.plays(family) == other.plays(family))
    }
}

impl Eq for Gate {}

impl fmt::Debug for Gate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Gate").field(&self.0.name()).finish()
    }
}

/// `swbf1` (2004) or `swbf2` (2005).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum GameFamily {
    /// Star Wars: Battlefront (2004).
    #[default]
    #[serde(rename = "swbf1")]
    Swbf1,
    /// Star Wars: Battlefront II (2005).
    #[serde(rename = "swbf2")]
    Swbf2,
}

impl GameFamily {
    /// Every family, in id order.
    pub const ALL: [GameFamily; 2] = [GameFamily::Swbf1, GameFamily::Swbf2];

    /// Returns `true` when `self` is the default 2004 family (`Swbf1`).
    /// Used with `#[serde(skip_serializing_if = "GameFamily::is_default")]`.
    #[inline]
    pub const fn is_default(&self) -> bool {
        matches!(self, GameFamily::Swbf1)
    }

    /// The family's id: `"swbf1"` or `"swbf2"`. Also its folder name under
    /// the prepared content root.
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            GameFamily::Swbf1 => "swbf1",
            GameFamily::Swbf2 => "swbf2",
        }
    }

    /// Human-readable game name (`"Battlefront (2004)"` or `"Battlefront II (2005)"`).
    #[inline]
    pub const fn title(self) -> &'static str {
        match self {
            GameFamily::Swbf1 => "Battlefront (2004)",
            GameFamily::Swbf2 => "Battlefront II (2005)",
        }
    }

    /// Whether this build, as compiled, can play battles belonging to
    /// `self`: [`Gate::BUILT`]. The interface (shell picker, battle UI) asks
    /// this; work that a caller starts takes that caller's [`Gate`].
    #[inline]
    pub fn is_available(self) -> bool {
        Gate::BUILT.plays(self)
    }

    /// [`Gate::require`] of [`Gate::BUILT`].
    #[inline]
    pub fn require_available(self) -> Result<(), UnavailableFamily> {
        Gate::BUILT.require(self)
    }

    /// Reads a family id, ignoring case and surrounding space.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        Self::ALL
            .into_iter()
            .find(|family| text.eq_ignore_ascii_case(family.as_str()))
    }

    /// Splits `"swbf2/hot1"` into the family and `"hot1"`. An id with no
    /// family prefix is a `swbf1` id and comes back whole. A prefix that is
    /// not a family (`"mods/hot1"`) is not split either.
    pub fn split_id(id: &str) -> (Self, &str) {
        match id.split_once('/') {
            Some((prefix, rest)) => match Self::parse(prefix) {
                Some(family) => (family, rest),
                None => (GameFamily::Swbf1, id),
            },
            None => (GameFamily::Swbf1, id),
        }
    }

    /// Joins a family and a bare id into the full id, `"swbf2/hot1"`.
    pub fn qualify(self, id: &str) -> String {
        format!("{}/{}", self.as_str(), id)
    }

    /// The folder holding this game's prepared `maps/`, `sides/` and
    /// `common/` under a content root: the root itself for the 2004 game (the
    /// released flat layout), `swbf2/.content` beneath it for the 2005 game.
    pub fn content_dir(self, root: &std::path::Path) -> std::path::PathBuf {
        match self {
            GameFamily::Swbf1 => root.to_path_buf(),
            GameFamily::Swbf2 => root.join(self.as_str()).join(".content"),
        }
    }
}

/// The game and mode a battle plays, as the network names them. A 2004
/// battle is always Conquest and is written with neither field, so every
/// message it appears in keeps its released keys; a 2005 battle names both.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub family: GameFamily,
    pub mode: String,
}

/// The 2004 game's only mode.
pub const CONQUEST: &str = DEFAULT_MODE;

impl Selection {
    /// The 2004 game's Conquest battle: the selection of every message that
    /// carries no family.
    pub fn swbf1() -> Self {
        Self { family: GameFamily::Swbf1, mode: CONQUEST.to_string() }
    }

    /// A 2005 battle of `mode`, a canonical token.
    pub fn swbf2(mode: &str) -> Option<Self> {
        mode_token(mode).then(|| Self { family: GameFamily::Swbf2, mode: mode.to_string() })
    }

    /// Reads the `family` and `mode` fields of a message. Both absent is the
    /// 2004 game; a 2005 battle names `"swbf2"` and a canonical mode. Anything
    /// else, including an explicit `"swbf1"`, is malformed: no writer sends it.
    pub fn read(family: Option<&str>, mode: Option<&str>) -> Result<Self, &'static str> {
        match (family, mode) {
            (None, None) => Ok(Self::swbf1()),
            (Some("swbf2"), Some(mode)) => Self::swbf2(mode).ok_or("invalid battle mode"),
            _ => Err("invalid battle game"),
        }
    }

    /// The fields a message carries for this selection: none for the 2004
    /// game, `family` and `mode` for the 2005 game.
    pub fn fields(&self) -> Option<(&'static str, &str)> {
        (self.family != GameFamily::Swbf1).then(|| (self.family.as_str(), self.mode.as_str()))
    }
}

/// A canonical mode token: 1 to 32 lowercase ASCII letters, digits or
/// underscores.
pub fn mode_token(mode: &str) -> bool {
    (1..=32).contains(&mode.len()) && mode.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

impl fmt::Display for GameFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned when a caller asks to play a [`GameFamily`] that this build
/// does not support yet (today [`GameFamily::Swbf2`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnavailableFamily {
    /// The unavailable game family that was requested.
    pub family: GameFamily,
}

impl UnavailableFamily {
    /// Constructs an availability error for `family`.
    #[inline]
    pub const fn not_available(family: GameFamily) -> Self {
        Self { family }
    }
}

impl UnavailableFamily {
    /// The refusal, as one fixed text per family.
    pub const fn message(&self) -> &'static str {
        match self.family {
            GameFamily::Swbf1 => "Battlefront (2004) is not available in this build",
            GameFamily::Swbf2 => "Battlefront II (2005) is not available in this build",
        }
    }
}

impl fmt::Display for UnavailableFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for UnavailableFamily {}

#[cfg(test)]
mod tests {
    use super::{mode_token, Selection, GameFamily as F};

    #[test]
    fn selections_write_nothing_for_the_2004_game() {
        assert_eq!(Selection::read(None, None), Ok(Selection::swbf1()));
        assert_eq!(Selection::swbf1().fields(), None);
        let eli = Selection::read(Some("swbf2"), Some("eli")).unwrap();
        assert_eq!((eli.family, eli.fields()), (F::Swbf2, Some(("swbf2", "eli"))));
        for (family, mode) in [(Some("swbf1"), None), (Some("swbf1"), Some("conquest")), (Some("swbf2"), None),
            (None, Some("eli")), (Some("SWBF2"), Some("eli")), (Some("swbf2"), Some("Eli")), (Some("swbf2"), Some(""))] {
            assert!(Selection::read(family, mode).is_err(), "{family:?} {mode:?}");
        }
        assert!(mode_token("ass") && mode_token("hero_1") && !mode_token(&"a".repeat(33)) && !mode_token("a-b"));
    }

    use super::{Availability, Gate, GameFamily, UnavailableFamily};

    #[test]
    fn ids_round_trip() {
        for family in GameFamily::ALL {
            assert_eq!(GameFamily::parse(family.as_str()), Some(family));
            assert_eq!(family.to_string(), family.as_str());
            let text = serde_json::to_string(&family).unwrap();
            assert_eq!(text, format!("\"{}\"", family.as_str()));
            assert_eq!(serde_json::from_str::<GameFamily>(&text).unwrap(), family);
        }
        assert_eq!(GameFamily::parse(" SWBF2 "), Some(GameFamily::Swbf2));
        assert_eq!(GameFamily::parse("mods"), None);
    }

    #[test]
    fn a_2005_battle_reads_its_content_by_its_bare_map_in_its_own_folder() {
        use super::swbf2::content_query;
        assert_eq!(content_query("?map=swbf2/tat2&side=all&mode=eli&match=0a", "tat2").as_deref(), Some("?map=tat2&side=all&mode=eli&match=0a&era=gcw"));
        assert_eq!(content_query("?side=rep&map=%73wbf2%2Fkam9&seed=1", "kam9").as_deref(), Some("?side=rep&map=kam9&seed=1&era=cw"));
        // A named era, even an empty one, and a side of no era stay as written.
        assert_eq!(content_query("?map=swbf2/tat2&era=cw&side=all", "tat2").as_deref(), Some("?map=tat2&era=cw&side=all"));
        assert_eq!(content_query("?map=swbf2/tat2&side=all&%65ra=", "tat2").as_deref(), Some("?map=tat2&side=all&%65ra="));
        assert_eq!(content_query("map=swbf2/tat2&side=local", "tat2").as_deref(), Some("map=tat2&side=local"));
        assert_eq!(content_query("?side=all", "tat2"), None);
        let root = std::path::Path::new("/content");
        assert_eq!(GameFamily::Swbf1.content_dir(root), root);
        assert_eq!(GameFamily::Swbf2.content_dir(root), root.join("swbf2").join(".content"));
    }

    #[test]
    fn a_bare_id_is_the_2004_game() {
        assert_eq!(GameFamily::split_id("hot1"), (GameFamily::Swbf1, "hot1"));
        assert_eq!(GameFamily::split_id("swbf1/hot1"), (GameFamily::Swbf1, "hot1"));
        assert_eq!(GameFamily::split_id("swbf2/hot1"), (GameFamily::Swbf2, "hot1"));
        assert_eq!(GameFamily::split_id("mods/hot1"), (GameFamily::Swbf1, "mods/hot1"));
        assert_eq!(GameFamily::Swbf2.qualify("spa6"), "swbf2/spa6");
        assert_eq!(GameFamily::default(), GameFamily::Swbf1);
    }

    #[test]
    fn availability_gate_allows_swbf1_and_refuses_swbf2() {
        assert!(GameFamily::Swbf1.is_available());
        assert_eq!(GameFamily::Swbf1.require_available(), Ok(()));
        assert!(!GameFamily::Swbf2.is_available());
        let err = GameFamily::Swbf2.require_available().unwrap_err();
        assert_eq!(err, UnavailableFamily::not_available(GameFamily::Swbf2));
        assert_eq!(
            err.to_string(),
            "Battlefront II (2005) is not available in this build"
        );
    }

    // A gate is a value: one that plays every game leaves the built gate, and
    // every other gate, as it was.
    #[test]
    fn a_gate_is_a_value_that_no_other_gate_changes() {
        struct Every;
        impl Availability for Every {
            fn plays(&self, _: GameFamily) -> bool { true }
            fn name(&self) -> &'static str { "every game" }
        }
        let every = Gate::new(&Every);
        assert!(every.plays(GameFamily::Swbf2) && every.require(GameFamily::Swbf2).is_ok());
        assert!(Gate::BUILT.plays(GameFamily::Swbf1));
        assert!(!Gate::BUILT.plays(GameFamily::Swbf2));
        assert_eq!(Gate::default(), Gate::BUILT);
        assert_ne!(every, Gate::BUILT);
        assert_eq!(format!("{:?}", Gate::BUILT), "Gate(\"built\")");
        assert!(!GameFamily::Swbf2.is_available());
    }

    // The developer switch is not compiled into an ordinary build, so its
    // environment variable does nothing there.
    #[cfg(not(bfe_dev_online))]
    #[test]
    fn an_ordinary_build_has_no_developer_switch() {
        std::env::set_var("BFE_DEV_SWBF2_ONLINE", "1");
        assert_eq!(format!("{:?}", super::online_gate()), "Gate(\"built\")");
        assert!(!super::online_gate().plays(GameFamily::Swbf2));
    }

    // A developer build opens the gate only while the switch is on.
    #[cfg(bfe_dev_online)]
    #[test]
    fn a_developer_build_follows_the_switch() {
        for (value, open) in [(None, false), (Some("0"), false), (Some(""), false), (Some("1"), true)] {
            match value {
                Some(value) => std::env::set_var("BFE_DEV_SWBF2_ONLINE", value),
                None => std::env::remove_var("BFE_DEV_SWBF2_ONLINE"),
            }
            let gate = super::online_gate();
            assert_eq!(gate.plays(GameFamily::Swbf2), open, "{value:?}");
            assert_eq!(format!("{gate:?}").contains(super::dev_online::MARKER), open, "{value:?}");
            assert!(!Gate::BUILT.plays(GameFamily::Swbf2));
        }
    }
}
