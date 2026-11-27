//! Read only the selected family's interface catalog at battle admission.
use super::Layout;
use serde::{de::DeserializeOwned, Deserialize};
use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
};

#[derive(Deserialize)]
struct Index {
    trees: BTreeMap<String, String>,
    textures: BTreeMap<String, String>,
    meshes: HashMap<String, String>,
    languages: BTreeMap<String, String>,
    #[serde(default)]
    bounds: HashMap<String, [f64; 4]>,
}

fn confined(root: &Path, name: &str) -> Result<PathBuf, String> {
    if name.is_empty()
        || name.len() > 512
        || name.contains(['\\', ':'])
        || Path::new(name)
            .components()
            .any(|p| !matches!(p, Component::Normal(_)))
    {
        return Err("invalid interface path".into());
    }
    // Compare canonical spellings on both sides: canonicalize answers with a
    // verbatim path on Windows, which never starts with the plain root.
    let root = root.canonicalize().map_err(|e| format!("{name}: {e}"))?;
    let path = root
        .join(name)
        .canonicalize()
        .map_err(|e| format!("{name}: {e}"))?;
    if !path.starts_with(&root) {
        return Err("interface path outside family root".into());
    }
    Ok(path)
}

pub fn child(root: &Path, name: &str) -> Result<PathBuf, String> {
    let path = confined(root, name)?;
    if !path.is_file() {
        return Err(format!("expected a regular interface file: {name}"));
    }
    Ok(path)
}

