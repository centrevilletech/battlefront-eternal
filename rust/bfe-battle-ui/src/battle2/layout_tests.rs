use super::*;
use crate::OwnedUiSurface;
use bfe_render::hud_model2::{catalog::Catalog, Layout};

fn radar() -> (Screens, OwnedUiSurface, BattleView<'static>) {
    let mut catalog = Catalog::default();
    catalog.layout = Layout::read(
        br#"{"file":"1playerhud","entries":[{"name":"map","values":["player1map"],"children":[
          {"name":"position","values":[230,170]},
          {"name":"BackdropSmall","values":["BackdropSmall"],"children":[{"name":"MASK","values":["disc"]},{"name":"texcoords","values":[0.12,0.14,0.62,0.64]}]},
          {"name":"PlayerFOVSmall","values":["PlayerFOVSmall"],"children":[{"name":"Mesh","values":["beam"]}]},
          {"name":"PlayerDirectionSmall","values":["PlayerDirectionSmall"],"children":[{"name":"Mesh","values":["pointer"]},{"name":"alpha","values":[0.23]}]},
          {"name":"NorthSmall","values":["NorthSmall"],"children":[{"name":"position","values":[0,-83]},{"name":"Bitmap","values":["north"]}]}
        ]},{"name":"group","values":["player1spaceassaultgroup"],"children":[
          {"name":"position","values":[170,90]},{"name":"Rect","values":[150,110]}
        ]}]}"#.as_slice(),
        br#"{"file":"hudtransforms","entries":[]}"#.as_slice(),
    ).unwrap();
    let mut surface = OwnedUiSurface::default();
    for (id, key) in ["paper", "disc", "beam", "pointer", "north", "contact"]
        .iter()
        .enumerate()
    {
        surface.textures.insert((*key).into(), 800 + id as u32);
    }
    let mut view = super::screens_tests::view();
    view.body = Some(Body {
        subject: Subject::default(),
        role: Role::Trooper,
        health: Gauge::default(),
        stamina: Gauge::default(),
        blocking: false,
        map: [Sample::steady(0.5); 2],
        yaw: Sample::default(),
        slots: [None; 2],
        reticle: "",
    });
    (Screens::new(catalog, "eternal"), surface, view)
}

#[test]
fn the_contact_limit_applies_inside_the_visible_circle() {
    let (mut screens, mut surface, base) = radar();
    let near = Contact {
        subject: Subject::default(),
        team: 12,
        map: [Sample::steady(0.51); 2],
        icon: "contact",
    };
    let far = Contact {
        map: [Sample::steady(0.9); 2],
        ..near
    };
    for (a, b) in [(far, near), (near, far)] {
        let contacts = [vec![a; 150], vec![b; 150]].concat();
        let view = BattleView {
            contacts: &contacts,
            ..base
        };
        surface.batch.begin();
        screens.draw(&mut surface, &view, [1024., 768.], 0.);
        let count: usize = surface
            .batch
            .draws
            .iter()
            .filter(|d| d.texture == 805)
            .map(|d| d.count / 6)
            .sum();
        assert_eq!(count, 128);
    }
}

fn quad(surface: &OwnedUiSurface, texture: u32) -> usize {
    surface
        .batch
        .draws
        .iter()
        .find(|d| d.texture == texture)
        .unwrap_or_else(|| panic!("missing texture {texture}"))
        .first
        / 6
}

fn plane_corner(surface: &OwnedUiSurface, q: usize, corner: usize) -> [f32; 2] {
    let i = (q * 4 + corner) * 3;
    [
        512. + surface.batch.position[i],
        384. - surface.batch.position[i + 1],
    ]
}

#[test]
fn the_map_contacts_and_north_turn_under_the_upright_cone() {
    use std::f32::consts::{FRAC_PI_2, PI};
    let (mut screens, mut surface, mut view) = radar();
    let contact = Contact {
        subject: Subject::default(),
        team: 12,
        map: [0.625, 0.5].map(Sample::steady),
        icon: "contact",
    };
    view.contacts = std::slice::from_ref(&contact);
    let mut cone = None;
    // The contact lies half way to the disc's rim.
    let half = (0.25 * super::playing::MAP_ACROSS) as f32;
    for (yaw, dot, north) in [
        (Sample::steady(0.), [230. + half, 170.], [230., 87.]),
        (Sample::steady(FRAC_PI_2), [230., 170. - half], [147., 170.]),
        (
            Sample {
                previous: PI - 0.1,
                current: -PI + 0.1,
            },
            [230. - half, 170.],
            [230., 253.],
        ),
    ] {
        view.body.as_mut().unwrap().yaw = yaw;
        surface.batch.begin();
        screens.draw(&mut surface, &view, [1024., 768.], 0.5);
        let beam = quad(&surface, 802);
        let geometry = [
            plane_corner(&surface, beam, 0),
            plane_corner(&surface, beam, 3),
        ];
        assert_eq!(cone.get_or_insert(geometry), &geometry);
        assert_eq!(&surface.batch.tint[beam * 16..beam * 16 + 4], &[1.; 4]);
        let direction = quad(&surface, 803);
        assert!((surface.batch.tint[direction * 16 + 3] - 0.23).abs() < 1e-5);
        for (texture, xy) in [(805, dot), (804, north)] {
            let q = quad(&surface, texture);
            let a = plane_corner(&surface, q, 0);
            let b = plane_corner(&surface, q, 3);
            for axis in 0..2 {
                assert!(
                    ((a[axis] + b[axis]) / 2. - xy[axis]).abs() < 0.02,
                    "texture {texture}"
                );
            }
        }
        let map = quad(&surface, 800);
        // Locate the east contact by its published coordinates in the textured quad.
        let uv = &surface.batch.uv[map * 8..map * 8 + 8];
        let corners = [0, 1, 2].map(|i| plane_corner(&surface, map, i));
        let du = [uv[4] - uv[0], uv[5] - uv[1]];
        let dv = [uv[2] - uv[0], uv[3] - uv[1]];
        let delta = [0.625 - uv[0], 0.5 - uv[1]];
        let det = du[0] * dv[1] - du[1] * dv[0];
        let u = (delta[0] * dv[1] - delta[1] * dv[0]) / det;
        let v = (du[0] * delta[1] - du[1] * delta[0]) / det;
        for axis in 0..2 {
            let pos = corners[0][axis]
                + u * (corners[2][axis] - corners[0][axis])
                + v * (corners[1][axis] - corners[0][axis]);
            assert!((pos - dot[axis]).abs() < 0.02, "map/contact registration");
        }
    }
    view.body.as_mut().unwrap().map = [0.01, 0.03].map(Sample::steady);
    view.body.as_mut().unwrap().yaw = Sample::steady(0.71);
    surface.batch.begin();
    screens.draw(&mut surface, &view, [1024., 768.], 0.);
    let map = quad(&surface, 800);
    for corner in 0..4 {
        let xy = plane_corner(&surface, map, corner);
        for (axis, center) in [230., 170.].iter().enumerate() {
            let i = map * 8 + corner * 2 + axis;
            assert!((0.0..=1.).contains(&surface.batch.uv[i]));
            let diameter = super::playing::MAP_ACROSS as f32;
            assert!((surface.batch.mask_uv[i] - (0.5 + (xy[axis] - center) / diameter)).abs() < 1e-4);
        }
    }
}

