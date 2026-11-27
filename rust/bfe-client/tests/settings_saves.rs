//! Native player settings persistence.
use bfe_client::app::settings::AppSettingsStore;
use bfe_desktop::paths::{resolve_desktop_paths, DesktopPaths, DesktopPathsOptions};
use std::{collections::BTreeMap, fs, sync::Barrier, thread};

fn profile_paths(test: &str) -> DesktopPaths {
    let root = std::env::temp_dir().join(format!("bfe-profile-{test}-{}", std::process::id()));
    resolve_desktop_paths(&DesktopPathsOptions {
        home: Some(root.clone()),
        env: BTreeMap::from([("BFE_USER_DATA_DIR".into(), root.to_str().unwrap().into())]),
        ..Default::default()
    })
}

#[test]
fn two_clients_can_save_to_the_same_settings_folder() {
    let paths = profile_paths("writers");
    let root = paths.user_data_dir.clone();
    let mut clients = ["amber".repeat(512), "birch".repeat(512)].map(|name| {
        let mut client = AppSettingsStore::load(paths.clone());
        client.player.profile_name = name;
        client
    });
    let expected = clients.each_mut().map(|client| {
        client.save_player().unwrap();
        fs::read(&paths.player_settings_file).unwrap()
    });
    assert_ne!(expected[0], expected[1]);

    let start = Barrier::new(2);
    thread::scope(|scope| {
        let writers = clients.map(|mut client| {
            let (start, expected, path) = (&start, &expected, &paths.player_settings_file);
            scope.spawn(move || {
                let mut errors = Vec::new();
                for _ in 0..64 {
                    start.wait();
                    if let Err(e) = client.save_player() {
                        errors.push(e);
                    }
                    match fs::read(path) {
                        Ok(bytes) if expected.contains(&bytes) => {}
                        _ => errors.push("reader observed an incomplete profile".into()),
                    }
                }
                errors
            })
        });
        for writer in writers {
            let errors = writer.join().unwrap();
            assert!(errors.is_empty(), "{} failures, including {:?}", errors.len(), errors.first());
        }
    });
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn saving_after_a_failed_read_keeps_the_original_profile_bytes() {
    let paths = profile_paths("unreadable");
    fs::create_dir_all(&paths.user_data_dir).unwrap();
    let file = &paths.player_settings_file;
    let backup = file.with_extension("json.bak");
    let next = file.with_extension("json.bak.1");

    // Valid JSON with the wrong type for a playlist, and a damaged non-text file.
    let partial = br#"{ "profileName": "Blue Finch", "iaRandom": true, "iaPlaylist": {"lost":42} }"#;
    let mut damaged = partial.to_vec();
    damaged[0] = 0xff;
    let records: [&[u8]; 2] = [partial, &damaged];
    let mut defaults = AppSettingsStore::load(paths.clone());
    defaults.save_player().unwrap();
    let expected = fs::read(file).unwrap();
    for (index, original) in [records[0], records[0], records[1], records[0]].iter().enumerate() {
        fs::write(file, original).unwrap();
        let mut store = AppSettingsStore::load(paths.clone());
        assert_eq!(fs::read(file).unwrap(), *original);
        if index == 0 {
            assert!(!backup.exists());
        }
        store.save_player().unwrap();
        assert_eq!(fs::read(file).unwrap(), expected);
        assert_eq!(fs::read(&backup).ok().as_deref(), Some(records[0]));
        if index < 2 {
            assert!(!next.exists());
        } else {
            assert_eq!(fs::read(&next).unwrap(), records[1]);
        }
    }
    assert!(!file.with_extension("json.bak.2").exists());
    fs::remove_dir_all(paths.user_data_dir).unwrap();
}

#[test]
fn missing_empty_and_readable_profiles_have_no_backup() {
    let paths = profile_paths("readable");
    let file = &paths.player_settings_file;
    let backup = file.with_extension("json.bak");
    let mut missing = AppSettingsStore::load(paths.clone());
    missing.save_player().unwrap();
    let expected = fs::read(file).unwrap();
    fs::write(file, []).unwrap();
    AppSettingsStore::load(paths.clone()).save_player().unwrap();
    assert_eq!(fs::read(file).unwrap(), expected);
    assert!(!backup.exists());

    fs::write(file, br#"{ "profileName": "Amber Finch", "extension": 9 }"#).unwrap();
    let mut readable = AppSettingsStore::load(paths.clone());
    assert_eq!(readable.player.profile_name, "Amber Finch");
    readable.save_player().unwrap();
    let saved = fs::read(file).unwrap();
    AppSettingsStore::load(paths.clone()).save_player().unwrap();
    assert_eq!(fs::read(file).unwrap(), saved);
    assert!(!backup.exists());
    fs::remove_dir_all(paths.user_data_dir).unwrap();
}

#[test]
fn concurrent_saves_preserve_damage_that_happened_after_loading() {
    let paths = profile_paths("changed");
    fs::create_dir_all(&paths.user_data_dir).unwrap();
    let file = &paths.player_settings_file;
    let clients = ["Mallow", "Sorrel"].map(|name| {
        let mut store = AppSettingsStore::load(paths.clone());
        store.player.profile_name = name.into();
        store
    });
    let original = b"{\"profileName\":\"Oriole\",";
    let older = b"another saved copy";
    fs::write(file, original).unwrap();
    fs::write(file.with_extension("json.bak"), older).unwrap();

    let start = Barrier::new(2);
    thread::scope(|scope| {
        for mut client in clients {
            let start = &start;
            scope.spawn(move || {
                start.wait();
                client.save_player().unwrap();
            });
        }
    });
    assert_eq!(fs::read(file.with_extension("json.bak")).unwrap(), older);
    assert_eq!(fs::read(file.with_extension("json.bak.1")).unwrap(), original);
    assert!(!file.with_extension("json.bak.2").exists());
    let name = AppSettingsStore::load(paths.clone()).player.profile_name;
    assert!(["Mallow", "Sorrel"].contains(&name.as_str()));
    fs::remove_dir_all(paths.user_data_dir).unwrap();
}

#[cfg(unix)]
#[test]
fn a_save_keeps_a_profile_that_cannot_be_opened() {
    use std::os::unix::fs::PermissionsExt;
    let paths = profile_paths("access");
    fs::create_dir_all(&paths.user_data_dir).unwrap();
    let file = &paths.player_settings_file;
    let original = b"damaged record";
    fs::write(file, original).unwrap();
    fs::set_permissions(file, fs::Permissions::from_mode(0o000)).unwrap();
    let mut store = AppSettingsStore::load(paths.clone());
    // A privileged account can still open the file.
    if fs::File::open(file).is_err() {
        assert!(store.save_player().is_err());
    }
    fs::set_permissions(file, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(fs::read(file).unwrap(), original);
    store.save_player().unwrap();
    assert_eq!(fs::read(file.with_extension("json.bak")).unwrap(), original);
    fs::remove_dir_all(paths.user_data_dir).unwrap();
}

#[test]
fn unreadable_file_backed_interface_settings_survive_an_edit() {
    use bfe_interface::Settings;
    let paths = profile_paths("interface");
    fs::create_dir_all(&paths.user_data_dir).unwrap();
    let file = &paths.player_settings_file;
    let backup = file.with_added_extension("bak");
    for readable in [br#"{"values":42}"#.as_slice(), br#"{"complete":false,"values":{}}"#] {
        fs::write(file, readable).unwrap();
        Settings::load_from_file(file).save_to_file(file).unwrap();
        assert!(!backup.exists());
    }
    fs::write(file, b"[]").unwrap();
    let mut settings = Settings::load_from_file(file);
    settings.set("mouseSensitivity", 0.75);
    assert!(serde_json::from_slice::<serde_json::Value>(&fs::read(file).unwrap()).unwrap().is_object());
    assert_eq!(fs::read(backup).ok().as_deref(), Some(&b"[]"[..]));
    fs::remove_dir_all(paths.user_data_dir).unwrap();
}

#[test]
fn unreadable_online_notice_survives_acceptance() {
    use bfe_online::{try_read_online_consent, write_online_consent_at};
    let paths = profile_paths("notice");
    fs::create_dir_all(&paths.user_data_dir).unwrap();
    let file = paths.user_data_dir.join("online-consent.json");
    let backup = file.with_added_extension("bak");
    let older_version = br#"{"version":"earlier","acceptedAt":3}"#;
    fs::write(&file, older_version).unwrap();
    assert_eq!(try_read_online_consent(&file), Ok(false));
    write_online_consent_at(&file, 42).unwrap();
    assert!(!backup.exists());
    let damaged = br#"{"version":[]}"#;
    fs::write(&file, damaged).unwrap();
    write_online_consent_at(&file, 42).unwrap();
    assert_eq!(try_read_online_consent(&file), Ok(true));
    assert_eq!(fs::read(backup).ok().as_deref(), Some(&damaged[..]));
    fs::remove_dir_all(paths.user_data_dir).unwrap();
}
