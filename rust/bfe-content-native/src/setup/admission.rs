//! A battle's admission for the native world: the options, the fetched
//! files and the setup row, in the shape the whole-world replay driver
//! feeds `Context::world_replay_admit` (docs/world-replay.md, the recorded
//! setup.json plus the assets' bytes). Built here from the player's content
//! instead of a recording, so a local battle and a replay admit alike.
use super::{assets::Asset, options::BattleOptions};
use bfe_content::battle::{Battle, Fetched};
use bfe_sim::gen::WorldTickSetup;
use serde_json::{json, Map, Value};

/// The recorder's fixed harness facts: the wall clock a battle starts at
/// and the seeded `Math.random` three.js draws object ids from. A live
/// battle's own time enters through each frame's `now`.
pub const CLOCK_START_MS: f64 = 1000.0;
pub const CLOCK_UNIX_MS: f64 = 1_700_000_000_000.0;
pub const RANDOM_INITIAL: u32 = 991;
pub const RANDOM_MULTIPLIER: u32 = 1_664_525;
pub const RANDOM_INCREMENT: u32 = 1_013_904_223;

pub struct Admission {
    pub options: BattleOptions,
    pub setup: WorldTickSetup,
    pub assets: Vec<Asset>,
}
impl Admission {
    /// The setup row as the recorder stores it: the four words'
    /// little-endian bytes, base64.
    pub fn world_setup(&self) -> String {
        let mut row = [0.0; WorldTickSetup::WORDS];
        let mut heap = Vec::new();
        self.setup.put(&mut row, &mut heap);
        let bytes: Vec<u8> = row.iter().flat_map(|word| word.to_le_bytes()).collect();
        base64(&bytes)
    }
    /// The recorder's `options` record for a battle a host plays live: the
    /// harness-only fields hold the values of an ordinary page battle (a
    /// snapshot every tick, no acknowledgements, no mount script).
    pub fn options_value(&self) -> Value {
        let o = &self.options;
        json!({
            "map": o.map, "side": o.side, "era": o.era(), "seed": o.seed, "ticks": 0, "interval": 1, "ack": false,
            "view": Value::Null, "ai": o.ai.name(), "heroes": o.heroes, "unit": o.unit, "mount": -1, "battles": 1,
            "nativeRandom": false, "query": o.query(),
        })
    }
    /// The recorder's `argv` for these options.
    pub fn argv(&self) -> Vec<String> {
        let o = &self.options;
        let mut argv = vec!["--map".into(), o.map.clone(), "--side".into(), o.side.clone(), "--seed".into(), o.seed.to_string()];
        if o.ai != super::Ai::Classic { argv.push("--ai".into()); argv.push(o.ai.name().into()); }
        if o.heroes { argv.push("--heroes".into()); }
        if let Some(unit) = &o.unit { argv.push("--unit".into()); argv.push(unit.clone()); }
        argv
    }
    /// setup.json's shape for one battle, without the recording-only
    /// checksums (inputsSha256, frames, compressedBytes) and schema hash.
    pub fn setup_value(&self) -> Value {
        let mut assets = Map::new();
        for asset in &self.assets {
            assets.insert(asset.url.clone(), match asset.hex() {
                Some(sha256) => json!({ "sha256": sha256, "bytes": asset.bytes }),
                None => Value::Null,
            });
        }
        json!({
            "battles": [{ "index": 0, "now": CLOCK_START_MS, "draw": RANDOM_INITIAL, "worldSetup": self.world_setup() }],
            "version": 1,
            "argv": self.argv(),
            "assets": assets,
            "clock": { "start": CLOCK_START_MS, "unix": CLOCK_UNIX_MS, "step": 1000.0 / 60.0 },
            "random": { "initial": RANDOM_INITIAL, "multiplier": RANDOM_MULTIPLIER, "increment": RANDOM_INCREMENT },
            "options": self.options_value(),
        })
    }
    /// The driver's admission line: `{kind:'admit', setup, assets:[{url,hex}]}`,
    /// with every fetched file's bytes (hex, as tools/world-native.mjs sends
    /// them; null for a missing file). Cold: built once a battle.
    pub fn value(&self, battle: &Battle, files: &dyn bfe_content::files::Files) -> Value {
        let assets: Vec<Value> = self
            .assets
            .iter()
            .map(|asset| {
                let path = asset.url.trim_start_matches('/');
                let bytes = match battle.fetched.entries.get(path) {
                    Some(Fetched::Missing) | None => None,
                    Some(_) => files.read(path),
                };
                json!({ "url": asset.url, "hex": bytes.map(|b| hex(&b)) })
            })
            .collect();
        json!({ "kind": "admit", "setup": self.setup_value(), "assets": assets })
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes { out.push_str(&format!("{b:02x}")); }
    out
}

/// Standard base64 with padding, as Buffer.toString('base64') writes it.
pub fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}