#[test]
fn long_captions_preserve_done_and_every_visible_score_row() {
    let long = "i".repeat(620);
    let title = Caption::Literal(&long);
    let rows: Vec<_> = (0..64)
        .map(|i| ScoreRow {
            player: i,
            team: if i % 2 == 0 { 7 } else { 12 },
            name: &long,
            score: 29,
            kills: 58,
            deaths: 46,
            assists: 0,
            ping: None,
        })
        .collect();
    let mut view = super::screens_tests::view();
    view.scores = &rows;
    view.map_name = title;
    for t in &mut view.teams {
        t.name = title;
    }
    for look in ["bf1", "bf2", "eternal"] {
        let mut screens = Screens::new(Catalog::default(), look);
        for size in [[640., 360.], [1280., 720.], [4096., 2160.]] {
            let mut surface = OwnedUiSurface::default();
            for font in &mut surface.fonts {
                for glyph in font.glyphs.values_mut() {
                    glyph.advance = 1.;
                }
            }
            surface.fonts[1]
                .glyphs
                .get_mut(&u16::from(b'6'))
                .unwrap()
                .page = 819;
            {
                let (phase, selection) = (Phase::Ended(Outcome::Draw), Hit::Done);
                view.phase = phase;
                surface.batch.begin();
                screens.draw(&mut surface, &view, size, 0.);
                // The action paints an accent stripe three plane units wide.
                let (x, y) = (764., 666.);
                let unit = (size[0] / 1024.).min(size[1] / 768.);
                let at = [
                    (x - 512.) * unit * 1024. / size[0],
                    (384. - y) * unit * 768. / size[1],
                ];
                assert!(
                    surface.batch.position[..surface.batch.count * 12]
                        .as_chunks::<12>()
                        .0
                        .iter()
                        .any(|q| (q[0] as f64 - at[0]).abs() < 0.01
                            && (q[1] as f64 - at[1]).abs() < 0.01
                            && ((q[6] - q[0]) as f64 - 3. * unit * 1024. / size[0]).abs() < 0.01),
                    "{look} {size:?} {selection:?}"
                );
                if selection == Hit::Done {
                    let shown: usize = surface
                        .batch
                        .draws
                        .iter()
                        .filter(|d| d.texture == 819)
                        .map(|d| d.count / 6)
                        .sum();
                    assert_eq!(
                        shown,
                        2 * screens.menu.board_rows * if look == "bf2" { 3 } else { 2 }
                    );
                }
                assert!(screens.omitted_quads > 0);
            }
        }
    }
}

#[test]
fn a_narrow_heading_stroke_covers_a_device_pixel() {
    let mut s = OwnedUiSurface::new(640, 360);
    let catalog = Catalog::default();
    s.fonts[0].height = 24.;
    let g = s.fonts[0].glyphs.get_mut(&u16::from(b'l')).unwrap();
    g.bounds = [0., 1., 1.1, 19.];
    g.page = 881;
    for look in ["eternal", "bf1", "bf2"] {
        s.batch.begin();
        let mut p =
            super::paint::Painter::new(&mut s, &catalog, Style::for_theme(look), [640., 360.]);
        p.text(
            [301.3, 17.],
            190.,
            17.,
            1,
            0,
            super::paint::WHITE,
            format_args!("lull"),
        );
        for d in s.batch.draws.iter().filter(|d| d.texture == 881) {
            for q in d.first / 6..(d.first + d.count) / 6 {
                let pos = &s.batch.position[q * 12..];
                assert!(
                    (pos[6] - pos[0]) * 640. / 1024. >= 0.999,
                    "{look}: subpixel stroke"
                );
            }
        }
    }
}

#[test]
fn the_cone_stays_inside_the_radar_and_contacts_keep_their_color() {
    let (mut screens, mut surface, base) = radar();
    let contacts = [Contact {
        subject: Subject::default(),
        team: 12,
        map: [Sample::steady(0.51); 2],
        icon: "contact",
    }];
    let view = BattleView {
        contacts: &contacts,
        ..base
    };
    screens.draw(&mut surface, &view, [1024., 768.], 0.);
    let contact = quad(&surface, 805);
    for mesh in [802, 803] {
        let draw = surface
            .batch
            .draws
            .iter()
            .find(|draw| draw.texture == mesh)
            .unwrap();
        assert_eq!(draw.mask, 801, "field of view must be clipped to the chart");
        assert!(
            draw.first / 6 < contact,
            "mesh must not darken a contact's team tint"
        );
    }
}

