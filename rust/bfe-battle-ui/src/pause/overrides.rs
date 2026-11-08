// URL query parameter overrides for `RESTART MISSION` (`src/pause/overrides.js`).
//
// When `RESTART MISSION` reloads the battle's URL, any query parameter that overrides
// an option setting (`ai`, `tier`, `heroes`, `size`, `scale`, `sens`, `invert`, `shadows`)
// is rewritten to the setting's current value while leaving other query parameters
// (such as a bare `?noreload`) untouched.
//
// Without this rewrite, reloading a battle URL that was initially launched with a one-visit
// query override (e.g. `?tier=hard&invert=1`) would revert any changes the player made
// inside `OPTIONS` during the match (`OVERRIDES` in `src/pause/overrides.js`).

/// Trait providing read access to option settings rewritten by `with_settings` (`src/pause/overrides.js`).
///
/// Decouples URL rewriting from concrete storage so both in-memory `BTreeMap` test fixtures
/// and live `Settings` models can supply current option values.
pub trait PauseSettingsLookup {
    /// Returns the string representation of setting `key` (`aiMode`, `aiDifficulty`, `battleSize`, etc.).
    fn setting_string(&self, key: &str) -> Option<String>;
    /// Returns the boolean value of setting `key` (used for `"aiHeroes"` and `"invertMouse"`).
    fn setting_bool(&self, key: &str) -> Option<bool>;
}

impl PauseSettingsLookup for std::collections::BTreeMap<String, String> {
    fn setting_string(&self, key: &str) -> Option<String> {
        self.get(key).cloned()
    }

    fn setting_bool(&self, key: &str) -> Option<bool> {
        self.get(key)
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    }
}

/// Maps a URL query key from `OVERRIDES` in `src/pause/overrides.js` to its current setting value.
///
/// Boolean settings (`heroes` -> `aiHeroes`, `invert` -> `invertMouse`) serialize as `"1"` or `"0"`;
/// all other recognized keys serialize their string representation directly.
fn override_value(param_key: &str, settings: &dyn PauseSettingsLookup) -> Option<String> {
    match param_key {
        "ai" => settings.setting_string("aiMode"),
        "tier" => settings.setting_string("aiDifficulty"),
        "heroes" => settings
            .setting_bool("aiHeroes")
            .map(|b| if b { "1".to_string() } else { "0".to_string() }),
        "size" => settings.setting_string("battleSize"),
        "scale" => settings.setting_string("renderScale"),
        "sens" => settings.setting_string("mouseSensitivity"),
        "invert" => settings
            .setting_bool("invertMouse")
            .map(|b| if b { "1".to_string() } else { "0".to_string() }),
        "shadows" => settings.setting_string("shadows"),
        _ => None,
    }
}

/// Decodes `%XX` hex escape sequences in a query parameter key (`decodeURIComponent` in `src/pause/overrides.js`).
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            ) {
                out.push(((hi << 4) | lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Percent-encodes a setting value matching JS `encodeURIComponent` unreserved characters
/// (`A-Z`, `a-z`, `0-9`, `-`, `_`, `.`, `!`, `~`, `*`, `'`, `(`, `)`).
fn percent_encode_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        let unreserved = b.is_ascii_alphanumeric()
            || matches!(b, b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')');
        if unreserved {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

/// Rewrites setting-override query keys in `href` to their current values in `settings`,
/// leaving non-override keys (including bare flags like `?noreload`) unchanged (`withSettings` in `src/pause/overrides.js`).
///
/// Preserves the original URL path, hash fragment, and raw key spelling before `=`.
/// Parameters not present in the original query string are never added.
pub fn with_settings(href: &str, settings: &dyn PauseSettingsLookup) -> String {
    // Split off any `#hash` fragment first so query parsing never touches fragment text.
    let (before_hash, hash) = match href.find('#') {
        Some(idx) => (&href[..idx], &href[idx..]),
        None => (href, ""),
    };
    let (path, query) = match before_hash.find('?') {
        Some(idx) => (&before_hash[..idx], &before_hash[idx + 1..]),
        None => (before_hash, ""),
    };
    if query.is_empty() {
        return format!("{}{}", path, hash);
    }

    let mut rewritten = Vec::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        // A bare flag like `?noreload` has no `=`; `raw_key` is the entire token.
        let raw_key = match pair.find('=') {
            Some(at) => &pair[..at],
            None => pair,
        };
        let key = percent_decode(raw_key);
        if let Some(val) = override_value(&key, settings) {
            rewritten.push(format!("{}={}", raw_key, percent_encode_component(&val)));
        } else {
            rewritten.push(pair.to_string());
        }
    }

    if rewritten.is_empty() {
        format!("{}{}", path, hash)
    } else {
        format!("{}?{}{}", path, rewritten.join("&"), hash)
    }
}
