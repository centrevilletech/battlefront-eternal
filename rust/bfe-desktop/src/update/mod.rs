//! Desktop auto-updater: SemVer comparison, Ed25519 channel envelope verification,
//! Zstandard manifest and content-addressed blob installation, version state, and
//! boot handoff.
//!
//! Mirrors the desktop host{version,keys,state,tree,feed,install,launch,index}.js`.

pub mod feed;
pub mod handoff;
pub mod install;
pub mod keys;
pub mod state;
pub mod tree;
pub mod version;

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};

    use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
    use ed25519_dalek::{Signer, SigningKey};

    use crate::launch::{settle_with, SettleOptions};
    use crate::update::feed::{
        decode_and_validate_manifest, encode_zstd_store_frame, verify_envelope, ChannelPayload,
        ChannelPlatformEntry, ChannelRelease, ManifestFileEntry, PlatformManifest,
        SignedChannelEnvelope,
    };
    use crate::update::handoff::{
        check_and_install, hand_off, CheckOutcome, UpdaterContext,
    };
    use crate::update::install::InstallProgress;
    use crate::update::keys::{encode_spki_pem, sha256_key_id_for_test};
    use crate::update::state::{
        choose_launch, installed_versions, read_state, record_attempt, versions_dir,
        INSTALL_RECORD,
    };
    use crate::update::tree::sha256_bytes;

    fn make_temp_dir(label: &str) -> PathBuf {
        let mut nonce = [0u8; 6];
        let _ = getrandom::getrandom(&mut nonce);
        let hex: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
        let dir = env::temp_dir().join(format!("bfe-update-test-{label}-{hex}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[allow(clippy::too_many_arguments)]
    fn write_published_release(
        http_root: &Path,
        signing_key: &SigningKey,
        key_id: &str,
        channel: &str,
        release_id: &str,
        version: &str,
        platform: &str,
        files_spec: &[(&str, &[u8], u32)],
    ) -> serde_json::Value {
        let mut manifest_files = Vec::new();
        let mut total_size = 0u64;

        for (rel_path, raw_bytes, mode) in files_spec {
            let sha = sha256_bytes(raw_bytes);
            let zst = encode_zstd_store_frame(raw_bytes);
            let blob_rel = format!("updates/blobs/{}/{sha}.zst", &sha[..2]);
            let blob_full = http_root.join(&blob_rel);
            fs::create_dir_all(blob_full.parent().unwrap()).unwrap();
            fs::write(&blob_full, &zst).unwrap();

            total_size += raw_bytes.len() as u64;
            manifest_files.push(ManifestFileEntry {
                path: (*rel_path).to_string(),
                size: raw_bytes.len() as u64,
                sha256: sha,
                mode: Some(*mode),
                blob: Some(blob_rel),
                compressed_size: Some(zst.len() as u64),
            });
        }

        let manifest = PlatformManifest {
            version: version.to_string(),
            release_id: Some(release_id.to_string()),
            revision: Some("deadbeef".to_string()),
            platform: platform.to_string(),
            executable: files_spec[0].0.to_string(),
            server_executable: None,
            files: manifest_files,
            links: Vec::new(),
            dirs: Vec::new(),
        };
        let manifest_json = serde_json::to_vec(&manifest).unwrap();
        let manifest_zst = encode_zstd_store_frame(&manifest_json);
        let manifest_sha = sha256_bytes(&manifest_zst);
        let manifest_rel = format!("{release_id}/update-{platform}.json.zst");
        let manifest_full = http_root.join(&manifest_rel);
        fs::create_dir_all(manifest_full.parent().unwrap()).unwrap();
        fs::write(&manifest_full, &manifest_zst).unwrap();

        let mut platforms = BTreeMap::new();
        platforms.insert(
            platform.to_string(),
            ChannelPlatformEntry {
                manifest: manifest_rel,
                sha256: manifest_sha,
                size: manifest_zst.len() as u64,
                total_size: Some(total_size),
            },
        );

        let payload = ChannelPayload {
            channel: channel.to_string(),
            published_at: Some("2026-11-08T00:00:00Z".to_string()),
            release: ChannelRelease {
                id: release_id.to_string(),
                version: version.to_string(),
                revision: Some("deadbeef".to_string()),
                platforms,
            },
        };
        let payload_bytes = serde_json::to_vec(&payload).unwrap();
        let sig = signing_key.sign(&payload_bytes);
        let env_obj = SignedChannelEnvelope {
            key: key_id.to_string(),
            payload: BASE64_STANDARD.encode(&payload_bytes),
            signature: BASE64_STANDARD.encode(sig.to_bytes()),
        };
        let env_val = serde_json::to_value(&env_obj).unwrap();
        let channel_path = http_root.join("updates").join(format!("{channel}.json"));
        fs::create_dir_all(channel_path.parent().unwrap()).unwrap();
        fs::write(
            &channel_path,
            serde_json::to_vec_pretty(&env_val).unwrap(),
        )
        .unwrap();
        env_val
    }

    #[test]
    fn synthetic_signed_channel_verify_apply_reuse_settle_and_rollback() {
        let work = make_temp_dir("full-flow");
        let http_root = work.join("http");
        let update_root = work.join("root");
        let boot_dir = work.join("bootstrap");
        fs::create_dir_all(boot_dir.join("resources")).unwrap();

        let signing_key = SigningKey::from_bytes(&[0x42u8; 32]);
        let verifying_key = signing_key.verifying_key();
        let pem = encode_spki_pem(&verifying_key);
        let key_id = sha256_key_id_for_test(&verifying_key);
        let mut keys = BTreeMap::new();
        keys.insert(key_id.clone(), pem);

        // Seed bootstrap 0.1.0-preview.331 with a shared asset file in bfe-tree.json.
        let shared_bytes = b"shared-asset-content-across-releases";
        let shared_sha = sha256_bytes(shared_bytes);
        fs::write(boot_dir.join("resources/shared.bin"), shared_bytes).unwrap();
        fs::write(
            boot_dir.join("resources/bfe-release.json"),
            r#"{"version":"0.1.0-preview.331","channel":"development-preview"}"#,
        )
        .unwrap();
        fs::write(
            boot_dir.join("resources/bfe-tree.json"),
            format!(
                r#"{{"version":"0.1.0-preview.331","platform":"linux-x64","files":[{{"path":"resources/shared.bin","size":{},"sha256":"{shared_sha}","mode":420}}]}}"#,
                shared_bytes.len()
            ),
        )
        .unwrap();

        // Publish 0.1.0-preview.332 on the channel.
        let exe_script = b"#!/bin/sh\necho $$ > \"$BFE_LAUNCH_DIR/$BFE_LAUNCH_TOKEN\"\nsleep 1\n";
        let rel_json_332 = br#"{"version":"0.1.0-preview.332","channel":"development-preview"}"#;
        let envelope = write_published_release(
            &http_root,
            &signing_key,
            &key_id,
            "development-preview",
            "preview-332",
            "0.1.0-preview.332",
            "linux-x64",
            &[
                ("bfe-client", exe_script, 0o755),
                ("resources/shared.bin", shared_bytes, 0o644),
                ("resources/bfe-release.json", rel_json_332, 0o644),
            ],
        );

        // 1. Verify signed envelope and reject tampered payload or unknown key.
        let verified = verify_envelope(&envelope, &keys).unwrap();
        assert_eq!(verified.channel, "development-preview");
        assert_eq!(verified.release.version, "0.1.0-preview.332");

        let mut tampered = envelope.clone();
        tampered["payload"] = serde_json::Value::String(BASE64_STANDARD.encode(b"{}"));
        assert!(verify_envelope(&tampered, &keys).is_err());

        let compressed_manifest =
            fs::read(http_root.join("preview-332/update-linux-x64.json.zst")).unwrap();
        let entry = &verified.release.platforms["linux-x64"];
        let decoded_manifest = decode_and_validate_manifest(
            &compressed_manifest,
            &entry.sha256,
            "0.1.0-preview.332",
            "linux-x64",
        )
        .unwrap();
        assert_eq!(decoded_manifest.files.len(), 3);

        // 2. Apply update via check_and_install, reusing resources/shared.bin from bootstrap.
        let ctx = UpdaterContext {
            own_dir: boot_dir.clone(),
            own_version: None,
            own_channel: None,
            root: Some(update_root.clone()),
            feed_url: Some(format!("file://{}", http_root.display())),
            keys: keys.clone(),
            platform: "linux".to_string(),
            arch: "x64".to_string(),
            argv: vec!["bfe-client".to_string()],
            is_packaged: true,
            env: BTreeMap::new(),
            home: work.clone(),
            launch_timeout_ms: 5000,
            fetch_timeout_ms: 5000,
            blob_timeout_ms: 5000,
            concurrency: 2,
        };

        let check_res = check_and_install::<fn(InstallProgress)>(&ctx, None);
        match check_res {
            CheckOutcome::Installed {
                version,
                reused,
                downloaded,
                ..
            } => {
                assert_eq!(version, "0.1.0-preview.332");
                assert_eq!(reused, 1);
                assert_eq!(downloaded, 2);
            }
            other => panic!("expected Installed, got {other:?}"),
        }

        let installed = installed_versions(&update_root, Some("linux-x64"));
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].version, "0.1.0-preview.332");
        assert!(versions_dir(&update_root)
            .join("0.1.0-preview.332")
            .join(INSTALL_RECORD)
            .exists());

        // 3. Hand off from 331 to 332 (on Unix, runs the script which writes BFE_LAUNCH_TOKEN).
        #[cfg(unix)]
        {
            let ho = hand_off(&ctx);
            assert!(ho.handed_off);
            assert_eq!(ho.target.unwrap().version, "0.1.0-preview.332");
        }

        // 4. Settle 0.1.0-preview.332: marks healthy in state.json and updates current symlink.
        let dir_332 = versions_dir(&update_root).join("0.1.0-preview.332");
        let settle_res = settle_with(&SettleOptions {
            own_dir: dir_332,
            own_version: None,
            own_channel: None,
            root: Some(update_root.clone()),
            bootstrap_version: Some("0.1.0-preview.331".to_string()),
            platform: "linux".to_string(),
            arch: "x64".to_string(),
            env: BTreeMap::new(),
            home: work.clone(),
        });
        assert!(settle_res.settled);
        let st = read_state(&update_root);
        assert_eq!(st.healthy, vec!["0.1.0-preview.332".to_string()]);
        assert!(!st.attempts.contains_key("0.1.0-preview.332"));

        // 5. Rollback after MAX_ATTEMPTS (2) failed launches of a broken 0.1.0-preview.333.
        let bad_exe = b"#!/bin/sh\nexit 1\n";
        let rel_json_333 = br#"{"version":"0.1.0-preview.333","channel":"development-preview"}"#;
        write_published_release(
            &http_root,
            &signing_key,
            &key_id,
            "development-preview",
            "preview-333",
            "0.1.0-preview.333",
            "linux-x64",
            &[
                ("bfe-client", bad_exe, 0o755),
                ("resources/shared.bin", shared_bytes, 0o644),
                ("resources/bfe-release.json", rel_json_333, 0o644),
            ],
        );

        let check_333 = check_and_install::<fn(InstallProgress)>(&ctx, None);
        assert!(matches!(check_333, CheckOutcome::Installed { .. }));

        // Simulate two failed boot attempts on 0.1.0-preview.333; after attempt 2,
        // 0.1.0-preview.333 is marked `bad` and `choose_launch` rolls back to 0.1.0-preview.332.
        record_attempt(&update_root, "0.1.0-preview.333").unwrap();
        assert_eq!(
            choose_launch(&update_root, "0.1.0-preview.331", Some("linux-x64"), None)
                .unwrap()
                .version,
            "0.1.0-preview.333"
        );
        record_attempt(&update_root, "0.1.0-preview.333").unwrap();
        let st_after_fail = read_state(&update_root);
        assert!(st_after_fail.bad.contains(&"0.1.0-preview.333".to_string()));
        assert_eq!(
            choose_launch(&update_root, "0.1.0-preview.331", Some("linux-x64"), None)
                .unwrap()
                .version,
            "0.1.0-preview.332"
        );

        let _ = fs::remove_dir_all(&work);
    }
}