#[test]
fn projected_reticle_moves_with_aim_colors_by_relation_and_shows_hit_and_scope() {
    let mut catalog = Catalog::default();
    catalog.layout = Layout::read(
        br#"{"file":"1playerhud","entries":[
            {"name":"group","values":["player1reticule"],"children":[
                {"name":"position","values":[0.5,0.5,0.0,"Viewport"]},
                {"name":"Scale","values":[0.75,0.75,0.75]},
                {"name":"Model3D","values":["player1reticule_shape"],"children":[
                    {"name":"Mesh","values":["hud_main_reticule"]},
                    {"name":"Scale","values":[0.30,0.30,0.30]},
                    {"name":"alpha","values":[0.44]}
                ]},
                {"name":"Model3D","values":["player1reticule_outline"],"children":[
                    {"name":"Mesh","values":["hud_main_reticule_outline"]},
                    {"name":"Scale","values":[0.30,0.30,0.30]},
                    {"name":"alpha","values":[0.40]},
                    {"name":"color","values":[0.0,0.0,0.0]}
                ]},
                {"name":"Model3D","values":["player1reticule_hit"],"children":[
                    {"name":"Mesh","values":["hud_main_reticule_hit"]},
                    {"name":"Scale","values":[0.30,0.30,0.30]},
                    {"name":"alpha","values":[0.82]},
                    {"name":"color","values":[20.0,110.0,230.0]},
                    {"name":"FadeHoldTime","values":[0.25]}
                ]},
                {"name":"Model3D","values":["player1reticule_criticalhit"],"children":[
                    {"name":"Mesh","values":["hud_main_reticule_criticalhit"]},
                    {"name":"Scale","values":[0.30,0.30,0.30]},
                    {"name":"alpha","values":[0.88]},
                    {"name":"color","values":[20.0,110.0,230.0]},
                    {"name":"FadeHoldTime","values":[0.45]},
                    {"name":"FadeSustainTime","values":[0.35]},
                    {"name":"fadeouttime","values":[0.05]}
                ]},
                {"name":"BarSegmented","values":["clipring"],"children":[
                    {"name":"AngleStart","values":[-120.0]},
                    {"name":"AngleEnd","values":[120.0]},
                    {"name":"NumSegments","values":[4.0]},
                    {"name":"EventValue","values":["player1.weapon1.totalClipFraction"]},
                    {"name":"Rect","values":[80.0,80.0,"Center","Center"]},
                    {"name":"Segment","values":["Segment"],"children":[
                        {"name":"Bitmap","values":["clipseg"]},
                        {"name":"BitmapRect","values":[8.0,8.0,"Center","Center"]}
                    ]}
                ]}
            ]}
        ]}"#.as_slice(),
        br#"{"file":"hudtransforms","entries":[]}"#.as_slice(),
    ).unwrap();
    catalog.meshes.extend([
        ("hud_main_reticule".into(), "mesh.ret".into()),
        ("hud_main_reticule_outline".into(), "mesh.out".into()),
        ("hud_main_reticule_bullseye".into(), "mesh.dot".into()),
        ("hud_main_reticule_hit".into(), "mesh.hit".into()),
        ("hud_main_reticule_criticalhit".into(), "mesh.crit".into()),
    ]);
    for key in ["mesh.ret", "mesh.out", "mesh.hit", "mesh.crit"] {
        catalog.bounds.insert(key.into(), [-1.25, -1.25, 1.25, 1.25]);
    }
    let mut surface = OwnedUiSurface::default();
    surface.textures.extend([
        ("mesh.ret".into(), 901),
        ("mesh.out".into(), 902),
        ("mesh.dot".into(), 903),
        ("mesh.hit".into(), 904),
        ("mesh.crit".into(), 905),
        ("clipseg".into(), 906),
        ("optic_quad".into(), 907),
    ]);
    let mut screens = Screens::new(catalog, "bf2");
    let mut view = super::screens_tests::view();
    view.tick = 100;
    view.body = Some(Body {
        subject: Subject::default(),
        role: Role::Trooper,
        health: Gauge::default(),
        stamina: Gauge::default(),
        blocking: false,
        map: [Sample::steady(0.5); 2],
        yaw: Sample::default(),
        slots: [
            Some(Slot {
                kind: SlotKind::Weapon,
                name: Caption::Literal("Carbine"),
                icon: "",
                changed_at: Some(1),
                rounds: Some(12),
                reserve: Some(48),
                charge: None,
                ready_at: None,
                clip: Some(Gauge { value: Sample::steady(0.75), maximum: 1. }),
                heat: None,
                overheated: false,
                refire: None,
            }),
            None,
        ],
        reticle: "",
    });
    // Projected above center (y=0.30 -> plane y=230.4), enemy relation, normal hit.
    view.aim = Aim {
        position: Some([0.5, 0.30]),
        visible: true,
        relation: AimRelation::Enemy,
        hit: Some(HitMarker { tick: 95, critical: false }),
        scope: None,
    };
    screens.draw(&mut surface, &view, [1024., 768.], 0.);
    let ret_q = quad(&surface, 901);
    let ret_c0 = plane_corner(&surface, ret_q, 0);
    let ret_c3 = plane_corner(&surface, ret_q, 3);
    let ret_cy = (ret_c0[1] + ret_c3[1]) * 0.5;
    assert!((ret_cy - 0.30 * 768.).abs() < 0.5, "reticle y={ret_cy}");
    // A model is as large as its extent: 2.5 units at a scale of 0.75 x 0.30.
    let side = f64::from(ret_c3[0] - ret_c0[0]);
    let want = 2.5 * 0.75 * 0.30 * super::paint::MODEL_UNIT;
    assert!((side - want).abs() < 0.01, "reticle side {side}, want {want}");
    let enemy = super::playing::team_color(false).map(|c| c as f32);
    assert_eq!(&surface.batch.tint[ret_q * 16..ret_q * 16 + 3], &enemy[..3]);
    assert!((surface.batch.tint[ret_q * 16 + 3] - 0.44).abs() < 1e-4, "shape alpha comes from layout");
    let hit_q = quad(&surface, 904);
    assert!((surface.batch.tint[hit_q * 16 + 3] - 0.82).abs() < 1e-4, "hit alpha comes from layout");
    // Clip ring moves upward with the projected reticle.
    let seg_q = quad(&surface, 906);
    let seg_cy = (plane_corner(&surface, seg_q, 0)[1] + plane_corner(&surface, seg_q, 3)[1]) * 0.5;
    assert!(seg_cy < 300., "clip ring must orbit projected reticle, got y={seg_cy}");
    // Hit marker draws within its window, and the normalized bullseye mesh is never stretched across the reticle.
    assert!(surface.batch.draws.iter().any(|d| d.texture == 904));
    assert!(!surface.batch.draws.iter().any(|d| d.texture == 903));

    // Past the authored hit FadeHoldTime (0.25 s = 15 ticks), the hit marker disappears.
    view.tick = 115;
    surface.batch.begin();
    screens.draw(&mut surface, &view, [1024., 768.], 0.);
    assert!(!surface.batch.draws.iter().any(|d| d.texture == 904));

    // Sniper scope renders four 4:3 mirrored quadrants plus widescreen side strips and critical hit flash.
    view.tick = 100;
    view.aim.scope = Some("optic_quad");
    view.aim.hit = Some(HitMarker { tick: 60, critical: true }); // age = 40/60 = 0.667 s (inside 0.45 + 0.35 sustain)
    surface.batch.begin();
    screens.draw(&mut surface, &view, [1280., 720.], 0.);
    let scope_quads: usize = surface
        .batch
        .draws
        .iter()
        .filter(|d| d.texture == 907)
        .map(|d| d.count / 6)
        .sum();
    // 4 quadrants + 2 widescreen side strips = 6 quads.
    assert_eq!(scope_quads, 6);
    // The four mirrored quadrants span the 4:3 plane (512x384 each).
    let scope_first = surface
        .batch
        .draws
        .iter()
        .find(|d| d.texture == 907)
        .unwrap()
        .first
        / 6;
    let q0 = plane_corner(&surface, scope_first + 2, 0);
    let q3 = plane_corner(&surface, scope_first + 2, 3);
    let screen_w = (q3[0] - q0[0]).abs() * (1280. / 1024.);
    let screen_h = (q3[1] - q0[1]).abs() * (720. / 768.);
    assert!(
        ((screen_w / screen_h) - (4. / 3.)).abs() < 1e-3,
        "scope quadrant screen aspect {screen_w}x{screen_h}"
    );
    assert!(surface.batch.draws.iter().any(|d| d.texture == 905));
    assert!(!surface.batch.draws.iter().any(|d| d.texture == 901));
    assert!(!surface.batch.draws.iter().any(|d| d.texture == 903));
}

/// Every drawn quad as `[left, top, right, bottom]` in window pixels.
fn window_quads(surface: &OwnedUiSurface, size: [f64; 2]) -> Vec<[f64; 4]> {
    surface.batch.position[..surface.batch.count * 12]
        .chunks(12)
        .map(|q| {
            let xs = [q[0], q[3], q[6], q[9]].map(|x| (f64::from(x) + 512.) / 1024. * size[0]);
            let ys = [q[1], q[4], q[7], q[10]].map(|y| (384. - f64::from(y)) / 768. * size[1]);
            [
                xs.iter().copied().fold(f64::MAX, f64::min),
                ys.iter().copied().fold(f64::MAX, f64::min),
                xs.iter().copied().fold(f64::MIN, f64::max),
                ys.iter().copied().fold(f64::MIN, f64::max),
            ]
        })
        .collect()
}