fn json<T: DeserializeOwned>(path: &Path, limit: usize) -> Result<T, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|f| f.take(limit as u64 + 1).read_to_end(&mut bytes))
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err("oversized interface data".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

pub fn text_id(key: &str) -> u32 {
    if key.len() == 8 {
        if let Ok(n) = u32::from_str_radix(key, 16) {
            return n;
        }
    }
    key.bytes().fold(2166136261, |hash, b| {
        (hash ^ u32::from(b | 32)).wrapping_mul(16777619)
    })
}

fn locale(name: &str) -> String {
    let tag = name.to_ascii_lowercase().replace('_', "-");
    let (base, region) = tag.split_once('-').unwrap_or((&tag, ""));
    let base = match base {
        "de" => "german",
        "en" => "english",
        "es" => "spanish",
        "fr" => "french",
        "it" => "italian",
        b => b,
    };
    if tag == "uk-english" || tag == "en-gb" {
        "english-gb".into()
    } else if region.is_empty() {
        base.into()
    } else {
        format!("{base}-{region}")
    }
}

#[derive(Default)]
pub struct Catalog {
    pub layout: Layout,
    pub textures: BTreeMap<String, PathBuf>,
    pub meshes: HashMap<String, String>,
    /// A model picture's extent in model units: left, bottom, right, top.
    pub bounds: HashMap<String, [f64; 4]>,
    languages: Vec<HashMap<u32, String>>,
}

impl Catalog {
    pub fn open(root: &Path, language: &str) -> Result<Self, String> {
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        let folder = confined(&root, "common/hud")?;
        let index: Index = json(&child(&folder, "presentation.json")?, 1 << 20)?;
        if index.textures.len() > 2048
            || index.meshes.len() > 4096
            || index.bounds.len() > 4096
            || index.bounds.values().flatten().any(|v| !v.is_finite() || v.abs() > 1e4)
            || !(1..=32).contains(&index.languages.len())
        {
            return Err("oversized or empty battle catalog".into());
        }
        let file = |name: &str| {
            let rel = index.trees.get(name).ok_or("missing HUD definition")?;
            File::open(child(&folder, rel)?).map_err(|e| e.to_string())
        };
        let layout = Layout::read(file("1playerhud")?, file("hudtransforms")?)?;
        let mut textures = BTreeMap::new();
        for (id, rel) in index.textures {
            textures.insert(id, child(&folder, &rel)?);
        }
        let mut locales: Vec<_> = index.languages.into_iter().collect();
        let exact = language.to_ascii_lowercase().replace('_', "-");
        let requested = locale(language);
        let base = requested.split('-').next().unwrap_or(&requested);
        locales.sort_by_key(|(name, _)| {
            let tag = locale(name);
            let rank = if name.to_ascii_lowercase().replace('_', "-") == exact {
                0
            } else if tag == requested {
                1
            } else if tag == base {
                2
            } else if tag == "english" {
                3
            } else {
                4
            };
            (rank, name.clone())
        });
        let mut languages = Vec::with_capacity(locales.len());
        let mut budget: usize = 32 << 20;
        for (_, rel) in locales {
            let path = child(&folder, &rel)?;
            let bytes = path.metadata().map_err(|e| e.to_string())?.len();
            if bytes > budget as u64 {
                return Err("oversized HUD languages".into());
            }
            budget -= bytes as usize;
            let strings: BTreeMap<String, String> = json(&path, 8 << 20)?;
            if strings.len() > 65536 {
                return Err("too many interface strings".into());
            }
            let mut table = HashMap::with_capacity(strings.len());
            for (key, value) in strings {
                if key.len() != 8 || value.len() > 65536 {
                    return Err("invalid localized entry".into());
                }
                let id = u32::from_str_radix(&key, 16).map_err(|_| "invalid localized ID")?;
                if !value.is_empty() {
                    table.insert(id, value);
                }
            }
            languages.push(table);
        }
        Ok(Self {
            layout,
            textures,
            meshes: index.meshes,
            bounds: index.bounds,
            languages,
        })
    }

    /// Add a language table after the prepared ones: keys are text keys or prepared IDs.
    pub fn push_language<'a>(&mut self, strings: impl IntoIterator<Item = (&'a str, &'a str)>) {
        self.languages.push(
            strings
                .into_iter()
                .map(|(key, value)| (text_id(key), value.to_owned()))
                .collect(),
        );
    }

    pub fn localized(&self, key: &str) -> Option<&str> {
        let id = text_id(key);
        self.languages
            .iter()
            .find_map(|locale| locale.get(&id).map(String::as_str))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fallback_applies_per_string_and_accepts_a_prepared_id() {
        let catalog = Catalog {
            layout: Layout::default(),
            textures: Default::default(),
            meshes: Default::default(),
            bounds: Default::default(),
            languages: vec![
                [(text_id("loom.row"), "Trame".into())].into(),
                [(text_id("loom.edge"), "Hem".into())].into(),
            ],
        };
        assert_eq!(catalog.localized("LOOM.row"), Some("Trame"));
        assert_eq!(
            catalog.localized(&format!("{:08x}", text_id("loom.edge"))),
            Some("Hem")
        );
        assert_eq!(locale("DE_ch"), "german-ch");
    }
}

#[cfg(test)]
#[test]
fn a_child_is_found_through_an_uncanonical_root_spelling() {
    let root = std::env::temp_dir().join(format!("bfe-hud-root-{}", std::process::id()));
    std::fs::create_dir_all(root.join("common")).unwrap();
    std::fs::write(root.join("common/zz.json"), "{}").unwrap();
    let dotted = root.join(".");
    let found = child(&dotted, "common/zz.json").unwrap();
    assert_eq!(found, root.join("common/zz.json").canonicalize().unwrap());
    assert!(child(&dotted, "../outside.json").is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(all(unix, test))]
#[test]
fn a_fifo_is_rejected_before_opening_the_interface_file() {
    let root = std::env::temp_dir().join(format!("bfe-hud-fifo-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let fifo = root.join("viewport");
    assert!(std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    let value = child(&root, "viewport");
    std::fs::remove_dir_all(root).unwrap();
    assert!(value.is_err());
}

#[cfg(all(unix, test))]
#[test]
fn child_accepts_a_symlinked_family_root_and_still_rejects_escapes() {
    let base = std::env::temp_dir().join(format!("bfe-hud-symlink-root-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let real = base.join("real");
    let common = real.join("common");
    std::fs::create_dir_all(&common).unwrap();
    let inside = common.join("common.json");
    std::fs::write(&inside, b"{}").unwrap();
    let outside = base.join("outside.json");
    std::fs::write(&outside, b"{}").unwrap();
    let alias = base.join("alias");
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    std::os::unix::fs::symlink(&outside, common.join("escape.json")).unwrap();

    let resolved = child(&alias, "common/common.json");
    let escaped = child(&alias, "common/escape.json");
    let expected = inside.canonicalize().unwrap();
    std::fs::remove_dir_all(&base).unwrap();

    assert_eq!(resolved.unwrap(), expected);
    assert!(escaped.unwrap_err().contains("outside family root"));
}
