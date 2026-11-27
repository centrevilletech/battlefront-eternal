//! The 2005 tree: where a start may write, whose a tree is, what a record and its inventory
//! make of a tree, and that a start stopped or failing at any single step neither touches
//! other data nor leaves a tree that passes for complete. Every name and value here is made
//! up for these tests.

use super::*;
use crate::prepare::checkout_content_version;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

static NEXT: AtomicUsize = AtomicUsize::new(0);

#[test]
fn recorded_game_folders_must_be_spelled_absolutely() {
    for yes in [
        "/zz/GameData/data/_lvl_pc",
        r"C:\zz\GameData\data\_lvl_pc",
        "C:/zz/GameData/data/_lvl_pc",
        r"\\server\share\GameData\data\_lvl_pc",
        r"\\?\C:\zz\GameData\data\_lvl_pc",
    ] {
        assert!(absolute_game(yes), "{yes}");
    }
    for no in ["", "zz/GameData", "C:zz", r"C:zz\GameData", r"\zz\GameData", r"\\server", r"\\"] {
        assert!(!absolute_game(no), "{no}");
    }
}

#[cfg(windows)]
#[test]
fn a_verbatim_path_keeps_its_prefix_past_a_device_name() {
    // Without the prefix these names are devices, not folders.
    for kept in [r"\\?\C:\zz\conin$\tree", r"\\?\C:\zz\CONOUT$\tree", r"\\?\C:\zz\nul.txt\tree"] {
        assert_eq!(simplify(PathBuf::from(kept)), PathBuf::from(kept), "{kept}");
    }
    assert_eq!(
        simplify(PathBuf::from(r"\\?\C:\zz\plain\tree")),
        PathBuf::from(r"C:\zz\plain\tree")
    );
    assert_eq!(
        simplify(PathBuf::from(r"\\?\UNC\server\share\tree")),
        PathBuf::from(r"\\server\share\tree")
    );
}

/// The generation the tests' tools publish (the tools' own at this writing).
const GENERATION: u32 = 12;

/// A folder of its own for one test, removed afterwards (made writable first).
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "bfe-swbf2-tree-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn join(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        writable(&self.0);
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

fn writable(#[allow(unused_variables)] path: &Path) {
    #[cfg(unix)]
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.is_dir() {
            set_mode(path, 0o755);
            if let Ok(entries) = fs::read_dir(path) {
                for entry in entries.flatten() {
                    writable(&entry.path());
                }
            }
        }
    }
}

#[cfg(unix)]
fn privileged() -> bool {
    use std::os::unix::fs::MetadataExt;
    // A file this process creates belongs to its effective user.
    let probe = tempfile::NamedTempFile::new().expect("a probe file");
    probe.as_file().metadata().map(|m| m.uid() == 0).unwrap_or(false)
}

fn tools() -> Swbf2Tools {
    Swbf2Tools {
        content: checkout_content_version().unwrap(),
        generation: GENERATION,
        handover: true,
        inventory: true,
    }
}

/// A tools folder whose `content-version.json` publishes the 2005 generation and, when asked,
/// the lock protocol and the inventory format.
fn tools_dir_with(at: &Path, handover: bool, inventory: bool) -> PathBuf {
    let mut value = json!(checkout_content_version().unwrap());
    value["swbf2"] = json!(GENERATION);
    if handover {
        value["swbf2Lock"] = json!("stdin");
    }
    if inventory {
        value["swbf2Outputs"] = json!(1);
    }
    put(at, "content-version.json", value.to_string().as_bytes());
    at.to_path_buf()
}

fn tools_dir(at: &Path) -> PathBuf {
    tools_dir_with(at, true, true)
}

/// A process another test starts holds copies of this process's open files between its fork
/// and its exec, so a lock just let go can look held for a moment. Where a test expects a
/// start to succeed, it waits that moment out; another real holder keeps the lock far longer.
fn settled<T>(mut start: impl FnMut() -> Result<T, StartError>) -> Result<T, StartError> {
    for _ in 0..400 {
        match start() {
            Err(StartError::Busy(_)) => std::thread::sleep(std::time::Duration::from_millis(25)),
            done => return done,
        }
    }
    start()
}

fn begin_settled(root: &Path, game: &Path) -> Swbf2Start {
    settled(|| begin(root, game, &tools())).unwrap()
}