#[test]
fn groups_keep_their_size_and_follow_their_screen_edge_at_any_aspect() {
    let mut catalog = Catalog::default();
    catalog.layout = Layout::read(
        br#"{"file":"1playerhud","entries":[
        {"name":"group","values":["port"],"children":[{"name":"position","values":[0.02,0.9,0,"Viewport"]},
            {"name":"BarBitmap","values":["a"],"children":[{"name":"BitmapRect","values":[100,10]},{"name":"EventValue","values":["player1.healthFraction"]}]}]},
        {"name":"group","values":["mast"],"children":[{"name":"position","values":[0.45,0.1,0,"Viewport"]},
            {"name":"BarBitmap","values":["b"],"children":[{"name":"BitmapRect","values":[100,10]},{"name":"EventValue","values":["player1.weapon1.target.healthFraction"]}]}]},
        {"name":"group","values":["starboard"],"children":[{"name":"position","values":[0.8,0.9,0,"Viewport"]},
            {"name":"BarBitmap","values":["c"],"children":[{"name":"BitmapRect","values":[100,10]},{"name":"EventValue","values":["player1.energyFraction"]}]}]}
        ]}"#
        .as_slice(),
        br#"{"file":"hudtransforms","entries":[]}"#.as_slice(),
    )
    .unwrap();
    let mut screens = Screens::new(catalog, "eternal");
    let mut view = super::screens_tests::view();
    let full = Gauge {
        value: Sample::steady(1.),
        maximum: 1.,
    };
    view.body = Some(Body {
        subject: Subject::default(),
        role: Role::Trooper,
        health: full,
        stamina: full,
        blocking: false,
        map: [Sample::steady(0.5); 2],
        yaw: Sample::default(),
        slots: [None; 2],
        reticle: "",
    });
    let target = Target {
        subject: Subject::default(),
        team: 12,
        name: Caption::Literal(""),
        health: Some(full),
        lock: Lock::None,
        progress: full,
        bracket: None,
    };
    view.target = Some(target);
    view.aim.visible = false;
    // Left, centre and right bars as [left, right] in window pixels and the distance of the
    // right bar from the window's right edge.
    let measure = |screens: &mut Screens, size: [f64; 2]| {
        let mut surface = OwnedUiSurface::default();
        screens.draw(&mut surface, &view, size, 0.);
        let quads = window_quads(&surface, size);
        let low: Vec<_> = quads.iter().filter(|q| q[1] > size[1] / 2.).collect();
        let high: Vec<_> = quads.iter().filter(|q| q[3] < size[1] / 2.).collect();
        let span = |set: &[&[f64; 4]]| {
            [
                set.iter().map(|q| q[0]).fold(f64::MAX, f64::min),
                set.iter().map(|q| q[2]).fold(f64::MIN, f64::max),
            ]
        };
        let (port, starboard): (Vec<_>, Vec<_>) =
            low.into_iter().partition(|q| q[2] < size[0] / 2.);
        let [a, b, c] = [span(&port), span(&high), span(&starboard)];
        (a, [b[0] - size[0] / 2., b[1] - size[0] / 2.], size[0] - c[1], c[1] - c[0])
    };
    let classic = measure(&mut screens, [1200., 900.]);
    for size in [[1440., 900.], [1600., 900.], [2100., 900.], [3200., 900.]] {
        let wide = measure(&mut screens, size);
        for (got, want) in [
            (wide.0[0], classic.0[0]),
            (wide.0[1], classic.0[1]),
            (wide.1[0], classic.1[0]),
            (wide.1[1], classic.1[1]),
            (wide.2, classic.2),
            (wide.3, classic.3),
        ] {
            assert!((got - want).abs() < 0.01, "{size:?}: {got} != {want}");
        }
    }
}

#[test]
fn space_assault_leaves_the_map_corner_to_its_critical_systems() {
    let (mut screens, mut surface, mut view) = radar();
    let mut seen = |view: &BattleView<'_>| {
        surface.batch.begin();
        screens.draw(&mut surface, view, [1024., 768.], 0.);
        surface.batch.draws.iter().any(|d| d.texture == 800)
    };
    assert!(seen(&view));
    view.mode = Mode::SpaceAssault;
    assert!(!seen(&view));
}

#[test]
fn a_bar_draws_its_backing_and_takes_the_team_colour_through_its_group_tint() {
    let mut catalog = Catalog::default();
    catalog.layout = Layout::read(
        br#"{"file":"1playerhud","entries":[{"name":"group","values":["gauge"],"children":[
            {"name":"position","values":[400,40]},{"name":"color","values":[128,255,51]},{"name":"alpha","values":[0.5]},
            {"name":"BarBitmap","values":["back"],"children":[{"name":"Bitmap","values":["slab"]},{"name":"BitmapRect","values":[120,12]},
                {"name":"color","values":[30,30,30]},{"name":"ZOrder","values":[90]}]},
            {"name":"BarBitmap","values":["fill"],"children":[{"name":"Bitmap","values":["slab"]},{"name":"BitmapRect","values":[100,6]},
                {"name":"position","values":[10,3]},{"name":"EventValue","values":["player1.weapon1.target.healthFraction"]}]},
            {"name":"Bitmap","values":["badge"],"children":[{"name":"Bitmap","values":["pin"]},{"name":"BitmapRect","values":[8,8]}]}
        ]}]}"#
        .as_slice(),
        br#"{"file":"hudtransforms","entries":[]}"#.as_slice(),
    )
    .unwrap();
    let mut screens = Screens::new(catalog, "eternal");
    let mut surface = OwnedUiSurface::default();
    surface.textures.extend([("slab".into(), 941), ("pin".into(), 942)]);
    let mut view = super::screens_tests::view();
    view.aim.visible = false;
    view.target = Some(Target {
        subject: Subject::default(),
        team: 12,
        name: Caption::Literal(""),
        health: Some(Gauge {
            value: Sample::steady(3.),
            maximum: 4.,
        }),
        lock: Lock::None,
        progress: Gauge::default(),
        bracket: None,
    });
    screens.draw(&mut surface, &view, [1024., 768.], 0.);
    let slabs: Vec<_> = surface.batch.draws.iter().filter(|d| d.texture == 941).collect();
    assert!(!surface.batch.draws.iter().any(|d| d.texture == 942));
    let quads: Vec<usize> = slabs
        .iter()
        .flat_map(|d| d.first / 6..(d.first + d.count) / 6)
        .collect();
    assert_eq!(quads.len(), 2);
    let width = |q: usize| surface.batch.position[q * 12 + 6] - surface.batch.position[q * 12];
    let tint = |q: usize| &surface.batch.tint[q * 16..q * 16 + 4];
    // The backing first, whole; then the fill, three quarters long.
    assert!((width(quads[0]) - 120.).abs() < 1e-3);
    assert!((width(quads[1]) - 75.).abs() < 1e-3);
    let backing = tint(quads[0]);
    assert!((backing[0] - 30. / 255. * 128. / 255.).abs() < 1e-3 && (backing[3] - 0.5).abs() < 1e-6);
    // The other team's colour, tinted by the group, at the group's alpha.
    let fill = tint(quads[1]);
    let enemy = super::playing::team_color(false).map(|c| c as f32);
    assert!((fill[0] - enemy[0] * 128. / 255.).abs() < 1e-3);
    assert!((fill[1] - enemy[1]).abs() < 1e-3);
    assert!((fill[2] - enemy[2] * 0.2).abs() < 1e-3);
    assert!((fill[3] - 0.5).abs() < 1e-6);
}

