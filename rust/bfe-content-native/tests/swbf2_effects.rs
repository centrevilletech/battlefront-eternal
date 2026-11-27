//! Optional admission check against a locally prepared Battlefront II installation.
use bfe_content_native::{GameFamily, Loader};

#[test]
#[ignore = "requires BFE_SWBF2_CONTENT with prepared mission effects"]
fn opening_missions_admit_particle_tracks_images_and_models() {
    let root = std::env::var_os("BFE_SWBF2_CONTENT").expect("set BFE_SWBF2_CONTENT");
    for (map, mission) in [("tat2", "tat2g_eli"), ("spa6", "spa6c_ass")] {
        let loader = Loader::detect(&root);
        let query = format!("?mission={mission}");
        let battle = loader
            .load_for_family(GameFamily::Swbf2, map, &query)
            .unwrap();
        let library = &battle.effects[0].1;
        let content: bfe_presentation::Content =
            serde_json::from_value(library.descriptor.clone()).unwrap();
        assert!(!content.definitions.is_empty());
        let groups = battle
            .visual_effects()
            .get("effects")
            .unwrap()
            .as_object()
            .unwrap();
        assert!(groups
            .keys()
            .all(|name| library.handles.iter().any(|(n, _)| n == name)));
        let missing = battle.visual_effects().get("missing").unwrap();
        let missing_assets = |kind| missing.get(kind).unwrap().as_array().unwrap();
        for def in library.descriptor["definitions"].as_array().unwrap() {
            let g = &def["geometry"];
            if let Some(name) = g["texture"].as_str() {
                assert!(
                    battle.texture_paths.contains_key(&name.to_lowercase())
                        || missing_assets("textures")
                            .iter()
                            .any(|n| n.as_str() == Some(name)),
                    "{name}"
                );
            }
            if let Some(name) = g["model"].as_str() {
                assert!(
                    battle
                        .models
                        .iter()
                        .any(|m| m.name.starts_with(&format!("{name}/")))
                        || missing_assets("models")
                            .iter()
                            .any(|n| n.as_str() == name.strip_prefix("effects/")),
                    "{name}"
                );
            }
        }
        println!(
            "{map}: {} groups, {} particle definitions, {} analytic tracks, {} placed effects",
            library.handles.len(),
            content.definitions.len(),
            library.gpu_indices.iter().flatten().count(),
            battle.environment_instances.len()
        );
        // A presentation-only pack must preserve the class and ordnance IDs sent by the host.
        let loaded = battle.defs().unwrap();
        use bfe_content::{
            battle::{Battle, Loader as BattleLoader},
            content::Content,
            files::DirFiles,
        };
        let files = DirFiles::from_resolved_root(&root, bfe_family::GameFamily::Swbf2);
        let authoritative =
            Battle::load_for_family(&BattleLoader::new(&files), GameFamily::Swbf2, map, &query)
                .unwrap();
        let host = bfe_content::registry::entity_registry(&Content::build(authoritative).unwrap())
            .unwrap();
        assert_eq!(host.classes, loaded.registry.classes);
        assert_eq!(host.weapons, loaded.registry.weapons);
        assert_eq!(host.posts, loaded.registry.posts);
        assert!(host.combat.iter().map(|row| &row.0).eq(loaded
            .registry
            .combat
            .iter()
            .map(|row| &row.0)));
        println!(
            "{map}: {} matching host projectile and explosion IDs",
            host.combat.len()
        );
    }
}