fn lock_settled(file: &File) {
    for _ in 0..400 {
        if file.try_lock().is_ok() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    file.try_lock().unwrap();
}

fn put(dir: &Path, rel: &str, bytes: &[u8]) {
    let path = dir.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// A made-up 2005 install the validators accept.
fn install(at: &Path) -> PathBuf {
    let lvl = at.join("GameData/data/_lvl_pc");
    fs::create_dir_all(lvl.join("side")).unwrap();
    put(at, "GameData/BattlefrontII.exe", b"made up");
    for name in ["common.lvl", "core.lvl", "mission.lvl", "ingame.lvl", "shell.lvl"] {
        put(&lvl, name, b"made up");
    }
    at.to_path_buf()
}

/// A made-up installation folder on no disk, spelled absolutely for this platform.
#[cfg(windows)]
const ANOTHER_COPY: &str = r"C:\another\copy\GameData\data\_lvl_pc";
#[cfg(not(windows))]
const ANOTHER_COPY: &str = "/another/copy/GameData/data/_lvl_pc";

fn game_path(install: &Path) -> String {
    fs::canonicalize(install.join("GameData/data/_lvl_pc"))
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

/// Made-up 2004 prepared data at `root` (its own layout, flat), with a record the 2004 reader
/// takes as current.
fn data_2004(root: &Path) {
    let v = checkout_content_version().unwrap();
    put(root, "common/common.json", b"{\"kind\":\"shared\"}");
    put(root, "maps/qq7/map.json", b"{\"kind\":\"map\"}");
    put(root, "sides/vv/units.json", b"{\"kind\":\"units\"}");
    put(root, "audio/index.json", b"{\"kind\":\"audio\"}");
    let record = json!({"version": v.state, "mapImportVersion": v.maps, "muzzleVersion": v.sides,
        "commonVersion": v.common, "complete": true, "game": "/made/up/GameData/Data/_LVL_PC",
        "addOns": [], "failed": [], "skipped": []});
    put(root, ".prepare.json", record.to_string().as_bytes());
}

const BODY: &[u8] = b"made-up compiled script body";
static BODY_PATH: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| format!("scripts/{}.luac", sha_hex(BODY)));

/// The files a complete made-up 2005 preparation holds, the record aside, with the step that
/// wrote each (`*` for the shared ones).
fn outputs() -> Vec<(&'static str, &'static str, Vec<u8>)> {
    let sidecar = json!({"scripts": [
        {"name": "entry", "file": BODY_PATH.as_str(), "length": BODY.len()},
    ]});
    vec![
        ("map:zz1", "maps/zz1/map.json", b"{\"made\":\"up\"}".to_vec()),
        ("map:zz1", "maps/zz1/geom.bin", vec![7u8; 4096]),
        // A file a step writes empty (a map with nothing of that kind).
        ("map:zz1", "maps/zz1/anim_banks.bin", Vec::new()),
        ("map:zz1", "maps/zz1/collision.bin", b"made-up collision".to_vec()),
        ("map:zz1", "maps/zz1/vehicles/vehicles.json", b"{\"vehicles\":[]}".to_vec()),
        ("map:zz1", "maps/zz1/scripts/era-mode.json", sidecar.to_string().into_bytes()),
        ("side:qa", "sides/qa/units.json", b"{\"units\":[]}".to_vec()),
        ("common", "common/common.json", b"{\"shared\":true}".to_vec()),
        ("common", "firstperson/anims.bin", vec![5, 6, 7, 8]),
        ("common", "firstperson/rigs/reed.json", b"{}".to_vec()),
        ("common", "firstperson/rigs/reed.bin", vec![13]),
        ("common", "firstperson/rigs/reed_index.bin", vec![3]),
        ("common", "firstperson/manifest.json",
            br#"{"version":2,"animsData":"anims.bin","rigs":{"reed":{"file":"rigs/reed.json"}},"textures":{}}"#.to_vec()),
        ("common", "common/anim_banks.json", b"{\"banks\":[]}".to_vec()),
        ("*", BODY_PATH.as_str(), BODY.to_vec()),
        ("*", "audio/index.json", b"{\"banks\":[]}".to_vec()),
        ("*", "audio/zz1sub/sound.json", b"{\"sound\":1}".to_vec()),
    ]
}

fn sha_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// The Enhanced Sound step of the fixture: planned, done, and listing nothing (its files are
/// the sound check's).
const SOUND_STEP: &str = "audio:zz1sub";

/// The inventory of [`outputs`], as a record carries it.
fn inventory() -> Value {
    let mut by_step: BTreeMap<&str, Vec<Value>> = BTreeMap::new();
    by_step.insert(SOUND_STEP, Vec::new());
    for (step, path, bytes) in outputs() {
        let entry = if path.starts_with("scripts/") || path.starts_with("firstperson/") {
            json!([path, bytes.len(), sha_hex(&bytes)])
        } else {
            json!([path, bytes.len()])
        };
        by_step.entry(step).or_default().push(entry);
    }
    json!(by_step)
}

fn record(install: &Path) -> Value {
    let v = checkout_content_version().unwrap();
    json!({
        "family": "swbf2",
        "version": v.state,
        "mapImportVersion": v.maps,
        "muzzleVersion": v.sides,
        "commonVersion": v.common,
        "swbf2Version": GENERATION,
        "game": game_path(install),
        "plan": ["map:zz1", "side:qa", "common", SOUND_STEP],
        "done": ["map:zz1", "side:qa", "common", SOUND_STEP],
        "outputs": inventory(),
        "addOns": [],
        "failed": [],
        "skipped": [],
        "failures": [],
        "complete": true,
        "toolsOwnCache": {"kept": true},
    })
}

/// A complete 2005 tree in `dir`, its record changed by `edit`.
fn tree(dir: &Path, install: &Path, edit: impl FnOnce(&mut serde_json::Map<String, Value>)) {
    for (_, rel, bytes) in outputs() {
        put(dir, rel, &bytes);
    }
    // Enhanced Sound: outside the inventory.
    put(dir, ENHANCED, b"{\"made\":\"up\"}");
    let mut value = record(install);
    edit(value.as_object_mut().unwrap());
    put(dir, SWBF2_PREPARE_STATE_FILE, value.to_string().as_bytes());
}

/// An Enhanced Sound file of the fixture, which no list names.
const ENHANCED: &str = "audio/enhanced/zz1sub/manifest.json";

fn mark(dir: &Path) {
    put(dir, SWBF2_MARKER_FILE, SWBF2_MARKER_BODY.as_bytes());
}

fn same(_: &mut serde_json::Map<String, Value>) {}

/// Every entry under `dir`: a file's bytes, `None` for a folder, the target for a link.
fn snapshot(dir: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
    fn walk(base: &Path, at: &Path, out: &mut BTreeMap<String, Option<Vec<u8>>>) {
        let Ok(entries) = fs::read_dir(at) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let rel = path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
            let meta = fs::symlink_metadata(&path).unwrap();
            if meta.file_type().is_symlink() {
                let target = fs::read_link(&path).unwrap();
                out.insert(rel, Some(target.to_string_lossy().into_owned().into_bytes()));
            } else if meta.is_dir() {
                out.insert(rel.clone(), None);
                walk(base, &path, out);
            } else if entry.file_name() == LOCK_FILE {
                // A start holds the preparation lock, and on Windows a held byte-range lock refuses
                // reads even to the process that holds it. Nothing writes into the lock file: its
                // size stands for it.
                out.insert(rel, Some(format!("lock file of {} bytes", meta.len()).into_bytes()));
            } else {
                out.insert(rel, fs::read(&path).ok());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

fn files_2004(root: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
    snapshot(root)
        .into_iter()
        .filter(|(rel, _)| !rel.starts_with("swbf2"))
        .collect()
}

fn swbf2_dir(root: &Path) -> PathBuf {
    root.join("swbf2").join(".content")
}

#[cfg(unix)]
fn link(target: impl AsRef<Path>, at: impl AsRef<Path>) {
    std::os::unix::fs::symlink(target, at).unwrap();
}

fn found(dir: &Path, game: &Path) -> Swbf2Record {
    let install = identify_install(game).unwrap();
    inspect(dir, &tools(), Some(&install)).unwrap()
}

fn kind(r: &Swbf2Record) -> &'static str {
    match r {
        Swbf2Record::Empty => "empty",
        Swbf2Record::Complete => "complete",
        Swbf2Record::Resumable => "resumable",
        Swbf2Record::Stale(_) => "stale",
        Swbf2Record::Foreign(_) => "foreign",
    }
}

#[cfg(unix)]
#[test]
fn a_start_refuses_every_place_and_tree_it_does_not_own_and_changes_nothing() {
    let s = Scratch::new("places");
    let game = install(&s.join("game"));
    let t = tools();
    let elsewhere = s.join("elsewhere-2004");
    data_2004(&elsewhere);
    let mut roots = Vec::new();

    // A 2004 copy named swbf2 inside the root, with `.content` a link back to itself.
    let root = s.join("a");
    data_2004(&root.join("swbf2"));
    link(".", root.join("swbf2/.content"));
    roots.push(root);
    // The same copy without the link: its own folder is no place for the 2005 data.
    let root = s.join("a2");
    data_2004(&root.join("swbf2"));
    roots.push(root);
    // `.content` a link to the root's own 2004 data, or to a 2004 copy elsewhere.
    let root = s.join("b");
    data_2004(&root);
    fs::create_dir_all(root.join("swbf2")).unwrap();
    link("..", root.join("swbf2/.content"));
    roots.push(root);
    let root = s.join("c");
    fs::create_dir_all(root.join("swbf2")).unwrap();
    link(&elsewhere, root.join("swbf2/.content"));
    roots.push(root);
    // `swbf2` itself a link to a 2004 copy; `.content` a plain file.
    let root = s.join("d");
    fs::create_dir_all(&root).unwrap();
    link(&elsewhere, root.join("swbf2"));
    roots.push(root);
    let root = s.join("d2");
    put(&root, "swbf2/.content", b"not a folder");
    roots.push(root);
    // The content folder inside a 2004 copy, or a 2005 tree itself.
    let outer = s.join("e");
    data_2004(&outer);
    fs::create_dir_all(outer.join("inner")).unwrap();
    roots.push(outer.join("inner"));
    let marked_tree = swbf2_dir(&s.join("f"));
    tree(&marked_tree, &game, same);
    mark(&marked_tree);
    roots.push(marked_tree);
    // A link inside an otherwise owned tree, to 2004 data.
    let root = s.join("g");
    tree(&swbf2_dir(&root), &game, |r| {
        r.insert("game".into(), json!("/another/copy/GameData/data/_lvl_pc"));
    });
    link(elsewhere.join("maps"), swbf2_dir(&root).join("maps/linked"));
    roots.push(root);
    // A 2004 record the 2004 reader takes as current, with 2005 fields added, beside the
    // marker: 2004 content whatever else it carries.
    let root = s.join("h");
    let dir = swbf2_dir(&root);
    data_2004(&dir);
    mark(&dir);
    let mut current: Value = serde_json::from_slice(&fs::read(dir.join(".prepare.json")).unwrap()).unwrap();
    current["family"] = json!("swbf2");
    current["swbf2Version"] = Value::Null;
    put(&dir, ".prepare.json", current.to_string().as_bytes());
    roots.push(root);
    // An early 2005 record alone: no record of this build's and no marker.
    let root = s.join("h2");
    let dir = swbf2_dir(&root);
    for (_, rel, bytes) in outputs() {
        put(&dir, rel, &bytes);
    }
    put(&dir, ".prepare.json", b"{\"family\":\"swbf2\",\"swbf2Version\":4}");
    roots.push(root);
    // A marker naming another game.
    let root = s.join("h3");
    tree(&swbf2_dir(&root), &game, same);
    put(&swbf2_dir(&root), SWBF2_MARKER_FILE, b"{\"family\":\"swbf1\"}");
    roots.push(root);
    // Another prepared tree nested inside the 2005 tree.
    let root = s.join("i");
    tree(&swbf2_dir(&root), &game, same);
    data_2004(&swbf2_dir(&root).join("maps/nested"));
    roots.push(root);
    // Prepared files with no 2005 record or marker (the shape of a copied 2004 folder).
    let root = s.join("j");
    data_2004(&swbf2_dir(&root));
    fs::remove_file(swbf2_dir(&root).join(".prepare.json")).unwrap();
    roots.push(root);

    for root in &roots {
        let before = snapshot(&s.0);
        let error = begin(root, &game, &t).unwrap_err();
        assert!(matches!(error, StartError::Refused(_)), "{}: {error:?}", root.display());
        assert_eq!(snapshot(&s.0), before, "{}: nothing may change", root.display());
        assert!(!verify(root, &game, &t).unwrap(), "{} must not verify", root.display());
    }
}

#[cfg(unix)]
#[test]
fn a_destination_the_tools_could_not_receive_exactly_is_refused_before_any_change() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let s = Scratch::new("spelling");
    let game = install(&s.join("game"));
    let odd = s.0.join(OsStr::from_bytes(b"made-up-\xff-folder"));
    match fs::create_dir_all(&odd) {
        Ok(()) => {}
        Err(error) if error.raw_os_error() == Some(libc::EILSEQ) => {}
        Err(error) => panic!("unexpected error creating non-UTF-8 folder: {error}"),
    }
    let before = snapshot(&s.0);
    let error = begin(&odd.join("content"), &game, &tools()).unwrap_err();
    assert!(matches!(error, StartError::Refused(_)), "{error:?}");
    assert_eq!(snapshot(&s.0), before);
}

#[cfg(unix)]
#[test]
fn every_spelling_of_a_content_folder_finds_the_same_tree() {
    let s = Scratch::new("spellings");
    let root = s.join("content");
    data_2004(&root);
    let real = locate(&root).unwrap();
    assert_eq!(real.dir, fs::canonicalize(&root).unwrap().join("swbf2/.content"));
    link(&root, s.join("alias"));
    for spelling in [
        root.join("."),
        root.join(""),
        root.join("../content"),
        PathBuf::from(format!("{}//", root.display())),
        s.join("alias"),
        s.join("alias/"),
    ] {
        assert_eq!(locate(&spelling).unwrap(), real, "{}", spelling.display());
    }
    // `..` after a link is resolved physically, never against the spelling.
    fs::create_dir_all(s.join("deep/inner")).unwrap();
    link(s.join("deep/inner"), s.join("hop"));
    data_2004(&s.join("deep/sibling"));
    let via_hop = locate(&s.join("hop/../sibling")).unwrap();
    assert_eq!(via_hop.root, fs::canonicalize(s.join("deep/sibling")).unwrap());
    // A folder that does not exist yet resolves through its existing parent; one inside the
    // 2004 data is refused.
    fs::create_dir_all(s.join("plain")).unwrap();
    link(s.join("plain"), s.join("plain-alias"));
    let fresh = locate(&s.join("plain-alias/new/deeper")).unwrap();
    assert_eq!(fresh.root, fs::canonicalize(s.join("plain")).unwrap().join("new/deeper"));
    assert!(locate(&s.join("alias/new/deeper")).is_err());
}

#[test]
fn toggling_enhanced_sound_keeps_a_completed_tree_usable() {
    let s = Scratch::new("sound-setting");
    let game = install(&s.join("game"));
    let root = s.join("content");
    let dir = swbf2_dir(&root);
    let tdir = tools_dir(&s.join("tools"));
    tree(&dir, &game, same);
    let mut receipt = record(&game);
    let full_plan = receipt["plan"].clone();
    let mut core_plan = full_plan.clone();
    core_plan.as_array_mut().unwrap().retain(|step| step != SOUND_STEP);
    for (label, plan) in [
        ("sound on", &full_plan),
        ("sound off", &core_plan),
        ("sound still off", &core_plan),
        ("sound back on", &full_plan),
    ] {
        // The producer keeps done and its inventories when the player's plan changes.
        receipt["plan"] = plan.clone();
        put(&dir, SWBF2_PREPARE_STATE_FILE, receipt.to_string().as_bytes());
        check(&s, &root, &game, &tdir, label, "complete");
    }
}

/// What a record says must agree with what preparation, verification, export and import
/// make of it.
#[test]
fn the_record_and_its_inventory_decide_for_start_verify_export_and_import() {
    let s = Scratch::new("records");
    let game = install(&s.join("game"));
    let other = install(&s.join("other-copy"));
    let tdir = tools_dir(&s.join("tools"));
    type Edit = Box<dyn Fn(&mut serde_json::Map<String, Value>)>;
    let edit = |f: fn(&mut serde_json::Map<String, Value>)| -> Edit { Box::new(f) };
    let record_cases: Vec<(&str, Edit, &str)> = vec![
        ("complete", edit(|_| {}), "complete"),
        ("unfinished", edit(|r| { r.insert("complete".into(), json!(false)); }), "resumable"),
        ("a failed step", edit(|r| { r.insert("failed".into(), json!(["map:zz1"])); }), "resumable"),
        ("a planned step not done", edit(|r| { r.insert("done".into(), json!(["map:zz1", "common"])); }), "resumable"),
        ("no plan", edit(|r| { r.remove("plan"); }), "resumable"),
        ("a planned step without its list", edit(|r| { r["outputs"].as_object_mut().unwrap().remove("side:qa"); }), "resumable"),
        ("a planned map without its list", edit(|r| { r["outputs"].as_object_mut().unwrap().remove("map:zz1"); }), "resumable"),
        ("the sound step without its list", edit(|r| { r["outputs"].as_object_mut().unwrap().remove(SOUND_STEP); }), "resumable"),
        ("no shared list", edit(|r| { r["outputs"].as_object_mut().unwrap().remove("*"); }), "resumable"),
        ("no inventory, as before the tools kept one", edit(|r| { r.remove("outputs"); }), "resumable"),
        ("a file listed as empty that is not", edit(|r| { r["outputs"]["side:qa"] = json!([["sides/qa/units.json", 0]]); }), "resumable"),
        ("the inventory leaving out the map's record", edit(|r| {
            r["outputs"]["map:zz1"].as_array_mut().unwrap().retain(|e| e[0] != "maps/zz1/map.json");
        }), "resumable"),
        ("the inventory leaving out common.json", edit(|r| {
            r["outputs"]["common"].as_array_mut().unwrap().retain(|e| e[0] != "common/common.json");
        }), "resumable"),
        ("an inventory that is not an object", edit(|r| { r.insert("outputs".into(), json!([])); }), "stale"),
        ("a listed size that is not a number", edit(|r| { r["outputs"]["*"] = json!([["audio/index.json", "12"]]); }), "stale"),
        ("a path outside the tree listed", edit(|r| { r["outputs"]["common"] = json!([["../outside.json", 3]]); }), "stale"),
        ("the lock listed", edit(|r| { r["outputs"]["*"] = json!([[".prepare.lock", 3]]); }), "stale"),
        ("a path listed twice", edit(|r| { r["outputs"]["*"] = json!([["audio/index.json", 12], ["AUDIO/index.json", 12]]); }), "stale"),
        ("a hash that is not one", edit(|r| { r["outputs"]["*"] = json!([["audio/index.json", 12, "nope"]]); }), "stale"),
        ("an unplanned inventory group", edit(|r| { r["outputs"]["other"] = json!([["audio/enhanced/unlisted", 1]]); }), "resumable"),
        ("an unfinished extra step with an empty list", edit(|r| {
            r["outputs"]["audio:optional"] = json!([]);
        }), "resumable"),
        ("a retired add-on step with an empty list", edit(|r| {
            r["done"].as_array_mut().unwrap().push(json!("addon:optional"));
            r["outputs"]["addon:optional"] = json!([]);
        }), "complete"),
        ("a map outside the plan still claims products", edit(|r| {
            r["done"].as_array_mut().unwrap().push(json!("map:former"));
            r["outputs"]["map:former"] = json!([["maps/former/geom.bin", 8]]);
        }), "resumable"),
        ("a newly planned map has no products yet", edit(|r| {
            r["plan"].as_array_mut().unwrap().push(json!("map:next"));
            r["done"].as_array_mut().unwrap().push(json!("map:next"));
            r["outputs"]["map:next"] = json!([]);
        }), "resumable"),
        ("a repeated planned step", edit(|r| {
            r["plan"].as_array_mut().unwrap().push(json!("common"));
        }), "resumable"),
        ("an Enhanced Sound step claiming a core file", edit(|r| {
            let row = r["outputs"]["*"].as_array_mut().unwrap().pop().unwrap();
            r["outputs"][SOUND_STEP] = json!([row]);
        }), "resumable"),
        ("no family", edit(|r| { r.remove("family"); }), "foreign"),
        ("an older state", edit(|r| { r.insert("version".into(), json!(1)); }), "stale"),
        ("a version as text", edit(|r| { r.insert("muzzleVersion".into(), json!("3")); }), "stale"),
        ("no generation", edit(|r| { r.remove("swbf2Version"); }), "foreign"),
        ("a negative generation", edit(|r| { r.insert("swbf2Version".into(), json!(-1)); }), "foreign"),
        ("generation zero", edit(|r| { r.insert("swbf2Version".into(), json!(0)); }), "foreign"),
        ("an older generation, which the tools bring up to date", edit(|r| { r.insert("swbf2Version".into(), json!(GENERATION - 1)); }), "resumable"),
        ("generation one", edit(|r| { r.insert("swbf2Version".into(), json!(1)); }), "resumable"),
        ("a future generation", edit(|r| { r.insert("swbf2Version".into(), json!(GENERATION + 1)); }), "foreign"),
        ("a fractional generation", edit(|r| { r.insert("swbf2Version".into(), json!(9.5)); }), "foreign"),
        ("no game", edit(|r| { r.remove("game"); }), "stale"),
        ("a blank game", edit(|r| { r.insert("game".into(), json!("   ")); }), "stale"),
        ("a relative game", edit(|r| { r.insert("game".into(), json!("GameData/data/_lvl_pc")); }), "stale"),
        ("no complete field", edit(|r| { r.remove("complete"); }), "stale"),
        ("no finished steps", edit(|r| { r.remove("done"); }), "stale"),
        ("a malformed failure", edit(|r| { r.insert("failures".into(), json!([{"label": "no id"}])); }), "stale"),
        ("an add-on", edit(|r| { r.insert("addOns".into(), json!(["made-up-add-on"])); }), "stale"),
        ("no map planned", edit(|r| { r.insert("plan".into(), json!(["side:qa", "common"])); }), "stale"),
        ("another copy that is gone", edit(|r| { r.insert("game".into(), json!("/made/up/gone/GameData/data/_lvl_pc")); }), "stale for the copy"),
    ];
    let mut n = 0;
    for (label, change, want) in &record_cases {
        n += 1;
        let root = s.join(&format!("record-{n}"));
        tree(&swbf2_dir(&root), &game, |r| change(r));
        check(&s, &root, &game, &tdir, label, want);
    }
    // Another copy: stale for that copy, complete for an export, which needs no copy.
    let root = s.join("another-copy");
    tree(&swbf2_dir(&root), &game, |r| {
        r.insert("game".into(), json!(game_path(&other)));
    });
    check(&s, &root, &game, &tdir, "another copy", "stale for the copy");

    // Changes to the files rather than the record: each listed file the producer wrote,
    // missing, cut short, emptied or replaced, keeps the tree from being complete.
    type FileEdit = Box<dyn Fn(&Path)>;
    let file_cases: Vec<(&str, FileEdit, &str)> = vec![
        ("the ground geometry missing", Box::new(|d| fs::remove_file(d.join("maps/zz1/geom.bin")).unwrap()), "resumable"),
        ("the ground geometry cut short", Box::new(|d| fs::write(d.join("maps/zz1/geom.bin"), [7u8; 10]).unwrap()), "resumable"),
        ("the collision missing", Box::new(|d| fs::remove_file(d.join("maps/zz1/collision.bin")).unwrap()), "resumable"),
        ("the vehicles missing", Box::new(|d| fs::remove_dir_all(d.join("maps/zz1/vehicles")).unwrap()), "resumable"),
        ("the shared animation index missing", Box::new(|d| fs::remove_file(d.join("common/anim_banks.json")).unwrap()), "resumable"),
        ("the audio missing", Box::new(|d| fs::remove_dir_all(d.join("audio")).unwrap()), "resumable"),
        ("the map's file emptied", Box::new(|d| fs::write(d.join("maps/zz1/map.json"), b"").unwrap()), "resumable"),
        ("the side's file emptied", Box::new(|d| fs::write(d.join("sides/qa/units.json"), b"").unwrap()), "resumable"),
        ("a first-person buffer changed at equal size", Box::new(|d| { put(d, "firstperson/rigs/reed.bin", &[14]); }), "resumable"),
        ("missing first-person triangles", Box::new(|d| fs::remove_file(d.join("firstperson/rigs/reed_index.bin")).unwrap()), "resumable"),
        ("the shared file emptied", Box::new(|d| fs::write(d.join("common/common.json"), b"").unwrap()), "resumable"),
        ("a script body changed", Box::new(|d| fs::write(d.join(BODY_PATH.as_str()), b"made-up compiled script bodY").unwrap()), "resumable"),
        ("the scripts and their index missing", Box::new(|d| {
            fs::remove_dir_all(d.join("scripts")).unwrap();
            fs::remove_dir_all(d.join("maps/zz1/scripts")).unwrap();
        }), "resumable"),
        ("a listed file replaced by a folder", Box::new(|d| {
            fs::remove_file(d.join("maps/zz1/collision.bin")).unwrap();
            fs::create_dir(d.join("maps/zz1/collision.bin")).unwrap();
        }), "resumable"),
        ("the empty file filled", Box::new(|d| fs::write(d.join("maps/zz1/anim_banks.bin"), b"x").unwrap()), "resumable"),
        ("the empty file missing", Box::new(|d| fs::remove_file(d.join("maps/zz1/anim_banks.bin")).unwrap()), "resumable"),
        ("an Enhanced Sound file missing", Box::new(|d| fs::remove_file(d.join(ENHANCED)).unwrap()), "complete"),
        ("an unlisted extra file", Box::new(|d| put(d, "maps/zz1/extra.bin", b"more")), "resumable"),
        ("an unfamiliar hidden entry", Box::new(|d| put(d, ".future-render/scene", b"later")), "resumable"),
        ("the record unreadable", Box::new(|d| fs::write(d.join(SWBF2_PREPARE_STATE_FILE), b"{\"family\":").unwrap()), "foreign"),
        ("the record a folder", Box::new(|d| {
            fs::remove_file(d.join(SWBF2_PREPARE_STATE_FILE)).unwrap();
            fs::create_dir(d.join(SWBF2_PREPARE_STATE_FILE)).unwrap();
            mark(d);
        }), "foreign"),
        ("no record beside the files", Box::new(|d| {
            fs::remove_file(d.join(SWBF2_PREPARE_STATE_FILE)).unwrap();
            mark(d);
        }), "foreign"),
        ("both record names", Box::new(|d| put(d, ".prepare.json", b"{\"family\":\"swbf2\",\"swbf2Version\":4}")), "foreign"),
        ("machinery only", Box::new(|d| {
            for top in ["maps", "sides", "common", "audio", "scripts"] {
                fs::remove_dir_all(d.join(top)).unwrap();
            }
            fs::remove_file(d.join(SWBF2_PREPARE_STATE_FILE)).unwrap();
            put(d, ".locks/made-up.lock", b"");
            put(d, LOCK_FILE, b"");
        }), "foreign"),
    ];
    for (label, change, want) in &file_cases {
        n += 1;
        let root = s.join(&format!("file-{n}"));
        let dir = swbf2_dir(&root);
        tree(&dir, &game, same);
        change(&dir);
        check(&s, &root, &game, &tdir, label, want);
    }
}

fn check(s: &Scratch, root: &Path, game: &Path, tdir: &Path, label: &str, want: &str) {
    let t = tools();
    let dir = swbf2_dir(root);
    let with_copy = found(&dir, game);
    let without = inspect(&dir, &t, None).unwrap();
    if want == "stale for the copy" {
        assert_eq!(kind(&with_copy), "stale", "{label}: {with_copy:?}");
        assert_eq!(kind(&without), "complete", "{label}: an export needs no installed copy");
    } else {
        assert_eq!(kind(&with_copy), want, "{label}: {with_copy:?}");
        assert_eq!(kind(&without), want, "{label}: {without:?}");
    }
    assert_eq!(
        verify(root, game, &t).unwrap(),
        with_copy == Swbf2Record::Complete,
        "{label}: verification disagrees"
    );
    let stem = label.replace(' ', "-");
    let archive = s.join(&format!("{stem}.bfecontent"));
    let exported = crate::archive::export_content_for(root, GameFamily::Swbf2, &archive, Some(tdir));
    assert_eq!(
        exported.is_ok(),
        without == Swbf2Record::Complete,
        "{label}: export disagrees: {exported:?}"
    );
    if exported.is_ok() {
        // An archive its own importer restores and verifies.
        let imported = crate::archive::import_content_for(
            &archive,
            GameFamily::Swbf2,
            &s.join(&format!("{stem}-imports")),
            Some(tdir),
        )
        .unwrap();
        let copy = swbf2_dir(&imported.content_dir);
        assert_eq!(inspect(&copy, &t, None).unwrap(), Swbf2Record::Complete, "{label}");
    }
    // A start clears exactly what is stale and leaves everything else as it was.
    let before = snapshot(&dir);
    if want == "foreign" {
        assert!(matches!(settled(|| begin(root, game, &t)), Err(StartError::Refused(_))));
        assert_eq!(snapshot(&dir), before, "{label}: the unowned tree changed");
        return;
    }
    let start = settled(|| begin(root, game, &t)).unwrap();
    assert_eq!(kind(&start.found), kind(&with_copy), "{label}");
    let after = snapshot(&dir);
    if kind(&with_copy) == "stale" {
        assert!(
            after.keys().all(|rel| rel.starts_with('.')),
            "{label}: a stale tree is cleared: {:?}",
            after.keys().collect::<Vec<_>>()
        );
        assert!(after.contains_key(SWBF2_MARKER_FILE));
    } else {
        for (rel, bytes) in &before {
            assert_eq!(after.get(rel), Some(bytes), "{label}: {rel} must stay");
        }
    }
}

/// A minimal stored ZIP, to hand an importer an archive its exporter would never write.
fn zip_of(path: &Path, entries: &[(&str, &[u8])]) {
    use crc32fast::Hasher as Crc32;
    let mut out = Vec::new();
    let mut central = Vec::new();
    for &(name, data) in entries {
        let mut crc = Crc32::new();
        crc.update(data);
        let crc = crc.finalize();
        let offset = out.len() as u32;
        let header = |sig: u32, central_record: bool| {
            let mut h = Vec::new();
            h.extend_from_slice(&sig.to_le_bytes());
            if central_record {
                h.extend_from_slice(&45u16.to_le_bytes());
            }
            h.extend_from_slice(&20u16.to_le_bytes());
            h.extend_from_slice(&0x0800u16.to_le_bytes());
            h.extend_from_slice(&0u16.to_le_bytes());
            h.extend_from_slice(&0u16.to_le_bytes());
            h.extend_from_slice(&0x21u16.to_le_bytes());
            h.extend_from_slice(&crc.to_le_bytes());
            h.extend_from_slice(&(data.len() as u32).to_le_bytes());
            h.extend_from_slice(&(data.len() as u32).to_le_bytes());
            h.extend_from_slice(&(name.len() as u16).to_le_bytes());
            h.extend_from_slice(&0u16.to_le_bytes());
            if central_record {
                h.extend_from_slice(&0u16.to_le_bytes());
                h.extend_from_slice(&0u16.to_le_bytes());
                h.extend_from_slice(&0u16.to_le_bytes());
                h.extend_from_slice(&0u32.to_le_bytes());
                h.extend_from_slice(&offset.to_le_bytes());
            }
            h.extend_from_slice(name.as_bytes());
            h
        };
        out.extend(header(0x04034b50, false));
        out.extend_from_slice(data);
        central.extend(header(0x02014b50, true));
    }
    let start = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x06054b50u32.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    fs::write(path, out).unwrap();
}

#[test]
fn an_import_succeeds_only_with_the_record_and_a_complete_restored_tree() {
    let s = Scratch::new("import");
    let game = install(&s.join("game"));
    let tdir = tools_dir(&s.join("tools"));
    let record = record(&game).to_string().into_bytes();
    let manifest = |files: &[(&str, &[u8])]| {
        let entries: Vec<Value> = files
            .iter()
            .map(|(path, bytes)| json!({"path": path, "size": bytes.len(), "sha256": sha_hex(bytes)}))
            .collect();
        json!({"format": crate::archive::ARCHIVE_FORMAT_SWBF2, "version": 1, "files": entries})
            .to_string()
            .into_bytes()
    };
    /// The files an archive holds, by name.
    type Entries = Vec<(String, Vec<u8>)>;
    let all = outputs();
    let mut cases: Vec<(&str, Entries, bool)> = Vec::new();
    let full: Entries = all
        .iter()
        .map(|(_, p, b)| (p.to_string(), b.clone()))
        .chain([(SWBF2_PREPARE_STATE_FILE.to_string(), record.clone())])
        .collect();
    cases.push(("everything", full.clone(), true));
    cases.push((
        "a listed file left out",
        full.iter().filter(|(p, _)| p != "maps/zz1/geom.bin").cloned().collect(),
        false,
    ));
    cases.push((
        "no record",
        full.iter().filter(|(p, _)| p != SWBF2_PREPARE_STATE_FILE).cloned().collect(),
        false,
    ));
    let mut with_lock = full.clone();
    with_lock.push((LOCK_FILE.to_string(), b"held".to_vec()));
    cases.push(("the lock inside", with_lock, false));
    let mut with_marker = full.clone();
    with_marker.push((SWBF2_MARKER_FILE.to_string(), b"{}".to_vec()));
    cases.push(("a marker inside", with_marker, false));
    for (label, files, ok) in cases {
        let refs: Vec<(&str, &[u8])> = files.iter().map(|(p, b)| (p.as_str(), b.as_slice())).collect();
        let m = manifest(&refs);
        let mut entries: Vec<(&str, &[u8])> = vec![(crate::archive::MANIFEST_NAME, m.as_slice())];
        entries.extend(refs.iter().copied());
        let archive = s.join(&format!("{}.bfecontent", label.replace(' ', "-")));
        zip_of(&archive, &entries);
        let dest = s.join(&format!("{}-dest", label.replace(' ', "-")));
        let result = crate::archive::import_content_for(&archive, GameFamily::Swbf2, &dest, Some(&tdir));
        assert_eq!(result.is_ok(), ok, "{label}: {result:?}");
        if !ok {
            let left: Vec<_> = fs::read_dir(&dest).map(|d| d.flatten().collect()).unwrap_or_default();
            assert!(left.is_empty(), "{label}: a failed import leaves nothing");
        }
    }
}

/// The ways a test interrupts a start: every operation from `kill_at` on fails (the process
/// died there), or only the operation `fail_at` fails.
struct Faults {
    kill_at: Option<usize>,
    fail_at: Option<usize>,
    kind: io::ErrorKind,
    count: AtomicUsize,
    failed: AtomicBool,
    after_failure: Mutex<Vec<(Op, PathBuf)>>,
}

impl Faults {
    fn new(kill_at: Option<usize>, fail_at: Option<usize>, kind: io::ErrorKind) -> Self {
        Self {
            kill_at,
            fail_at,
            kind,
            count: AtomicUsize::new(0),
            failed: AtomicBool::new(false),
            after_failure: Mutex::new(Vec::new()),
        }
    }

    fn hook(&self, op: Op, path: &Path) -> io::Result<()> {
        let n = self.count.fetch_add(1, Ordering::SeqCst);
        if self.kill_at.is_some_and(|k| n >= k) {
            return Err(io::Error::other("stopped here"));
        }
        if self.failed.load(Ordering::SeqCst) {
            self.after_failure.lock().unwrap().push((op, path.to_path_buf()));
        }
        if self.fail_at == Some(n) {
            self.failed.store(true, Ordering::SeqCst);
            return Err(io::Error::new(self.kind, "made-up failure"));
        }
        Ok(())
    }
}

fn faulty_begin(faults: &Faults, root: &Path, game: &Path) -> Result<Swbf2Start, StartError> {
    let hook = |op: Op, path: &Path| faults.hook(op, path);
    let ops = Ops::with_hook(&hook);
    begin_with(&ops, root, game, &tools())
}

/// The states a start can find a tree in, each built in a fresh root beside 2004 data.
fn build_state(root: &Path, game: &Path, state: &str) {
    data_2004(root);
    let dir = swbf2_dir(root);
    match state {
        "fresh" => {}
        "complete" => {
            tree(&dir, game, same);
            mark(&dir);
        }
        "complete without a marker" => tree(&dir, game, same),
        "unfinished" => {
            tree(&dir, game, |r| {
                r.insert("complete".into(), json!(false));
            });
            mark(&dir);
        }
        "a file missing" => {
            tree(&dir, game, same);
            mark(&dir);
            fs::remove_file(dir.join("maps/zz1/geom.bin")).unwrap();
        }
        "another copy" => {
            tree(&dir, game, |r| {
                r.insert("game".into(), json!(ANOTHER_COPY));
            });
            mark(&dir);
        }
        "an early record" => {
            tree(&dir, game, same);
            fs::remove_file(dir.join(SWBF2_PREPARE_STATE_FILE)).unwrap();
            let early = json!({"family":"swbf2","swbf2Version":3,"version":2,"game":game_path(game),
                "done":[], "complete":false});
            put(&dir, OLD_RECORD, early.to_string().as_bytes());
        }
        "interrupted clear" => {
            tree(&dir, game, |r| { r.insert("clearing".into(), json!(true)); });
            mark(&dir);
        }
        other => panic!("unknown state {other}"),
    }
}

/// After any interrupted or failed start: the 2004 data is as it was, and the tree passes
/// for complete only when every file of a complete preparation is there.
fn assert_safe(root: &Path, game: &Path, data_before: &BTreeMap<String, Option<Vec<u8>>>, what: &str) {
    assert_eq!(&files_2004(root), data_before, "{what}: the 2004 data changed");
    let dir = swbf2_dir(root);
    let install = identify_install(game).unwrap();
    for copy in [Some(&install), None] {
        if inspect(&dir, &tools(), copy).unwrap() == Swbf2Record::Complete {
            for (_, rel, bytes) in outputs() {
                assert_eq!(fs::read(dir.join(rel)).ok(), Some(bytes), "{what}: complete without {rel}");
            }
        }
    }
}

fn destructive(op: Op, path: &Path) -> bool {
    let own_temp = path
        .file_name()
        .is_some_and(|name| is_marker_leftover(&name.to_string_lossy()));
    match op {
        Op::Remove => !own_temp,
        Op::MakeDir | Op::Write | Op::Rename | Op::Lock => true,
        Op::Resolve | Op::Stat | Op::Read | Op::List => false,
    }
}

/// Which operation indexes a fault run tries. Everywhere but Windows: all
/// of them. On Windows each cycle's file creation and flushing costs far
/// more (measured under Wine at over 14x the Linux time, and real NTFS adds
/// a virus scan per created file), so there the edges and a fixed stride
/// sample stand in for the interior: the property at the first and last
/// operations and across the whole sequence is kept, the untested interior
/// steps of each sequence are what is given up, and Linux CI stays exhaustive.
fn fault_steps(steps: usize) -> Vec<usize> {
    #[cfg(windows)]
    {
        (0..steps)
            .filter(|k| *k < 16 || *k + 16 >= steps || k % 11 == 5)
            .collect()
    }
    #[cfg(not(windows))]
    {
        (0..steps).collect()
    }
}

#[test]
fn fault_steps_keep_the_edges_and_a_stride() {
    let all = fault_steps(300);
    assert!(all.contains(&0) && all.contains(&299));
    assert!(all.windows(2).all(|w| w[0] < w[1]));
    #[cfg(windows)]
    assert!(all.len() < 90, "{}", all.len());
    #[cfg(not(windows))]
    assert_eq!(all.len(), 300);
    assert_eq!(fault_steps(3), vec![0, 1, 2]);
}

#[test]
fn a_start_stopped_or_failing_at_any_step_loses_nothing_and_fakes_no_completion() {
    let s = Scratch::new("faults");
    let game = install(&s.join("game"));
    let states = [
        "fresh",
        "complete",
        "complete without a marker",
        "unfinished",
        "a file missing",
        "another copy",
        "an early record",
        "interrupted clear",
    ];
    let mut runs = 0;
    for state in states {
        // A start with no faults: how many operations it takes and the tree it leaves.
        let clean = s.join(&format!("{}-clean", state.replace(' ', "-")));
        build_state(&clean, &game, state);
        let mut steps = 0;
        drop(settled(|| {
            let counted = Faults::new(None, None, io::ErrorKind::Other);
            let result = faulty_begin(&counted, &clean, &game);
            steps = counted.count.load(Ordering::SeqCst);
            result
        })
        .unwrap());
        let expected = snapshot(&clean);
        assert!(steps > 3, "{state}: {steps} operations");

        for k in fault_steps(steps) {
            for (mode, kind) in [
                ("stopped", io::ErrorKind::Other),
                ("failed", io::ErrorKind::Other),
                ("refused", io::ErrorKind::PermissionDenied),
            ] {
                let what = format!("{state}: {mode} at operation {k} of {steps}");
                let root = s.join(&format!("{}-{k}-{mode}", state.replace(' ', "-")));
                build_state(&root, &game, state);
                let data_before = files_2004(&root);
                let fresh = || {
                    if mode == "stopped" {
                        Faults::new(Some(k), None, kind)
                    } else {
                        Faults::new(None, Some(k), kind)
                    }
                };
                let mut faults = fresh();
                let mut result = faulty_begin(&faults, &root, &game);
                for _ in 0..400 {
                    // Busy before operation k was reached is the moment described at `settled`.
                    if !matches!(result, Err(StartError::Busy(_))) {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(25));
                    faults = fresh();
                    result = faulty_begin(&faults, &root, &game);
                }
                assert!(result.is_err(), "{what}: the start must not succeed");
                drop(result);
                assert_safe(&root, &game, &data_before, &what);
                if mode != "stopped" {
                    // After a failure nothing further is changed (removing its own marker
                    // file in the making aside).
                    let after = faults.after_failure.lock().unwrap();
                    assert!(
                        after.iter().all(|(op, path)| !destructive(*op, path)),
                        "{what}: changed things after the failure: {after:?}"
                    );
                }
                // The next start finishes the job and leaves what an undisturbed one leaves.
                drop(begin_settled(&root, &game));
                assert_eq!(snapshot(&root), expected, "{what}: the retry differs");
                fs::remove_dir_all(&root).unwrap();
                runs += 1;
            }
        }
    }
    println!("{runs} interrupted starts over {} starting states", states.len());
    assert!(runs > 100, "{runs} interrupted starts");
}

#[test]
fn a_failure_before_any_change_leaves_every_name_and_byte() {
    // Identifying the copy, locating the tree and checking it come before any change: a
    // failure at any of those operations leaves the whole root as it was.
    let s = Scratch::new("early-failures");
    let game = install(&s.join("game"));
    for state in ["fresh", "another copy", "complete without a marker"] {
        let clean = s.join(&format!("{}-count", state.replace(' ', "-")));
        build_state(&clean, &game, state);
        // How many operations a start makes before its first change.
        let first_change = AtomicUsize::new(usize::MAX);
        let count = AtomicUsize::new(0);
        let hook = |op: Op, path: &Path| -> io::Result<()> {
            let n = count.fetch_add(1, Ordering::SeqCst);
            if destructive(op, path) {
                first_change.fetch_min(n, Ordering::SeqCst);
            }
            Ok(())
        };
        drop(settled(|| begin_with(&Ops::with_hook(&hook), &clean, &game, &tools())).unwrap());
        let before_change = first_change.load(Ordering::SeqCst);
        for k in 0..before_change {
            let root = s.join(&format!("{}-{k}", state.replace(' ', "-")));
            build_state(&root, &game, state);
            let before = snapshot(&root);
            let faults = Faults::new(None, Some(k), io::ErrorKind::Other);
            let result = faulty_begin(&faults, &root, &game);
            assert!(result.is_err(), "{state}: failing operation {k}");
            assert_eq!(snapshot(&root), before, "{state}: failing operation {k} changed something");
            fs::remove_dir_all(&root).unwrap();
        }
    }
}

#[test]
fn the_copy_is_identified_once_and_a_failed_lookup_changes_nothing() {
    let s = Scratch::new("install");
    let game = install(&s.join("game"));
    // The same copy through a link is the same copy.
    #[cfg(unix)]
    {
        link(&game, s.join("game-alias"));
        let root = s.join("alias-content");
        build_state(&root, &game, "complete");
        assert_eq!(found(&swbf2_dir(&root), &s.join("game-alias")), Swbf2Record::Complete);
        assert!(verify(&root, &s.join("game-alias"), &tools()).unwrap());
    }
    // A copy spelled in another case is found the way the tools find it.
    let cased = s.join("cased");
    install(&s.join("cased-src"));
    fs::create_dir_all(cased.join("gamedata")).unwrap();
    fs::rename(s.join("cased-src/GameData/BattlefrontII.exe"), cased.join("gamedata/battlefrontii.exe")).unwrap();
    fs::rename(s.join("cased-src/GameData/data"), cased.join("gamedata/DATA")).unwrap();
    let identified = identify_install(&cased).unwrap();
    assert!(identified.lvl_pc.ends_with("gamedata/DATA/_lvl_pc"));
    // A failed listing of the copy's folder while it is identified stops the start with every
    // file as it was; so does a failed lookup of the folder the record names.
    let root = s.join("stale");
    build_state(&root, &game, "another copy");
    let install_root = identify_install(&game).unwrap().root;
    for (label, op, target) in [
        ("the copy's folder listing", Op::List, install_root.clone()),
        ("the record's folder lookup", Op::Stat, PathBuf::from(ANOTHER_COPY)),
    ] {
        let before = snapshot(&root);
        let hook = |o: Op, path: &Path| -> io::Result<()> {
            if o == op && path == target {
                return Err(io::Error::other("made-up failure"));
            }
            Ok(())
        };
        let result = settled(|| begin_with(&Ops::with_hook(&hook), &root, &game, &tools()));
        assert!(matches!(result, Err(StartError::Failed(_))), "{label}: {result:?}");
        assert_eq!(snapshot(&root), before, "{label}: nothing may change");
    }
}

#[test]
fn tools_that_cannot_take_over_the_lock_leave_the_folder_byte_identical() {
    let s = Scratch::new("no-handover");
    let game = install(&s.join("game"));
    let mut no_handover = tools();
    no_handover.handover = false;
    let mut no_inventory = tools();
    no_inventory.inventory = false;
    for (which, lacking) in [("handover", &no_handover), ("inventory", &no_inventory)] {
        for state in ["fresh", "another copy", "complete without a marker", "complete"] {
            let root = s.join(&format!("{which}-{}", state.replace(' ', "-")));
            build_state(&root, &game, state);
            let before = snapshot(&root);
            let error = begin(&root, &game, lacking).unwrap_err();
            assert!(matches!(error, StartError::Refused(_)), "{which}, {state}: {error:?}");
            assert_eq!(snapshot(&root), before, "{which}, {state}: every name and byte must stay");
        }
    }
    // Tools that publish no 2005 generation cannot prepare the 2005 game at all; the other
    // keys are read as published.
    let plain = s.join("plain-tools");
    let released = serde_json::to_vec(&checkout_content_version().unwrap()).unwrap();
    put(&plain, "content-version.json", &released);
    assert!(swbf2_tools(Some(&plain)).is_err());
    let read = |handover, inventory, at: &str| swbf2_tools(Some(&tools_dir_with(&s.join(at), handover, inventory))).unwrap();
    let generation_only = read(false, false, "generation-only");
    assert!(!generation_only.handover && !generation_only.inventory);
    assert_eq!(generation_only.generation, GENERATION);
    let lock_only = read(true, false, "lock-only");
    assert!(lock_only.handover && !lock_only.inventory);
    let both = read(true, true, "both");
    assert!(both.handover && both.inventory);
}

#[test]
fn a_2004_destination_is_refused_before_its_lock_is_opened() {
    let s = Scratch::new("foreign-lock");
    let root = s.join("content");
    let dir = swbf2_dir(&root);
    let game = install(&s.join("game"));
    data_2004(&dir);
    put(&dir, LOCK_FILE, b"");
    let before = snapshot(&root);
    let hook = |op, path: &Path| {
        assert_ne!(op, Op::Lock, "opened {}", path.display());
        Ok(())
    };
    let start = begin_with(&Ops::with_hook(&hook), &root, &game, &tools());
    assert!(matches!(start, Err(StartError::Refused(_))));
    assert_eq!(snapshot(&root), before);
}

#[test]
fn conflicting_evidence_at_lock_acquisition_prevents_writes() {
    let s = Scratch::new("changed-origin");
    let root = s.join("content");
    let dir = swbf2_dir(&root);
    let game = install(&s.join("game"));
    tree(&dir, &game, same);
    put(&dir, LOCK_FILE, b"");
    let hook = |op, _: &Path| {
        if op == Op::Lock {
            data_2004(&dir);
        }
        Ok(())
    };
    let start = begin_with(&Ops::with_hook(&hook), &root, &game, &tools());
    assert!(matches!(start, Err(StartError::Refused(_))));
    assert!(dir.join(OLD_RECORD).is_file());
    assert!(!dir.join(SWBF2_MARKER_FILE).exists());
}

#[test]
fn a_second_start_while_one_holds_the_tree_is_busy_and_changes_nothing() {
    let s = Scratch::new("busy");
    let game = install(&s.join("game"));
    let other = install(&s.join("other-copy"));
    let t = tools();
    let root = s.join("content");
    build_state(&root, &game, "another copy");

    // One start holds the lock. A second, even for another copy (whose record would be
    // stale for it), is busy and changes no name or byte.
    let first = begin_settled(&root, &game);
    let held = snapshot(&root);
    for (who, from) in [("same copy", &game), ("another copy", &other)] {
        match begin(&root, from, &t) {
            Err(StartError::Busy(message)) => assert!(message.contains("already running")),
            other => panic!("{who}: {other:?}"),
        }
        assert_eq!(snapshot(&root), held, "{who}");
    }
    #[cfg(unix)]
    {
        link(&root, s.join("alias"));
        assert!(matches!(begin(&s.join("alias"), &other, &t), Err(StartError::Busy(_))));
        assert_eq!(snapshot(&root), held);
    }
    drop(first);

    // The tools' own hold on their lock (opened by name, as they do) keeps a start out too,
    // while they are producing ordinary files.
    let tools_hold = fs::OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .open(swbf2_dir(&root).join(LOCK_FILE))
        .unwrap();
    lock_settled(&tools_hold);
    put(&swbf2_dir(&root), "maps/zz1/tools-writing.json", b"{}");
    let writing = snapshot(&root);
    assert!(matches!(begin(&root, &other, &t), Err(StartError::Busy(_))));
    assert_eq!(snapshot(&root), writing);
    // The writer can remove a temporary directory between enumeration and inspection.
    // A persistent read error must be reported once the writer releases the lock.
    let pending = swbf2_dir(&root).join("maps/zz1/half-written");
    fs::create_dir(&pending).unwrap();
    let pending = simplify(fs::canonicalize(&pending).unwrap());
    let start = |kind| {
        let hook = |op, path: &Path| {
            if op == Op::List && path == pending { Err(io::Error::from(kind)) } else { Ok(()) }
        };
        begin_with(&Ops::with_hook(&hook), &root, &other, &t)
    };
    for kind in [io::ErrorKind::NotFound, io::ErrorKind::PermissionDenied] {
        assert!(matches!(start(kind), Err(StartError::Busy(_))));
    }
    let side = swbf2_dir(&root).join("sides/foreign/units.json");
    put(&swbf2_dir(&root), "sides/foreign/units.json", br#"{"family":"swbf1"}"#);
    let hook = |op, path: &Path| {
        assert_ne!(op, Op::Lock, "foreign payload must veto the lock probe");
        if op == Op::List && path == pending {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        Ok(())
    };
    assert!(matches!(begin_with(&Ops::with_hook(&hook), &root, &other, &t), Err(StartError::Refused(_))));
    fs::remove_file(side).unwrap();
    drop(tools_hold);
    for kind in [io::ErrorKind::NotFound, io::ErrorKind::PermissionDenied] {
        // A child process another test forks keeps a copy of a lock descriptor this test held until
        // it starts its program, so a start right after a release can still see the tree busy.
        let mut result = start(kind);
        for _ in 0..400 {
            if !matches!(result, Err(StartError::Busy(_))) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
            result = start(kind);
        }
        assert!(matches!(result, Err(StartError::Failed(why)) if why.contains(&io::Error::from(kind).to_string())));
    }
    drop(begin_settled(&root, &other));
}

#[test]
fn a_start_paused_inside_its_lock_keeps_a_second_one_out() {
    let s = Scratch::new("paused");
    let game = install(&s.join("game"));
    let other = install(&s.join("other-copy"));
    let root = s.join("content");
    build_state(&root, &game, "another copy");
    let paused = std::sync::Barrier::new(2);
    let resume = std::sync::Barrier::new(2);
    let first_remove = AtomicBool::new(true);
    std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            let hook = |op: Op, _: &Path| -> io::Result<()> {
                if op == Op::Remove && first_remove.swap(false, Ordering::SeqCst) {
                    paused.wait();
                    resume.wait();
                }
                Ok(())
            };
            let ops = Ops::with_hook(&hook);
            settled(|| begin_with(&ops, &root, &game, &tools())).map(|start| start.found)
        });
        paused.wait();
        // The paused start is about to remove the stale record, holding the lock.
        let before = snapshot(&root);
        let second = begin(&root, &other, &tools()).map(|start| start.found);
        let during = snapshot(&root);
        // Let the paused start go on before judging, so a failure here cannot leave it waiting.
        resume.wait();
        assert!(matches!(second, Err(StartError::Busy(_))), "{second:?}");
        assert_eq!(during, before);
        assert!(matches!(first.join().unwrap(), Ok(Swbf2Record::Stale(_))));
    });
    assert!(!swbf2_dir(&root).join("maps").exists());
}

#[test]
fn a_removal_that_fails_stops_the_clear_and_a_retry_finishes_it() {
    let s = Scratch::new("removal");
    let game = install(&s.join("game"));
    let t = tools();
    let record_path = |root: &Path| swbf2_dir(root).join(SWBF2_PREPARE_STATE_FILE);

    // An unfinished record must be published before any old payload is removed.
    let root = s.join("record");
    build_state(&root, &game, "another copy");
    let before = snapshot(&root);
    let hook = |op: Op, path: &Path| -> io::Result<()> {
        if op == Op::Rename && path.ends_with(SWBF2_PREPARE_STATE_FILE) {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "made-up refusal"));
        }
        Ok(())
    };
    let error = settled(|| begin_with(&Ops::with_hook(&hook), &root, &game, &t)).unwrap_err();
    assert!(matches!(error, StartError::Failed(_)), "{error:?}");
    let after = snapshot(&root);
    for (rel, bytes) in &before {
        assert_eq!(after.get(rel), Some(bytes), "{rel} must stay");
    }
    assert!(record_path(&root).is_file());
    drop(begin_settled(&root, &game));
    assert!(record_path(&root).is_file() && !swbf2_dir(&root).join("maps").exists());

    // An interrupted removal retains proof of origin and never advertises completion.
    let root = s.join("output");
    build_state(&root, &game, "another copy");
    let hook = |op: Op, path: &Path| -> io::Result<()> {
        if op == Op::Remove && path.ends_with("sides/qa/units.json") {
            return Err(io::Error::other("made-up failure"));
        }
        Ok(())
    };
    let error = settled(|| begin_with(&Ops::with_hook(&hook), &root, &game, &t)).unwrap_err();
    assert!(matches!(error, StartError::Failed(_)), "{error:?}");
    assert!(record_path(&root).is_file());
    assert!(swbf2_dir(&root).join("sides/qa/units.json").is_file());
    assert!(matches!(found(&swbf2_dir(&root), &game), Swbf2Record::Stale(_)));
    assert!(!verify(&root, &game, &t).unwrap());
    drop(begin_settled(&root, &game));
    assert!(!swbf2_dir(&root).join("sides").exists());
    assert_eq!(found(&swbf2_dir(&root), &game), Swbf2Record::Resumable);
}