#[test]
fn hud_text_uses_its_own_font_at_the_games_size_and_counters_read_points_over_target() {
    let mut catalog = Catalog::default();
    catalog.layout = Layout::read(
        br#"{"file":"1playerhud","entries":[{"name":"group","values":["tally"],"children":[
            {"name":"position","values":[0.5,0.1,0,"Viewport"]},
            {"name":"text","values":["ours"],"children":[{"name":"position","values":[-150,0]},{"name":"TextFont","values":["gamefont_large"]},
                {"name":"TextAlignment","values":["Center","Top"]},{"name":"color","values":[20,40,60]},{"name":"EventText","values":["player1.team1.pointsText"]}]}
        ]}]}"#
        .as_slice(),
        br#"{"file":"hudtransforms","entries":[]}"#.as_slice(),
    )
    .unwrap();
    let mut screens = Screens::new(catalog, "eternal");
    let mut surface = OwnedUiSurface::default();
    for (ch, page) in [(b'1', 951), (b'/', 952), (b'8', 953)] {
        let glyph = surface.fonts[3].glyphs.get_mut(&u16::from(ch)).unwrap();
        glyph.page = page;
        glyph.advance = 7.;
    }
    let mut view = super::screens_tests::view();
    view.aim.visible = false;
    view.teams[0].score = 1;
    view.teams[0].target = Some(8);
    view.teams[1].show_score = false;
    screens.draw(&mut surface, &view, [1024., 768.], 0.);
    let x = |tex: u32| {
        let d = surface.batch.draws.iter().find(|d| d.texture == tex).unwrap();
        (surface.batch.position[d.first / 6 * 12], &surface.batch.tint[d.first / 6 * 16..d.first / 6 * 16 + 4])
    };
    let (one, tint) = x(951);
    let (slash, _) = x(952);
    let (eight, _) = x(953);
    // "1/8": no spaces, each advance at the game's size plus its spacing.
    let step = ((7. * super::paint::TEXT_SCALE + 0.5).floor() + super::paint::TEXT_SPACING) as f32;
    assert!((slash - one - step).abs() < 1e-3, "{one} {slash}");
    assert!((eight - slash - step).abs() < 1e-3);
    assert!((tint[0] - 20. / 255.).abs() < 1e-4 && (tint[2] - 60. / 255.).abs() < 1e-4);
}

#[test]
fn a_bar_takes_the_ramp_its_own_value_feeds() {
    let mut catalog = Catalog::default();
    catalog.layout = Layout::read(
        br#"{"file":"1playerhud","entries":[{"name":"group","values":["vigour"],"children":[
            {"name":"BarBitmap","values":["fill"],"children":[{"name":"BitmapRect","values":[50,4]},
                {"name":"EventValue","values":["player1.energyFraction"]},{"name":"EventColor","values":["player1.energyColor"]}]}]}]}"#
            .as_slice(),
        br#"{"file":"hudtransforms","entries":[
            {"name":"TransformNumberColor","values":["steady"],"children":[{"name":"EventInput","values":["player1.energyFraction"]},
                {"name":"EventOutput","values":["player1.energyColor"]},{"name":"NumberColor","values":[0,0,0,255]},{"name":"NumberColor","values":[1,0,255,0]}]},
            {"name":"TransformNumberColor","values":["spent"],"children":[{"name":"EventInput","values":["player1.energySpent"]},
                {"name":"EventOutput","values":["player1.energyColor"]},{"name":"NumberColor","values":[0,255,0,0]},{"name":"NumberColor","values":[1,90,90,90]}]}]}"#
            .as_slice(),
    )
    .unwrap();
    let mut screens = Screens::new(catalog, "eternal");
    let mut surface = OwnedUiSurface::default();
    let mut view = super::screens_tests::view();
    view.aim.visible = false;
    let full = Gauge {
        value: Sample::steady(1.),
        maximum: 1.,
    };
    view.body = Some(Body {
        subject: Subject::default(),
        role: Role::Trooper,
        health: full,
        stamina: full,
        blocking: false,
        map: [Sample::steady(0.5); 2],
        yaw: Sample::default(),
        slots: [None; 2],
        reticle: "",
    });
    screens.draw(&mut surface, &view, [1024., 768.], 0.);
    assert_eq!(surface.batch.count, 1);
    assert_eq!(&surface.batch.tint[..3], &[0., 1., 0.]);
}

#[test]
fn own_points_and_kills_show_when_they_change_and_fade_after_the_hold() {
    let mut catalog = Catalog::default();
    catalog.layout = Layout::read(
        br#"{"file":"1playerhud","entries":[{"name":"group","values":["player1stats"],"children":[
            {"name":"position","values":[30,300]},{"name":"FadeHoldTime","values":[2]},{"name":"fadeouttime","values":[0.5]},
            {"name":"Bitmap","values":["player1statskills.icon"],"children":[{"name":"Bitmap","values":["tally"]},{"name":"BitmapRect","values":[20,20]}]},
            {"name":"text","values":["kills"],"children":[{"name":"position","values":[30,0]},{"name":"EventNumber","values":["player1.statistic.kills"]}]},
            {"name":"text","values":["more"],"children":[{"name":"position","values":[60,0]},{"name":"EventText","values":["player1.statistic.killsDelta"]}]}
        ]}]}"#
        .as_slice(),
        br#"{"file":"hudtransforms","entries":[]}"#.as_slice(),
    )
    .unwrap();
    let mut screens = Screens::new(catalog, "eternal");
    let mut surface = OwnedUiSurface::default();
    surface.textures.insert("tally".into(), 961);
    for (ch, page) in [(b'3', 962), (b'+', 963), (b'4', 964)] {
        surface.fonts[1].glyphs.get_mut(&u16::from(ch)).unwrap().page = page;
    }
    let mut view = super::screens_tests::view();
    view.aim.visible = false;
    let mut row = ScoreRow {
        player: view.player,
        team: view.team,
        name: "Wren",
        score: 0,
        kills: 3,
        deaths: 0,
        assists: 0,
        ping: None,
    };
    let mut seen = |screens: &mut Screens, view: &BattleView<'_>| {
        surface.batch.begin();
        screens.draw(&mut surface, view, [1024., 768.], 0.);
        [961, 962, 963, 964].map(|t| surface.batch.draws.iter().any(|d| d.texture == t))
    };
    let rows = [row];
    view.scores = &rows;
    view.tick = 100;
    assert_eq!(seen(&mut screens, &view), [true, true, false, false]);
    // Past the hold and the fade the group is gone.
    view.tick = 100 + 151;
    assert_eq!(seen(&mut screens, &view), [false, false, false, false]);
    // A kill brings it back with the change.
    row.kills = 4;
    let rows = [row];
    view.scores = &rows;
    assert_eq!(seen(&mut screens, &view), [true, false, true, true]);
}

