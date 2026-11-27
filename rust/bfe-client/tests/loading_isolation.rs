//! The battle client reads the selected map's prepared backdrop. Only the
//! standalone preview may override the map or the tip.
#[test]
fn battle_client_uses_prepared_loading_art_without_preview_overrides() {
    let client = std::fs::read(env!("CARGO_BIN_EXE_bfe-client")).unwrap();
    let contains = |text: &str| {
        client
            .windows(text.len())
            .any(|bytes| bytes == text.as_bytes())
    };
    assert!(contains("common/shell/load2.json"));
    for marker in ["BFE_LOAD2_TIP", "load2map"] {
        assert!(!contains(marker), "{marker}");
    }
}