#[cfg(unix)]
#[test]
fn read_only_and_partly_read_only_trees_lose_nothing() {
    if privileged() {
        eprintln!("skipped: permissions do not bind this user");
        return;
    }
    let s = Scratch::new("read-only");
    let game = install(&s.join("game"));
    let t = tools();

    // A read-only tree: the start fails before it changes anything.
    for with_lock in [false, true] {
        let root = s.join(&format!("tree-{with_lock}"));
        build_state(&root, &game, "another copy");
        if with_lock {
            put(&swbf2_dir(&root), LOCK_FILE, b"");
        }
        set_mode(&swbf2_dir(&root), 0o555);
        let before = snapshot(&root);
        let error = settled(|| begin(&root, &game, &t)).unwrap_err();
        assert!(matches!(error, StartError::Failed(_)), "{error:?}");
        assert_eq!(snapshot(&root), before);
        writable(&root);
    }

    // A read-only folder interrupts the removal. The unfinished record survives.
    let root = s.join("partly");
    build_state(&root, &game, "another copy");
    set_mode(&swbf2_dir(&root).join("sides/qa"), 0o555);
    let data_before = files_2004(&root);
    let error = settled(|| begin(&root, &game, &t)).unwrap_err();
    assert!(matches!(error, StartError::Failed(_)), "{error:?}");
    assert_safe(&root, &game, &data_before, "partly read-only");
    assert!(!verify(&root, &game, &t).unwrap());
    writable(&root);
    drop(begin_settled(&root, &game));
    assert!(!swbf2_dir(&root).join("sides").exists());
}