#[test]
fn command_posts_show_on_the_small_map_and_far_ones_rest_on_its_rim() {
    let (_, mut surface, mut view) = radar();
    let mut catalog = Catalog::default();
    catalog.layout = Layout::read(
        br#"{"file":"1playerhud","entries":[{"name":"map","values":["player1map"],"children":[
          {"name":"position","values":[230,170]},
          {"name":"BackdropSmall","values":["BackdropSmall"],"children":[{"name":"MASK","values":["disc"]},{"name":"texcoords","values":[0.12,0.14,0.62,0.64]}]}]},
          {"name":"Bitmap","values":["player1mapkey_CPIcon"],"children":[{"name":"Bitmap","values":["ring"]}]}]}"#
            .as_slice(),
        br#"{"file":"hudtransforms","entries":[]}"#.as_slice(),
    )
    .unwrap();
    let mut screens = Screens::new(catalog, "eternal");
    surface.textures.insert("ring".into(), 971);
    let post = |team, x| Contact {
        subject: Subject::default(),
        team,
        map: [Sample::steady(x), Sample::steady(0.5)],
        icon: super::playing::POST_CONTACT,
    };
    let points = [post(7, 0.6), post(12, 0.99), post(0, 0.5)];
    view.contacts = &points;
    screens.draw(&mut surface, &view, [1024., 768.], 0.);
    let marks: Vec<usize> = surface
        .batch
        .draws
        .iter()
        .filter(|d| d.texture == 971)
        .flat_map(|d| d.first / 6..(d.first + d.count) / 6)
        .collect();
    assert_eq!(marks.len(), 3);
    let r = 0.5 * super::playing::MAP_ACROSS;
    let centre = |q| {
        let a = plane_corner(&surface, q, 0);
        let b = plane_corner(&surface, q, 3);
        [f64::from(a[0] + b[0]) / 2., f64::from(a[1] + b[1]) / 2.]
    };
    // The window spans half the image: 0.6 lies 0.4 of the radius east, 0.99 beyond the rim.
    let [near, far] = [centre(marks[0]), centre(marks[1])];
    assert!((near[0] - (230. + 0.4 * r)).abs() < 0.01 && (near[1] - 170.).abs() < 0.01);
    assert!((far[0] - (230. + r)).abs() < 0.01 && (far[1] - 170.).abs() < 0.01);
    let tint = |q: usize| surface.batch.tint[q * 16..q * 16 + 3].to_vec();
    let own = super::playing::team_color(true).map(|c| c as f32);
    let other = super::playing::team_color(false).map(|c| c as f32);
    assert_eq!(tint(marks[0]), own[..3]);
    assert_eq!(tint(marks[1]), other[..3]);
    // Nobody holds the third.
    assert_eq!(tint(marks[2]), [1., 1., 1.]);
}

#[test]
fn message_lines_stack_from_the_top_of_the_list_oldest_first() {
    let mut catalog = Catalog::default();
    catalog.layout = Layout::read(
        br#"{"file":"1playerhud","entries":[{"name":"MultilineText","values":["notes"],"children":[
            {"name":"position","values":[40,60]},{"name":"Rect","values":[400,100]},{"name":"EventText","values":["player1.message"]},
            {"name":"format","values":["Format"],"children":[{"name":"TextBox","values":[0.5,0.3]},
                {"name":"TextAlignment","values":["Left","Center"]},{"name":"TextFont","values":["gamefont_small"]}]}]}]}"#
            .as_slice(),
        br#"{"file":"hudtransforms","entries":[]}"#.as_slice(),
    )
    .unwrap();
    let mut screens = Screens::new(catalog, "eternal");
    let mut surface = OwnedUiSurface::default();
    for (ch, page) in [(b'O', 981), (b'N', 982)] {
        surface.fonts[1].glyphs.get_mut(&u16::from(ch)).unwrap().page = page;
    }
    let kill = |sequence, killer| Event {
        sequence,
        until_tick: 400,
        audience: Audience::All,
        message: Message::Kill {
            killer,
            victim: "x",
            weapon: "",
            weapon_name: None,
        },
    };
    let events = [kill(1, "Oak"), kill(2, "Nettle")];
    let mut view = super::screens_tests::view();
    view.aim.visible = false;
    view.events = &events;
    screens.draw(&mut surface, &view, [1024., 768.], 0.);
    let top = |tex| {
        let d = surface.batch.draws.iter().find(|d| d.texture == tex).unwrap();
        plane_corner(&surface, d.first / 6, 0)[1].min(plane_corner(&surface, d.first / 6, 3)[1])
    };
    let line = (surface.fonts[1].height * super::paint::TEXT_SCALE) as f32;
    let (older, newer) = (top(981), top(982));
    assert!(older >= 60. && older < 60. + line, "{older}");
    assert!((newer - older - line).abs() < 1.01, "{older} {newer}");
}

#[test]
fn space_assault_group_puts_own_ship_right_marks_where_the_mission_says_and_colours_by_health() {
    let mut catalog = Catalog::default();
    catalog.layout = Layout::read(
        br#"{"file":"1playerhud","entries":[{"name":"group","values":["player1spaceassaultgroup"],"children":[
            {"name":"position","values":[700,20]},{"name":"Rect","values":[200,120]}]}]}"#
            .as_slice(),
        br#"{"file":"hudtransforms","entries":[]}"#.as_slice(),
    )
    .unwrap();
    let mut screens = Screens::new(catalog, "eternal");
    let mut surface = OwnedUiSurface::default();
    for (name, id) in [("hull_a", 991), ("hull_b", 992), ("ward_a", 993), ("ward_b", 994), ("gem", 995)] {
        surface.textures.insert(name.into(), id);
    }
    let gauge = |previous: f32, current: f32| Gauge {
        value: Sample { previous, current },
        maximum: 4.,
    };
    let system = |id, team, ship, ward, hud, hull, shield| System {
        subject: Subject { id, generation: 1 },
        team,
        name: Caption::Literal(""),
        icon: "gem",
        hull,
        shield,
        destroyed: false,
        hud,
        marker: true,
        ship,
        shield_art: ward,
    };
    let systems = [
        // The player's team (7): full health, half shield.
        system(1, 7, "hull_a", "ward_a", [0.25, 0.5], gauge(4., 4.), Some(gauge(2., 2.))),
        // The other team (12): a quarter of its health, its shield just hit.
        system(2, 12, "hull_b", "ward_b", [0.5, 0.75], gauge(1., 1.), Some(gauge(4., 3.))),
        System { destroyed: true, ..system(3, 12, "hull_b", "ward_b", [1., 0.], gauge(0., 0.), None) },
    ];
    let mut view = super::screens_tests::view();
    view.mode = Mode::SpaceAssault;
    view.aim.visible = false;
    view.systems = &systems;
    view.tick = 600;
    screens.draw(&mut surface, &view, [1024., 768.], 0.);
    let rect = |surface: &OwnedUiSurface, tex: u32, nth: usize| {
        let q: Vec<usize> = surface
            .batch
            .draws
            .iter()
            .filter(|d| d.texture == tex)
            .flat_map(|d| d.first / 6..(d.first + d.count) / 6)
            .collect();
        let q = q[nth];
        let a = plane_corner(surface, q, 0);
        let b = plane_corner(surface, q, 3);
        let tint = surface.batch.tint[q * 16..q * 16 + 4].to_vec();
        ([a[0].min(b[0]), a[1].min(b[1]), (a[0] - b[0]).abs(), (a[1] - b[1]).abs()], tint)
    };
    let near = |a: &[f32], b: &[f32]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3);
    // Own ship on the right half, the other's on the left, each the half's size, each in its
    // team's colour.
    let own_ship = rect(&surface, 991, 0);
    let other_ship = rect(&surface, 992, 0);
    assert!(near(&own_ship.0, &[800., 20., 100., 120.]));
    assert!(near(&other_ship.0, &[700., 20., 100., 120.]));
    let as_f32 = |c: bfe_render::interface::Color| c.map(|v| v as f32);
    assert!(near(&own_ship.1, &as_f32(super::playing::team_color(true))));
    assert!(near(&other_ship.1, &as_f32(super::playing::team_color(false))));
    // Shields over them: half left is half cyan; a fresh hit shows white.
    let (_, own_ward) = rect(&surface, 993, 0);
    assert!(near(&own_ward, &[0.5, 0.5 * 200. / 255., 0.5 * 150. / 255., 0.5]));
    let (_, hit_ward) = rect(&surface, 994, 0);
    assert!(near(&hit_ward[..3], &[1., 1., 1.]));
    // Marks at the mission's place in their team's half, 0.15 of the group in size.
    let (own_mark, own_tint) = rect(&surface, 995, 0);
    assert!(near(&own_mark, &[800. + 25. - 15., 20. + 60. - 9., 30., 18.]));
    assert!(near(&own_tint, &[32. / 255., 223. / 255., 32. / 255., 1.]));
    let (other_mark, other_tint) = rect(&surface, 995, 1);
    assert!(near(&other_mark, &[700. + 50. - 15., 20. + 90. - 9., 30., 18.]));
    // A quarter of its health: half way from red to yellow.
    assert!(near(&other_tint, &[223. / 255., 127.5 / 255., 32. / 255., 1.]));
    // Destroyed: black.
    assert!(near(&rect(&surface, 995, 2).1, &[0., 0., 0., 1.]));
    // A second later the hit's white has gone.
    view.tick = 661;
    let calm = [Some(gauge(3., 3.)), None];
    let systems2 = [systems[0], System { shield: calm[0], ..systems[1] }, systems[2]];
    view.systems = &systems2;
    surface.batch.begin();
    screens.draw(&mut surface, &view, [1024., 768.], 0.);
    let (_, ward) = rect(&surface, 994, 0);
    assert!(near(&ward, &[0.25, 0.75 * 200. / 255., 0.75 * 150. / 255., 0.75]));
}

