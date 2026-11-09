// A themed view supplies rendered regions; clicks still traverse the
// same input queue and model as the perspective Classic controls.
#[test]
fn themed_spawn_regions_select_class_post_and_commit() {
    let mut port = TestSpawnPort::new();
    let mut ctrl = SpawnScreenController::new(624.);
    ctrl.open(&mut port, OpenReason::Start, 624.);
    ctrl.view.theme_hotspots = Some(vec![
        ([20., 100., 250., 160.], SpawnHitTarget::Class(1)),
        ([700., 300., 730., 330.], SpawnHitTarget::PostIndex(0)),
        ([330., 560., 640., 610.], SpawnHitTarget::Spawn),
    ]);
    for (point, expected) in [
        ([80., 130.], SpawnHitTarget::Class(1)),
        ([715., 315.], SpawnHitTarget::PostIndex(0)),
    ] {
        assert_eq!(
            ctrl.view
                .hit(point[0], point[1], 624., 15., &ctrl.model, &port),
            Some(expected.clone())
        );
        ctrl.input
            .click(&ctrl.model, point[0], point[1], 624., true, |_, _| {
                Some(expected.clone())
            });
        ctrl.update(0.016, &mut port, 624., 15., false, 0.);
    }
    assert_eq!(ctrl.model.class_index, 1);
    assert_eq!(ctrl.model.post.as_deref(), Some("cp_1"));
    let (_, action) = ctrl
        .input
        .click(&ctrl.model, 400., 585., 624., true, |_, _| {
            Some(SpawnHitTarget::Spawn)
        });
    assert_eq!(action, SpawnInputAction::Spawn);
    let choice = ctrl.spawn(&mut port).expect("eligible themed selection");
    assert_eq!(choice.class_name, "rep_class_1");
    assert_eq!(choice.post, "cp_1");
}

// humanSpawnPost compares squared 3D distances, retaining an eligible pick
// and moving a lost pick to the nearest eligible post without flattening Y.
#[test]
fn front_line_and_lost_posts_include_height() {
    let post = |name: &str, position, owner, eligible| SpawnPostEntry {
        name: name.into(),
        position,
        owner,
        eligible,
        ..SpawnPostEntry::default()
    };
    let posts = [
        post("low", [1., 0., 0.], 1, true),
        post("high", [8., 100., 0.], 1, true),
        post("foe", [0., 100., 0.], 2, false),
    ];
    assert_eq!(
        choose_frontline_post(&posts, 1, None).as_deref(),
        Some("high")
    );
    assert_eq!(
        choose_frontline_post(&posts, 1, Some("foe")).as_deref(),
        Some("high")
    );
    assert_eq!(
        choose_frontline_post(&posts, 1, Some("low")).as_deref(),
        Some("low")
    );
}

#[test]
fn post_distance_rounding_keeps_the_first_tied_choice() {
    let post = |name: &str, position, owner, eligible| SpawnPostEntry {
        name: name.into(),
        position,
        owner,
        eligible,
        ..SpawnPostEntry::default()
    };
    // 4096 squared plus one and 4096 squared are equal after f32 rounding.
    // humanSpawnPost uses a strict comparison, so the first post must stay.
    let posts = [
        post("first", [4096., 0., 1.], 1, true),
        post("second", [4096., 0., 0.], 1, true),
        post("foe", [0., 0., 0.], 2, false),
    ];
    assert_eq!(
        choose_frontline_post(&posts, 1, None).as_deref(),
        Some("first")
    );
}