#[cfg(unix)]
#[test]
fn the_marker_is_published_atomically_and_a_correct_one_is_left_alone() {
    use std::os::unix::fs::MetadataExt;
    let s = Scratch::new("marker");
    let game = install(&s.join("game"));
    let t = tools();
    let root = s.join("content");
    build_state(&root, &game, "complete");
    let marker = swbf2_dir(&root).join(SWBF2_MARKER_FILE);

    // A correct marker, however it is spelled, stays the very same file.
    fs::write(&marker, b"{\"format\":1,\"family\":\"swbf2\"}").unwrap();
    let inode = fs::metadata(&marker).unwrap().ino();
    drop(begin_settled(&root, &game));
    assert_eq!(fs::metadata(&marker).unwrap().ino(), inode);
    assert_eq!(fs::read(&marker).unwrap(), b"{\"format\":1,\"family\":\"swbf2\"}");

    // An empty or broken one is replaced by a new file, and leftovers of an interrupted
    // publication go.
    for bad in [&b""[..], b"{", b"[]"] {
        fs::write(&marker, bad).unwrap();
        put(&swbf2_dir(&root), ".bfe-family.json.1-2.tmp", b"half");
        let inode = fs::metadata(&marker).unwrap().ino();
        drop(begin_settled(&root, &game));
        assert_ne!(fs::metadata(&marker).unwrap().ino(), inode);
        assert_eq!(fs::read(&marker).unwrap(), SWBF2_MARKER_BODY.as_bytes());
        assert!(!swbf2_dir(&root).join(".bfe-family.json.1-2.tmp").exists());
    }
    assert_eq!(found(&swbf2_dir(&root), &game), Swbf2Record::Complete);

    // Stopped between writing the new marker and putting it in place: the old marker is
    // there as it was; the next start completes the publication.
    fs::write(&marker, b"").unwrap();
    let hook = |op: Op, _: &Path| -> io::Result<()> {
        if op == Op::Rename {
            return Err(io::Error::other("stopped here"));
        }
        Ok(())
    };
    assert!(!matches!(
        settled(|| begin_with(&Ops::with_hook(&hook), &root, &game, &t)),
        Ok(_) | Err(StartError::Busy(_))
    ));
    assert_eq!(fs::read(&marker).unwrap(), b"");
    drop(begin_settled(&root, &game));
    assert_eq!(fs::read(&marker).unwrap(), SWBF2_MARKER_BODY.as_bytes());

    // A directory in place of the marker contradicts the preparation layout.
    fs::remove_file(&marker).unwrap();
    fs::create_dir(&marker).unwrap();
    let before = snapshot(&root);
    let error = settled(|| begin(&root, &game, &t)).unwrap_err();
    assert!(matches!(error, StartError::Refused(_)), "{error:?}");
    assert_eq!(snapshot(&root), before);
}