#[test]
fn the_tooltip_strip_draws_its_box_background_in_nine_pieces_grown_by_the_border() {
    let mut catalog = Catalog::default();
    catalog.layout = Layout::read(
        br#"{"file":"1playerhud","entries":[{"name":"BorderedBox","values":["strip"],"children":[
            {"name":"background","values":["band"]},{"name":"Border","values":[4,5]},
            {"name":"Rect","values":[0.25,0.1,"Center","Center","Viewport"]},
            {"name":"position","values":[0.5,0.9,0,"Viewport"]},{"name":"color","values":[100,200,250]},
            {"name":"text","values":["tip"],"children":[
                {"name":"position","values":[10,8]},{"name":"TextBox","values":[0.2,0.05]},
                {"name":"EventText","values":["player1.tooltips"]}]}]}]}"#
            .as_slice(),
        br#"{"file":"hudtransforms","entries":[]}"#.as_slice(),
    )
    .unwrap();
    let mut screens = Screens::new(catalog, "eternal");
    let mut surface = OwnedUiSurface::default();
    surface.textures.insert("band".into(), 40);
    surface.assets[40] = bfe_render::interface::Asset {
        texture: 0,
        width: 80.,
        height: 50.,
        ready: true,
    };
    let view = BattleView {
        vehicle: Some(Vehicle {
            subject: Subject::default(),
            seat: Seat::Pilot,
            map_pose: None,
            hull: Gauge::default(),
            speed: Gauge::default(),
            boost: None,
            slots: [None; 2],
            landing: Some(Caption::Literal("Zone")),
        }),
        ..super::screens_tests::view()
    };
    screens.draw(&mut surface, &view, [1024., 768.], 1.);
    let pieces: Vec<usize> = surface
        .batch
        .draws
        .iter()
        .filter(|d| d.texture == 40)
        .flat_map(|d| d.first / 6..(d.first + d.count) / 6)
        .collect();
    assert_eq!(pieces.len(), 9);
    // The rect (256 x 76.8 centred on 512, 691.2) grown by 4 and 5 on each side; its edges
    // take 4 of the picture's 80 columns and 5 of its 50 rows.
    let xs = [380., 384., 640., 644.];
    let ys = [647.8, 652.8, 729.6, 734.6];
    let us = [0., 0.05, 0.95, 1.];
    let vs = [0., 0.1, 0.9, 1.];
    let near = |a: f32, b: f64| (f64::from(a) - b).abs() < 1e-3;
    for (n, &q) in pieces.iter().enumerate() {
        let (i, j) = (n % 3, n / 3);
        let (a, b) = (plane_corner(&surface, q, 0), plane_corner(&surface, q, 3));
        assert!(near(a[0], xs[i]) && near(a[1], ys[j]), "piece {n} at {a:?}");
        assert!(near(b[0], xs[i + 1]) && near(b[1], ys[j + 1]), "piece {n} to {b:?}");
        let uv = &surface.batch.uv[q * 8..q * 8 + 8];
        assert!(near(uv[0], us[i]) && near(uv[1], vs[j]), "piece {n} uv {uv:?}");
        assert!(near(uv[6], us[i + 1]) && near(uv[7], vs[j + 1]), "piece {n} uv {uv:?}");
        let tint = &surface.batch.tint[q * 16..q * 16 + 4];
        assert!(near(tint[0], 100. / 255.) && near(tint[1], 200. / 255.) && near(tint[2], 250. / 255.));
    }
    // Behind the hint's text.
    let text = surface.batch.draws.iter().position(|d| d.texture == 1).unwrap();
    let strip = surface.batch.draws.iter().position(|d| d.texture == 40).unwrap();
    assert!(strip < text);
}

