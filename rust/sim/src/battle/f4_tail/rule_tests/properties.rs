use super::*;
use rules::events::{ObjectProperty, RuleError::*};

#[test]
fn rejected_object_writes_leave_the_command_available() {
    let mut w = world();
    let (_, h) = prop(&mut w, false);
    let mut stale = h.clone();
    stale.life += 1.;
    let object = ObjectHandle::Prop(h);
    prepare_tail(&mut w);
    callback(&mut w, 1, move |_, a| {
        assert_eq!(a.set_health(&object, -1.), Err(InvalidValue));
        assert_eq!(a.kill_object(&ObjectHandle::Prop(stale.clone())), Err(StaleHandle));
        assert_eq!(a.set_property(&object, ObjectProperty::Locked(true)), Err(NotAvailable));
        assert_eq!(a.set_health(&object, 40.), Ok(()));
        assert_eq!(a.kill_object(&object), Err(LimitReached));
        assert_eq!(a.object_health(&object), Ok((40., 100.)));
    });
    tick(&mut w, 1.);
}

#[test]
fn maximum_health_must_be_positive_before_repairing_a_destroyed_object() {
    for trap in [false, true] {
        let mut w = world();
        let (_, h) = prop(&mut w, trap);
        prepare_tail(&mut w);
        let object = ObjectHandle::Prop(h.clone());
        callback(&mut w, 12, move |_, a| {
            assert_eq!(a.object_alive(&object), Ok(false));
            for invalid in [0., -0., -1., f64::NAN, f64::INFINITY] {
                assert_eq!(a.set_property(&object, ObjectProperty::MaxHealth(invalid)), Err(InvalidValue));
            }
            assert_eq!(a.object_health(&object), Ok((0., 100.)));
            assert_eq!(a.set_property(&object, ObjectProperty::MaxHealth(50.)), Ok(()));
            assert_eq!(a.set_health(&object, 1.), Ok(()));
            assert_eq!(a.object_alive(&object), Ok(false));
            assert_eq!(a.object_health(&object), Ok((1., 50.)));
        });
        f3::with_services_in(&mut w, |w, s| crate::units::f3_props::command_hit_in(w, 0, 0., None, None, s.unwrap()));
        if trap {
            live_step(&mut w, 1.);
            live_step(&mut w, 2.);
        }
        tick(&mut w, 1.);
        assert!(!crate::battle::f4::frame_in(&w.battle, &h).alive());
    }
}

#[test]
fn post_team_through_the_building_resets_capture_clocks() { check_post_assignment(true); }

#[test]
fn post_team_through_the_post_updates_its_building() { check_post_assignment(false); }

fn check_post_assignment(use_building: bool) {
    let mut w = world();
    soldier(&mut w, 2.);
    let (building, handle) = prop(&mut w, false);
    let post = w.sim.store.post.insert(Post {
        active: true, team: 1., building: Some(building),
        capture: Some(Built { r#type: RegionType::Sphere, radius: 5., ..Default::default() }),
        neutralize_time: 10., capture_time: 10., progress: 0.75, toward: 2., neutralize: 0.5,
        ..Default::default()
    });
    w.sim.store.command_posts.posts.push(post);
    prepare_tail(&mut w);
    let prop = ObjectHandle::Prop(handle);
    let placed = ObjectHandle::Post(SpawnPost::Post(post));
    callback(&mut w, 1, move |batch, a| {
        assert!(batch.events().is_empty());
        let target = if use_building { &prop } else { &placed };
        a.set_property(target, ObjectProperty::Team(2.)).unwrap();
        assert_eq!((a.object_team(&prop), a.object_team(&placed)), (Ok(2.), Ok(2.)));
    });
    tick(&mut w, 1. / 60.);
    let p = w.sim.store.post.get(post).unwrap();
    assert_eq!((p.team, p.progress, p.toward, p.neutralize), (2., 0., 0., 0.));
}