#[test]
fn an_export_carries_every_file_but_the_machinery_and_the_import_restores_it() {
    let s = Scratch::new("export");
    let game = install(&s.join("game"));
    let tdir = tools_dir(&s.join("tools"));
    let root = s.join("content");
    data_2004(&root);
    let dir = swbf2_dir(&root);
    tree(&dir, &game, |record| {
        let shared = record["outputs"]["*"].as_array_mut().unwrap();
        shared.extend([json!(["laterkind/deeper/made-up.bin", 9]), json!(["toplevel.json", 2]),
            json!([".future-render/scene", 5])]);
    });
    mark(&dir);
    // Kinds of files this build has never heard of, and the tree's machinery.
    put(&dir, "laterkind/deeper/made-up.bin", b"\x00\x01made up");
    put(&dir, "toplevel.json", b"{}");
    put(&dir, ".future-render/scene", b"later");
    put(&dir, LOCK_FILE, b"");
    put(&dir, ".locks/made-up.lock", b"");
    put(&dir, RECEIPT_NAME, b"{\"source\":null}");
    put(&dir, ".bfe-family.json.3-4.tmp", b"half");

    // The expected set, written independently of the exporter: every regular file under the
    // tree except the named machinery.
    let mut want: Vec<String> = Vec::new();
    for (rel, bytes) in snapshot(&dir) {
        if bytes.is_none() {
            continue;
        }
        let top = rel.split('/').next().unwrap();
        let machinery = [LOCK_FILE, ".locks", SWBF2_MARKER_FILE, RECEIPT_NAME, ".bfe-family.json.3-4.tmp"];
        if !machinery.contains(&top) {
            want.push(rel);
        }
    }
    want.sort();
    assert!(want.contains(&SWBF2_PREPARE_STATE_FILE.to_string()));
    assert!(want.contains(&".future-render/scene".to_string()));
    let listed: Vec<String> = content_files(&dir).unwrap().into_iter().map(|f| f.0).collect();
    assert_eq!(listed, want);

    let archive = s.join("tree.bfecontent");
    let summary = crate::archive::export_content_for(&root, GameFamily::Swbf2, &archive, Some(&tdir)).unwrap();
    assert_eq!(summary.files, want.len());
    let imported = crate::archive::import_content_for(&archive, GameFamily::Swbf2, &s.join("imports"), Some(&tdir)).unwrap();
    let copy = swbf2_dir(&imported.content_dir);
    for rel in &want {
        assert_eq!(fs::read(copy.join(rel)).unwrap(), fs::read(dir.join(rel)).unwrap(), "{rel}");
    }
    assert_eq!(fs::read(copy.join(SWBF2_MARKER_FILE)).unwrap(), SWBF2_MARKER_BODY.as_bytes());
    assert!(!copy.join(LOCK_FILE).exists() && !copy.join(".locks").exists());
    assert!(verify(&imported.content_dir, &game, &tools()).unwrap());
    // The 2004 import still declines it, as the released one does, naming the earliest entry
    // outside the 2004 layout, and writes nothing.
    assert_eq!(
        crate::archive::import_content(&archive, &s.join("imports-2004")).unwrap_err(),
        "Not prepared game content: .future-render/scene"
    );
    assert!(fs::read_dir(s.join("imports-2004")).unwrap().next().is_none());
    // A tree holding a name the archive keeps for itself is not exported.
    put(&dir, MANIFEST_NAME, b"{}");
    assert!(crate::archive::export_content_for(&root, GameFamily::Swbf2, &s.join("named.bfecontent"), Some(&tdir)).is_err());
    fs::remove_file(dir.join(MANIFEST_NAME)).unwrap();
    // A link in the tree stops the export.
    #[cfg(unix)]
    {
        link("../common", dir.join("laterkind/linked"));
        assert!(crate::archive::export_content_for(&root, GameFamily::Swbf2, &s.join("linked.bfecontent"), Some(&tdir)).is_err());
    }
}

