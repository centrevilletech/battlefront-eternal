//! Validate prepared loading data before using it to allocate or draw.
use super::{LedBar, MapLoadTicks, MAX_LEDS, MAX_PICTURES, MAX_TIPS};
use serde::Deserialize;
use std::{collections::BTreeMap, io::Read, num::NonZeroU32};

const MAX_MANIFEST_BYTES: usize = 4 << 20;
pub const MAX_TIP_UNITS: usize = 1024;
pub const MAX_TIP_LINES: usize = 64;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tip {
    pub text: String,
    pub line_ends: Vec<usize>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct World {
    pictures: Vec<String>,
    tips_prefix: String,
    duration_ticks: u32,
}

#[derive(Deserialize)]
struct Locale {
    tips: BTreeMap<String, Vec<Tip>>,
    titles: BTreeMap<String, String>,
    label: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Bar {
    num_leds: usize,
    led_width: f64,
    led_height: f64,
    led_spacing: f64,
    led_min_intensity: Vec<serde_json::Value>,
    led_texture: Option<String>,
}

#[derive(Deserialize)]
struct Panel {
    texture: Option<String>,
    #[serde(default)]
    border: Vec<f64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Display {
    progress_bar: Bar,
    tips_box: Panel,
    fallback_picture: Option<String>,
    tip_ticks: u32,
    loading_text_blink_rate: f64,
    max_tip_lines: usize,
}

#[derive(Deserialize)]
struct Catalog {
    maps: BTreeMap<String, World>,
    languages: BTreeMap<String, Locale>,
    language: String,
    display: Display,
}

/// One world and language after validation. The map's duration travels
/// separately from the local art choices.
pub struct PreparedBackdrop {
    pub duration: MapLoadTicks,
    pub tip_ticks: NonZeroU32,
    pub pictures: Vec<String>,
    pub fallback: Option<String>,
    pub panel: Option<String>,
    pub border: [f64; 2],
    pub led: Option<String>,
    pub bar: LedBar,
    pub blink_rate: f64,
    pub tips: Vec<Tip>,
    pub max_lines: usize,
    pub title: String,
    pub label: String,
}

impl PreparedBackdrop {
    pub fn read(input: impl Read, world: &str, language: Option<&str>) -> Result<Self, String> {
        let mut bytes = Vec::new();
        input
            .take(MAX_MANIFEST_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err("oversized loading manifest".into());
        }
        let mut catalog: Catalog =
            serde_json::from_slice(&bytes).map_err(|e| format!("loading data: {e}"))?;
        let display = catalog.display;
        let border = match display.tips_box.border.as_slice() {
            [x, y] if [x, y].iter().all(|&&n| (0. ..=64.).contains(&n)) => [*x, *y],
            [] if display.tips_box.texture.is_none() => [0.; 2],
            _ => return Err("invalid loading panel border".into()),
        };
        let bar = display.progress_bar;
        let min_level = bar
            .led_min_intensity
            .first()
            .and_then(|v| v.as_f64())
            .ok_or("missing LED intensity")?;
        let shape = LedBar {
            count: bar.num_leds,
            width: bar.led_width,
            height: bar.led_height,
            spacing: bar.led_spacing,
            min_level,
        };
        if !(1..=MAX_LEDS).contains(&shape.count)
            || !(0.25..=256.).contains(&shape.width)
            || !(0.25..=48.).contains(&shape.height)
            || !(0.0..=4.).contains(&shape.spacing)
            || !(0.0..=1.).contains(&min_level)
            || bar.led_min_intensity.len() > 2
            || shape.total_width() > 720.
            || !(0.0..=60.).contains(&display.loading_text_blink_rate)
        {
            return Err("invalid loading display geometry or intensity".into());
        }
        if catalog.maps.len() > 512
            || catalog.languages.len() > 32
            || display.max_tip_lines > MAX_TIP_LINES
        {
            return Err("oversized loading catalog".into());
        }
        for map in catalog.maps.values() {
            if map.pictures.len() > MAX_PICTURES || MapLoadTicks::new(map.duration_ticks).is_none()
            {
                return Err("invalid map loading duration or picture set".into());
            }
        }
        for path in catalog
            .maps
            .values()
            .flat_map(|map| &map.pictures)
            .chain(display.fallback_picture.iter())
            .chain(display.tips_box.texture.iter())
            .chain(bar.led_texture.iter())
        {
            if !path.starts_with("load/")
                || !path.ends_with(".png")
                || path.len() > 512
                || path.contains(['\\', ':', '?', '\0'])
                || path.split('/').any(|s| matches!(s, "" | "." | ".."))
            {
                return Err("invalid loading picture path".into());
            }
        }
        for locale in catalog.languages.values() {
            if locale
                .titles
                .values()
                .chain(locale.label.iter())
                .any(|s| s.encode_utf16().count() > 160)
            {
                return Err("oversized loading label".into());
            }
            for tips in locale.tips.values() {
                if tips.len() > MAX_TIPS {
                    return Err("too many loading tips".into());
                }
                for tip in tips {
                    if tip.line_ends.len() > display.max_tip_lines
                        || tip.text.encode_utf16().count() > MAX_TIP_UNITS
                        || !valid_line_ends(&tip.text, &tip.line_ends)
                    {
                        return Err("invalid tip text or line boundaries".into());
                    }
                }
            }
        }
        if catalog.languages.is_empty() {
            return Err("missing loading languages".into());
        }
        let requested = language
            .unwrap_or(&catalog.language)
            .to_lowercase()
            .replace('_', "-");
        let tag = language_key(&requested);
        let base = if tag == "uk-english" {
            "english"
        } else {
            requested.split('-').next().unwrap_or(&requested)
        };
        let base_tag = language_key(base);
        let mut locales: Vec<_> = catalog.languages.into_iter().collect();
        locales.sort_by_cached_key(|(name, _)| {
            let literal = name.to_lowercase().replace('_', "-");
            let key = language_key(&literal);
            [
                literal == requested,
                key == tag,
                literal == base,
                key == base_tag,
                literal == "english",
                key == "english",
            ]
            .iter()
            .position(|&matches| matches)
            .unwrap_or(6)
        });
        let map = catalog.maps.remove(world).ok_or("unknown loading world")?;
        Ok(Self {
            duration: MapLoadTicks::new(map.duration_ticks).ok_or("invalid map duration")?,
            tip_ticks: MapLoadTicks::new(display.tip_ticks)
                .ok_or("invalid tip interval")?
                .0,
            pictures: map.pictures,
            fallback: display.fallback_picture,
            panel: display.tips_box.texture,
            border,
            led: bar.led_texture,
            bar: shape,
            blink_rate: display.loading_text_blink_rate,
            tips: locales
                .iter_mut()
                .find_map(|(_, locale)| {
                    locale
                        .tips
                        .remove(&map.tips_prefix)
                        .filter(|group| !group.is_empty())
                })
                .unwrap_or_default(),
            title: locales
                .iter_mut()
                .find_map(|(_, locale)| locale.titles.remove(world).filter(|s| !s.is_empty()))
                .unwrap_or_else(|| world.to_ascii_uppercase()),
            label: locales
                .iter_mut()
                .find_map(|(_, locale)| locale.label.take().filter(|s| !s.is_empty()))
                .unwrap_or_else(|| "LOADING".into()),
            max_lines: display.max_tip_lines,
        })
    }
}

fn language_key(name: &str) -> String {
    let tag = name.to_ascii_lowercase().replace('_', "-");
    if matches!(tag.as_str(), "uk-english" | "en-gb") {
        return "uk-english".into();
    }
    let (base, region) = tag.split_once('-').unwrap_or((&tag, ""));
    let language = match base {
        "de" => "german",
        "en" => "english",
        "es" => "spanish",
        "fr" => "french",
        "it" => "italian",
        name => name,
    };
    if region.is_empty() {
        language.into()
    } else {
        format!("{language}-{region}")
    }
}

pub(crate) fn valid_line_ends(text: &str, ends: &[usize]) -> bool {
    ends.len() <= MAX_TIP_LINES
        && ends.last().copied().unwrap_or(0) == text.len()
        && ends
            .iter()
            .all(|&end| end != 0 && text.is_char_boundary(end))
        && ends.windows(2).all(|pair| pair[0] < pair[1])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn localized_fields_follow_requested_base_english_and_available_order() {
        let mut book = json!({
            "language": "Danish", "languages": {},
            "maps": {"garden":{"pictures":[], "tipsPrefix":"notes", "durationTicks":190}},
            "display": {"progressBar":{"numLeds":4, "ledWidth":11, "ledHeight":8, "ledSpacing":1,
                "ledMinIntensity":[0.25]}, "tipsBox":{}, "tipTicks":70, "loadingTextBlinkRate":0, "maxTipLines":1}
        });
        for name in ["Danish", "german", "English", "de-CH"] {
            book["languages"][name] = json!({"titles":{"garden":name}, "label":name,
                "tips":{"notes":[{"text":"mint","lineEnds":[4]}]}});
        }
        book["languages"]["de-CH"]["tips"] = json!({});
        for (requested, expected) in [
            (Some("DE_ch"), "de-CH"),
            (Some("de-AT"), "german"),
            (Some("UK_English"), "English"),
            (Some("absent"), "English"),
            (None, "Danish"),
        ] {
            let screen =
                PreparedBackdrop::read(book.to_string().as_bytes(), "garden", requested).unwrap();
            assert_eq!(screen.title, expected);
            assert_eq!(screen.label, expected);
            assert_eq!(screen.tips.len(), 1);
        }
        book["languages"]["de"] = json!({"titles":{"garden":"de"}, "tips":{}});
        for (requested, expected) in [("German", "german"), ("DE-AT", "de")] {
            let screen =
                PreparedBackdrop::read(book.to_string().as_bytes(), "garden", Some(requested))
                    .unwrap();
            assert_eq!(screen.title, expected);
        }
        book["languages"]
            .as_object_mut()
            .unwrap()
            .retain(|name, _| name == "Danish");
        let screen =
            PreparedBackdrop::read(book.to_string().as_bytes(), "garden", Some("en")).unwrap();
        assert_eq!(screen.title, "Danish");
    }

    #[test]
    fn rejects_invalid_manifest_values_before_drawing() {
        let valid = json!({
            "language": "en", "languages": { "en": { "tips":{"p":[]}, "titles":{} } },
            "maps": {"w":{"pictures":[], "tipsPrefix":"p", "durationTicks":475}},
            "display": {"progressBar":{"numLeds":6, "ledWidth":14, "ledHeight":9, "ledSpacing":1.75,
                "ledMinIntensity":[0.2]}, "tipsBox":{}, "tipTicks":115, "loadingTextBlinkRate":2.5, "maxTipLines":0}
        });
        let read = |value: &serde_json::Value| {
            PreparedBackdrop::read(value.to_string().as_bytes(), "w", None)
        };
        assert_eq!(read(&valid).unwrap().duration.get(), 475);
        for (path, invalid) in [
            ("/display/progressBar/numLeds", json!(u64::MAX)),
            ("/display/progressBar/numLeds", json!(2.5)),
            ("/display/progressBar/ledWidth", json!("20")),
            ("/display/tipsBox", json!({"border":[3,-1]})),
            ("/display/tipsBox", json!({"border":[65,8]})),
            (
                "/display/tipsBox",
                json!({"texture":"load/p.png","border":[4]}),
            ),
            ("/display/progressBar/ledHeight", json!(143)),
            ("/display/progressBar/ledSpacing", json!(-1)),
            ("/display/progressBar/ledMinIntensity/0", json!(2)),
            ("/display/tipTicks", json!(0)),
            ("/maps/w/durationTicks", json!(0)),
            ("/maps/w/pictures", json!(["load/../x.png"])),
            ("/display/maxTipLines", json!(100)),
            (
                "/languages/en/tips/p",
                json!([{"text":"last word", "lineEnds":[4]}]),
            ),
        ] {
            let mut value = valid.clone();
            *value.pointer_mut(path).unwrap() = invalid;
            assert!(read(&value).is_err(), "{path}");
        }
        assert!(PreparedBackdrop::read(std::io::repeat(b' '), "w", None).is_err());
    }
}
