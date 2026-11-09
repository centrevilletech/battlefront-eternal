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