#[cfg(unix)]
#[test]
fn the_record_is_recognised_by_what_it_is_not_how_a_listing_spells_it() {
    // A second name for the record (what a case-folding disk shows as another spelling of the
    // same file) is exported once, under the record's own name.
    let s = Scratch::new("record-alias");
    let game = install(&s.join("game"));
    let root = s.join("content");
    build_state(&root, &game, "complete");
    let dir = swbf2_dir(&root);
    let alias_spelling = |from: &str, to: &str| {
        let src = dir.join(from);
        let dst = dir.join(to);
        match fs::hard_link(&src, &dst) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                fs::rename(&src, &dst).unwrap();
            }
            Err(error) => panic!("unexpected error aliasing {from} to {to}: {error}"),
        }
    };
    alias_spelling(SWBF2_PREPARE_STATE_FILE, ".PREPARE-SWBF2.JSON");
    let listed: Vec<String> = content_files(&dir).unwrap().into_iter().map(|f| f.0).collect();
    assert_eq!(listed.iter().filter(|n| n.eq_ignore_ascii_case(SWBF2_PREPARE_STATE_FILE)).count(), 1);
    assert!(listed.contains(&SWBF2_PREPARE_STATE_FILE.to_string()));
    // A second name for the lock is machinery too.
    put(&dir, LOCK_FILE, b"");
    alias_spelling(LOCK_FILE, ".PREPARE.LOCK");
    let listed: Vec<String> = content_files(&dir).unwrap().into_iter().map(|f| f.0).collect();
    assert!(!listed.iter().any(|n| n.eq_ignore_ascii_case(LOCK_FILE)));
}

