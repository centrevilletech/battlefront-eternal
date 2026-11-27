//! Controls labels from the 2005 game's prepared common pack.
use bfe_family::GameFamily;
use bfe_interface::{hotkeys::{swbf2_control_label_keys, swbf2_key_labels}, options::options_for};
use serde::Deserialize;
use std::{collections::HashMap, fs::File, io::BufReader, path::Path};

#[derive(Deserialize)]
struct CommonText {
    family: GameFamily,
    strings: HashMap<String, String>,
}

fn read_common(path: &Path) -> Option<CommonText> {
    let file = File::open(path).ok()?;
    let common: CommonText = serde_json::from_reader(BufReader::new(file)).ok()?;
    (common.family == GameFamily::Swbf2).then_some(common)
}

pub(super) fn load(content_dir: &Path) -> HashMap<String, String> {
    // A caller can supply the shared content root or an individual prepared game.
    let common = Path::new("common/common.json");
    let Some(pack) = read_common(&content_dir.join("swbf2").join(common))
        .or_else(|| read_common(&content_dir.join(common))) else {
        return HashMap::new();
    };
    let page_keys = [
        "ifs.GameOpt.pc_optionstitle", "ifs.GameOpt.pc_configbindings",
        "common.back", "common.reset",
    ];
    let tuning_keys = options_for("controls").into_iter().flat_map(|opt| {
        opt.label_key.into_iter().chain(opt.toggle_label_keys.into_iter().flatten())
    });
    let rows = swbf2_control_label_keys().chain(page_keys).chain(tuning_keys).filter_map(|key| {
        let text = pack.strings.get(&bfe_content::odf::fnv(key))?;
        Some((key.to_owned(), text.clone()))
    });
    rows.chain(swbf2_key_labels().filter_map(|(code, key)| {
        let text = pack.strings.get(&bfe_content::odf::fnv(&key))?;
        Some((code.to_owned(), text.clone()))
    })).collect()
}

pub(super) fn takeoff_caption<'a>(
    family: GameFamily,
    settings: &bfe_interface::Settings,
    localized: impl FnOnce(&str) -> Option<&'a str>,
) -> String {
    let flight = match family {
        GameFamily::Swbf1 => bfe_interface::flight_bindings(settings.get_bindings_ref("flightBindings")),
        GameFamily::Swbf2 => bfe_interface::hotkeys::ControlContext::Starfighter.bindings(settings),
    };
    let code = flight.get("takeoff").and_then(|codes| codes.first()).map_or("", String::as_str);
    takeoff_hint(family, code, localized)
}

fn takeoff_hint<'a>(family: GameFamily, code: &str, localized: impl FnOnce(&str) -> Option<&'a str>) -> String {
    match family {
        GameFamily::Swbf1 => bfe_interface::key_name(code),
        GameFamily::Swbf2 => bfe_interface::hotkeys::swbf2_key_caption(code, localized),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takeoff_hint_uses_the_active_games_key_captions() {
        let text = HashMap::<_, String>::from([
            (bfe_content::odf::fnv("ifs.controls.numpad0"), "test keypad".into()),
            (bfe_content::odf::fnv("ifs.controls.lalt"), "test left".into()),
        ]);
        let label = |key: &str| text.get(&bfe_content::odf::fnv(key)).map(String::as_str);
        assert_eq!(takeoff_hint(GameFamily::Swbf1, "Numpad0", label), "KP_INSERT");
        assert_eq!(takeoff_hint(GameFamily::Swbf2, "Numpad0", label), "TEST KEYPAD");
        assert_eq!(takeoff_hint(GameFamily::Swbf2, "AltLeft", label), "TEST LEFT");
        assert_eq!(takeoff_hint(GameFamily::Swbf2, "AltRight", |_| None), format!("RIGHT {}", bfe_interface::key_name("AltRight")));
        assert_eq!(takeoff_hint(GameFamily::Swbf2, "", label), "");
    }

    #[test]
    fn only_the_2005_common_pack_supplies_its_labels() {
        let root = std::env::temp_dir().join(format!("bfe-control-text-{}", std::process::id()));
        let game = root.join("swbf2");
        let key = "ifs.controls.lightsaberblock";
        let hash = bfe_content::odf::fnv(key);
        let plus = bfe_content::odf::fnv("ifs.controls.add");
        for (dir, family, label) in [(&root, "swbf1", "other"), (&game, "swbf2", "test")] {
            std::fs::create_dir_all(dir.join("common")).unwrap();
            let pack = serde_json::json!({"family":family,"strings":{(&hash):label, (&plus):"test plus", "01234567":"unused"}});
            std::fs::write(dir.join("common/common.json"), pack.to_string()).unwrap();
        }
        let expected = HashMap::from([(key.to_string(), "test".to_string()), ("NumpadAdd".into(), "test plus".into())]);
        assert_eq!(load(&root), expected);
        assert_eq!(load(&game), expected);
        std::fs::remove_dir_all(&game).unwrap();
        assert!(load(&root).is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