#[test]
fn leaving_the_battlefield_shows_the_red_warning_at_the_centre_and_pulses_while_outside() {
    use super::playing::{LEAVING_KEY, LEAVING_REST, LEAVING_START, LEAVING_STIFFNESS, WARNING_RED};
    let mut catalog = Catalog::default();
    catalog.push_language([(LEAVING_KEY, "Go back")]);
    let mut screens = Screens::new(catalog, "eternal");
    let mut surface = OwnedUiSurface::default();
    let mut view = super::screens_tests::view();
    view.aim.visible = false;
    view.body = Some(Body {
        subject: Subject::default(),
        role: Role::Trooper,
        health: Gauge::default(),
        stamina: Gauge::default(),
        blocking: false,
        map: [Sample::steady(0.5); 2],
        yaw: Sample::default(),
        slots: [None; 2],
        reticle: "",
    });
    let glyphs = |surface: &OwnedUiSurface| -> Vec<usize> {
        surface
            .batch
            .draws
            .iter()
            .filter(|d| d.texture == 1)
            .flat_map(|d| d.first / 6..(d.first + d.count) / 6)
            .collect()
    };
    let draw = |screens: &mut Screens, surface: &mut OwnedUiSurface, view: &BattleView<'_>, f: f32| {
        surface.batch.begin();
        screens.draw(surface, view, [1024., 768.], f);
    };
    // Inside: nothing.
    view.tick = 600;
    draw(&mut screens, &mut surface, &view, 0.);
    assert!(glyphs(&surface).is_empty());
    // Outside: the text ("Go back", 7 glyphs) centred on the screen centre, in the warning red,
    // nearly opaque on its first frame.
    view.leaving = true;
    draw(&mut screens, &mut surface, &view, 0.);
    let quads = glyphs(&surface);
    assert_eq!(quads.len(), 7);
    let corners: Vec<[f32; 2]> = quads
        .iter()
        .flat_map(|&q| [plane_corner(&surface, q, 0), plane_corner(&surface, q, 3)])
        .collect();
    let (left, right) = corners.iter().fold((f32::MAX, f32::MIN), |(a, b), c| (a.min(c[0]), b.max(c[0])));
    let (top, bottom) = corners.iter().fold((f32::MAX, f32::MIN), |(a, b), c| (a.min(c[1]), b.max(c[1])));
    assert!(((left + right) * 0.5 - 512.).abs() < 2., "centred across: {left}..{right}");
    assert!(((top + bottom) * 0.5 - 384.).abs() < 2., "centred down: {top}..{bottom}");
    let tint = |surface: &OwnedUiSurface, q: usize| surface.batch.tint[q * 16..q * 16 + 4].to_vec();
    let first = tint(&surface, quads[0]);
    for (have, want) in first.iter().zip(WARNING_RED) {
        assert!((f64::from(*have) - want).abs() < 1e-3);
    }
    assert!((f64::from(first[3]) - LEAVING_START.trunc() / 255.).abs() < 1e-3);
    // Half a swing of the spring later it is at its floor, a whole swing later full again.
    let half = std::f64::consts::PI / LEAVING_STIFFNESS.sqrt() * 60.;
    let at = |ticks: f64| (600 + ticks.floor() as u32, (ticks - ticks.floor()) as f32);
    let (tick, f) = at(half);
    view.tick = tick;
    draw(&mut screens, &mut surface, &view, f);
    let low = tint(&surface, glyphs(&surface)[0])[3];
    let floor = (LEAVING_REST - (LEAVING_START - LEAVING_REST)).trunc().max(2.) / 255.;
    assert!((f64::from(low) - floor).abs() < 1e-2, "floor {low}");
    let (tick, f) = at(2. * half);
    view.tick = tick;
    draw(&mut screens, &mut surface, &view, f);
    assert!(tint(&surface, glyphs(&surface)[0])[3] > 0.95);
    // Back inside it goes, and the next stint starts from the top again.
    view.leaving = false;
    draw(&mut screens, &mut surface, &view, 0.);
    assert!(glyphs(&surface).is_empty());
    view.leaving = true;
    view.tick += 3;
    draw(&mut screens, &mut surface, &view, 0.);
    assert!(f64::from(tint(&surface, glyphs(&surface)[0])[3]) > 0.98);
    // Nothing without a body or a seat (waiting to spawn).
    view.body = None;
    draw(&mut screens, &mut surface, &view, 0.);
    assert!(glyphs(&surface).is_empty());
}

#[test]
fn a_message_wider_than_its_box_breaks_at_a_space_and_the_list_keeps_its_line_count() {
    let mut catalog = Catalog::default();
    catalog.layout = Layout::read(
        br#"{"file":"1playerhud","entries":[{"name":"MultilineText","values":["notes"],"children":[
            {"name":"position","values":[40,60]},{"name":"Rect","values":[400,300]},{"name":"NumLines","values":[3]},
            {"name":"EventText","values":["player1.message"]},
            {"name":"format","values":["Format"],"children":[{"name":"TextBox","values":[0.12,0.3]},
                {"name":"TextAlignment","values":["Left","Center"]},{"name":"TextFont","values":["gamefont_small"]}]}]}]}"#
            .as_slice(),
        br#"{"file":"hudtransforms","entries":[]}"#.as_slice(),
    )
    .unwrap();
    catalog.push_language([("game.infomsg.killed", "%s got %s")]);
    let mut screens = Screens::new(catalog, "eternal");
    let mut surface = OwnedUiSurface::default();
    for (ch, page) in [(b'O', 981), (b'N', 982), (b'W', 983), (b'P', 984)] {
        surface.fonts[1].glyphs.get_mut(&u16::from(ch)).unwrap().page = page;
    }
    let kill = |sequence, killer, victim| Event {
        sequence,
        until_tick: 400,
        audience: Audience::All,
        message: Message::Kill {
            killer,
            victim,
            weapon: "",
            weapon_name: None,
        },
    };
    // Each line is wider than the box (0.12 of the plane): it breaks after "got". Three rows:
    // the older message keeps only its first row, the newer takes two.
    let events = [kill(1, "Oak", "Wren"), kill(2, "Nettle", "Pine")];
    let mut view = super::screens_tests::view();
    view.mode = Mode::Other;
    view.aim.visible = false;
    view.events = &events;
    screens.draw(&mut surface, &view, [1024., 768.], 0.);
    let top = |surface: &OwnedUiSurface, tex| {
        surface.batch.draws.iter().find(|d| d.texture == tex).map(|d| {
            plane_corner(surface, d.first / 6, 0)[1].min(plane_corner(surface, d.first / 6, 3)[1])
        })
    };
    let line = (surface.fonts[1].height * super::paint::TEXT_SCALE) as f32;
    let row = |y: f32| ((y - 60.) / line).floor() as i32;
    assert_eq!(top(&surface, 981).map(row), Some(0), "Oak got");
    assert_eq!(top(&surface, 983), None, "Wren has no row left");
    assert_eq!(top(&surface, 982).map(row), Some(1), "Nettle got");
    assert_eq!(top(&surface, 984).map(row), Some(2), "Pine");
    // In a box wide enough, two names at the longest a player may choose are drawn whole on
    // one row (the line's glyphs, its two spaces included).
    let mut wide = Catalog::default();
    wide.layout = Layout::read(
        br#"{"file":"1playerhud","entries":[{"name":"MultilineText","values":["notes"],"children":[
            {"name":"position","values":[10,60]},{"name":"Rect","values":[1000,300]},{"name":"NumLines","values":[3]},
            {"name":"EventText","values":["player1.message"]},
            {"name":"format","values":["Format"],"children":[{"name":"TextBox","values":[0.98,0.3]},
                {"name":"TextFont","values":["gamefont_small"]}]}]}]}"#
            .as_slice(),
        br#"{"file":"hudtransforms","entries":[]}"#.as_slice(),
    )
    .unwrap();
    wide.push_language([("game.infomsg.killed", "%s got %s")]);
    let mut screens = Screens::new(wide, "eternal");
    let mut surface = OwnedUiSurface::default();
    let (killer, victim) = ("Q".repeat(32), "R".repeat(32));
    let events = [kill(3, &killer, &victim)];
    view.events = &events;
    screens.draw(&mut surface, &view, [1024., 768.], 0.);
    assert_eq!(screens.omitted_quads, 0);
    let glyphs = surface.batch.draws.iter().filter(|d| d.texture == 1).map(|d| d.count / 6).sum::<usize>();
    assert_eq!(glyphs, 32 + " got ".len() + 32);
}