#[cfg(unix)]
#[test]
fn an_export_takes_the_tree_a_start_takes_and_holds_its_lock() {
    let s = Scratch::new("export-path");
    let game = install(&s.join("game"));
    let tdir = tools_dir(&s.join("tools"));
    // `hop/../content` names `deep/content` physically and `content` lexically: both are
    // complete trees with different maps.
    let physical_root = s.join("deep/content");
    build_state(&physical_root, &game, "complete");
    let lexical_root = s.join("content");
    build_state(&lexical_root, &game, "complete");
    let lexical_dir = swbf2_dir(&lexical_root);
    fs::rename(lexical_dir.join("maps/zz1"), lexical_dir.join("maps/zz2")).unwrap();
    let mut r: Value = serde_json::from_slice(&fs::read(lexical_dir.join(SWBF2_PREPARE_STATE_FILE)).unwrap()).unwrap();
    let text = r.to_string().replace("zz1", "zz2");
    r = serde_json::from_str(&text).unwrap();
    put(&lexical_dir, SWBF2_PREPARE_STATE_FILE, r.to_string().as_bytes());
    fs::create_dir_all(s.join("deep/inner")).unwrap();
    link(s.join("deep/inner"), s.join("hop"));
    let spelled = s.join("hop/../content");
    let tree = locate(&spelled).unwrap();
    assert_eq!(tree.root, fs::canonicalize(&physical_root).unwrap());
    let summary = crate::archive::export_content_for(&spelled, GameFamily::Swbf2, &s.join("a.bfecontent"), Some(&tdir)).unwrap();
    assert_eq!(summary.maps, vec!["zz1".to_string()], "the export took another tree");
    // While a preparation holds the tree, an export does not read it.
    let held = begin_settled(&physical_root, &game);
    let busy = crate::archive::export_content_for(&spelled, GameFamily::Swbf2, &s.join("b.bfecontent"), Some(&tdir));
    assert!(busy.unwrap_err().contains("already running"));
    drop(held);
}

#[test]
fn relocation_makes_nothing_in_a_folder_it_does_not_own() {
    let s = Scratch::new("relocate");
    let game = install(&s.join("game"));
    let tdir = tools_dir(&s.join("tools"));
    // A 2004 copy at the inner location, with no lock file of its own.
    let root = s.join("content");
    data_2004(&swbf2_dir(&root));
    let before = snapshot(&root);
    let moved = crate::archive::relocate_preparation_for(&root, GameFamily::Swbf2, &game, Some(&tdir)).unwrap();
    assert!(!moved.ok);
    assert_eq!(snapshot(&root), before, "relocation made something in a folder it does not own");
}

#[test]
fn writing_requires_a_consistent_origin() {
    let scratch = Scratch::new("origin");
    let game = install(&scratch.join("game"));
    let tdir = tools_dir(&scratch.join("tools"));
    let source = scratch.join("source");
    tree(&swbf2_dir(&source), &game, same);
    let archive = scratch.join("healthy.bfecontent");
    let export = |root: &Path, dest: &Path| {
        crate::archive::export_content_for(root, GameFamily::Swbf2, dest, Some(&tdir))
    };
    export(&source, &archive).unwrap();
    let early = json!({"family":"swbf2","swbf2Version":3,"version":2,"game":game_path(&game),"done":[],"complete":false});
    let invalid = json!({"family":"swbf2","swbf2Version":null});
    let mut failures = Vec::new();
    for (case, permit) in [
        ("absent", true), ("empty", true), ("dedicated", true), ("early", true),
        ("two genuine", true), ("empty marker and receipt", true), ("marker directory", false),
        ("marker", false), ("empty marker", false), ("marker and files", false), ("lock only", false),
        ("visible 2004", false), ("current 2004", false), ("current with extensions", false),
        ("current with damaged optional data", false),
        #[cfg(unix)] ("dedicated link", false),
        ("dedicated directory", false), ("null", false), ("string", false), ("zero", false),
        ("future", false), ("negative", false), ("fractional", false), ("other family", false), ("malformed", false), ("early impostor", false),
        ("other marker", false), ("two games", false), ("2004 payload beside receipt", false),
        ("loose map file", false), ("loose side file", false),
        ("released map and receipt", false), ("released side and receipt", false), ("released common and receipt", false),
    ] {
        let root = scratch.join(case);
        let dir = swbf2_dir(&root);
        let save = |name: &str, v: Value| put(&dir, name, v.to_string().as_bytes());
        match case {
            "absent" => {}
            "empty" => fs::create_dir_all(&dir).unwrap(),
            "dedicated" => tree(&dir, &game, same),
            "early" => save(OLD_RECORD, early.clone()),
            "two genuine" => { tree(&dir, &game, same); save(OLD_RECORD, early.clone()); }
            "empty marker and receipt" => { tree(&dir, &game, same); put(&dir, SWBF2_MARKER_FILE, b""); }
            "marker directory" => { tree(&dir, &game, same); fs::create_dir_all(dir.join(SWBF2_MARKER_FILE)).unwrap(); }
            "negative" => save(SWBF2_PREPARE_STATE_FILE, json!({"family":"swbf2","swbf2Version":-1})),
            "fractional" => save(SWBF2_PREPARE_STATE_FILE, json!({"family":"swbf2","swbf2Version":1.5})),
            "released map and receipt" => { tree(&dir, &game, same); save("maps/zz1/map.json", json!({"world":{},"missions":[]})); }
            "released side and receipt" => { tree(&dir, &game, same); save("sides/qa/units.json", json!({"firstPersonBones":{},"wantedEffects":[]})); }
            "released common and receipt" => { tree(&dir, &game, same); save("common/common.json", json!({"holograms":[],"rasterHud":{}})); }
            "marker" => mark(&dir),
            "empty marker" => put(&dir, SWBF2_MARKER_FILE, b""),
            "marker and files" => { mark(&dir); put(&dir, "maps/example/map.json", b"{}"); }
            "lock only" => put(&dir, LOCK_FILE, b""),
            "visible 2004" => { data_2004(&dir); fs::remove_file(dir.join(OLD_RECORD)).unwrap(); }
            "current 2004" => data_2004(&dir),
            "current with extensions" | "current with damaged optional data" => {
                data_2004(&dir);
                let mut v: Value = serde_json::from_slice(&fs::read(dir.join(OLD_RECORD)).unwrap()).unwrap();
                v["family"] = json!("swbf2");
                v["swbf2Version"] = json!(3);
                if case == "current with damaged optional data" {
                    v["done"] = json!([]);
                    v["addOns"] = json!(7);
                }
                save(OLD_RECORD, v);
                mark(&dir);
            }
            #[cfg(unix)]
            "dedicated link" => {
                fs::create_dir_all(&dir).unwrap();
                link(swbf2_dir(&source).join(SWBF2_PREPARE_STATE_FILE), dir.join(SWBF2_PREPARE_STATE_FILE));
            }
            "dedicated directory" => fs::create_dir_all(dir.join(SWBF2_PREPARE_STATE_FILE)).unwrap(),
            "null" => save(SWBF2_PREPARE_STATE_FILE, invalid.clone()),
            "string" => save(SWBF2_PREPARE_STATE_FILE, json!({"family":"swbf2","swbf2Version":"3"})),
            "zero" => save(SWBF2_PREPARE_STATE_FILE, json!({"family":"swbf2","swbf2Version":0})),
            "future" => save(SWBF2_PREPARE_STATE_FILE, json!({"family":"swbf2","swbf2Version":GENERATION+1})),
            "other family" => save(SWBF2_PREPARE_STATE_FILE, json!({"family":"swbf1","swbf2Version":3})),
            "malformed" => put(&dir, SWBF2_PREPARE_STATE_FILE, b"{"),
            "early impostor" => save(OLD_RECORD, invalid.clone()),
            "other marker" => { tree(&dir, &game, same); save(SWBF2_MARKER_FILE, json!({"family":"swbf1"})); }
            "two games" => { tree(&dir, &game, same); data_2004(&dir); }
            "loose map file" | "loose side file" => {
                tree(&dir, &game, same);
                let group = if case == "loose map file" { "maps" } else { "sides" };
                put(&dir, &format!("{group}/copied-file"), b"preserve these bytes");
            }
            "2004 payload beside receipt" => { tree(&dir, &game, same); put(&dir, "common/shell/shell.json", b"{}"); }
            _ => unreachable!(),
        }
        let before = snapshot(&root);
        let result = settled(|| begin(&root, &game, &tools()));
        if result.is_ok() != permit {
            failures.push(case);
        }
        drop(result);
        if !permit {
            let backup = scratch.join("refused.bfecontent");
            let relocated = crate::archive::relocate_preparation_for(&root, GameFamily::Swbf2, &game, Some(&tdir)).unwrap();
            let imported = crate::archive::import_content_for(&archive, GameFamily::Swbf2, &dir, Some(&tdir));
            if relocated.ok || imported.is_ok() || export(&root, &backup).is_ok()
                || export(&source, &dir.join("backup.bfecontent")).is_ok()
                || backup.exists() || snapshot(&root) != before {
                failures.push(case);
            }
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn only_the_held_stdin_can_initialize_a_marker_and_lock() {
    let s = Scratch::new("handover-origin");
    let tree = locate(&s.join("pack")).unwrap();
    fs::create_dir_all(&tree.dir).unwrap();
    mark(&tree.dir);
    let lock = lock_new(&Ops::REAL, &tree).unwrap();
    let other = locate(&s.join("other")).unwrap();
    fs::create_dir_all(&other.dir).unwrap();
    let wrong = lock_new(&Ops::REAL, &other).unwrap();
    let tools = tools();
    let origin = |stdin: Option<&TreeLock>| write_origin(&Ops::REAL, &tree.dir, Some(&tools), stdin);
    assert!(origin(None).is_err());
    assert!(origin(Some(&wrong)).is_err());
    assert_eq!(origin(Some(&lock)).unwrap(), Origin::Fresh);
    put(&tree.dir, SWBF2_MARKER_FILE, br#"{"family":"swbf2","format":2}"#);
    assert!(origin(Some(&lock)).is_err());
    mark(&tree.dir);
    put(&tree.dir, "payload", b"keep");
    let before = snapshot(&tree.dir);
    assert!(origin(Some(&lock)).is_err());
    assert_eq!(snapshot(&tree.dir), before);
}

#[test]
fn a_required_product_cannot_be_certified_empty_or_omitted() {
    let s = Scratch::new("required");
    let game = install(&s.join("game"));
    let mut failures = Vec::new();
    for (group, file) in [
        ("map:zz1", "maps/zz1/map.json"), ("map:zz1", "maps/zz1/geom.bin"),
        ("side:qa", "sides/qa/units.json"), ("common", "common/common.json"),
        ("*", "audio/streams/voice.bin"),
    ] {
        let root = s.join("pack");
        let dir = swbf2_dir(&root);
        tree(&dir, &game, |v| {
            let rows = v["outputs"][group].as_array_mut().unwrap();
            rows.retain(|row| row[0] != file);
            rows.push(json!([file,0]));
        });
        put(&dir, file, b"");
        if verify(&root, &game, &tools()).unwrap() { failures.push(file); }
        if group != "*" {
            tree(&dir, &game, |v| v["outputs"][group].as_array_mut().unwrap().retain(|row| row[0] != file));
            if verify(&root, &game, &tools()).unwrap() { failures.push(file); }
            fs::remove_file(dir.join(file)).unwrap();
            if verify(&root, &game, &tools()).unwrap() { failures.push(file); }
        }
        fs::remove_dir_all(root).unwrap();
    }
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn a_stream_must_cover_the_byte_spans_in_its_index() {
    let s = Scratch::new("stream-span");
    let game = install(&s.join("game"));
    let root = s.join("pack");
    let dir = swbf2_dir(&root);
    let index = br#"{"segments":{"sample":[3,7,10,1]}}"#;
    let bytes = "audio/streams/example.bin";
    let metadata = "audio/streams/example.json";
    for length in [10, 9, 0] {
        tree(&dir, &game, |v| {
            let shared = v["outputs"]["*"].as_array_mut().unwrap();
            shared.push(json!([bytes, length]));
            shared.push(json!([metadata, index.len()]));
        });
        put(&dir, bytes, &vec![55; length]);
        put(&dir, metadata, index);
        assert_eq!(verify(&root, &game, &tools()).unwrap(), length == 10);
        if length != 10 {
            let receipt = fs::read(dir.join(SWBF2_PREPARE_STATE_FILE)).unwrap();
            let session = begin(&root, &game, &tools()).unwrap();
            assert_eq!(session.found, Swbf2Record::Resumable);
            assert_eq!(fs::read(dir.join(SWBF2_PREPARE_STATE_FILE)).unwrap(), receipt);
        }
    }
}

#[test]
fn an_install_with_missing_archives_cannot_authorize_a_clear() {
    let s = Scratch::new("install-markers");
    let root = s.join("pack");
    let prior = install(&s.join("prior"));
    tree(&swbf2_dir(&root), &prior, same);
    let selected = install(&s.join("selected"));
    fs::remove_file(selected.join("GameData/data/_lvl_pc/mission.lvl")).unwrap();
    let before = snapshot(&root);
    assert!(begin(&root, &selected, &tools()).is_err());
    assert_eq!(snapshot(&root), before);
}

#[test]
fn archive_destinations_preserve_existing_preparations() {
    let s = Scratch::new("archive-destination");
    let game = install(&s.join("game"));
    let source = s.join("source");
    tree(&swbf2_dir(&source), &game, same);
    let tdir = tools_dir(&s.join("tools"));
    let flat = s.join("flat");
    data_2004(&flat);
    let before = snapshot(&flat);
    let archive = s.join("copy.bfecontent");
    let export = |p: &Path| crate::archive::export_content_for(&source, GameFamily::Swbf2, p, Some(&tdir));
    assert!(export(&flat.join("maps/qq7/map.json")).is_err());
    assert_eq!(snapshot(&flat), before);
    assert!(!swbf2_dir(&source).join(LOCK_FILE).exists());
    export(&archive).unwrap();
    assert!(crate::archive::import_content_for(&archive, GameFamily::Swbf2, &flat, Some(&tdir)).is_err());
    assert_eq!(snapshot(&flat), before);
}
#[test]
fn an_interrupted_extractor_can_return_to_its_owned_workspace() {
    let s = Scratch::new("workspace");
    let root = s.join("content");
    let game = install(&s.join("game"));
    let dir = swbf2_dir(&root);
    tree(&dir, &game, same);
    put(&dir, ".locks/products-x_a9b2c4/common/common.json", b"{}");
    assert!(begin(&root, &game, &tools()).is_ok());
}
