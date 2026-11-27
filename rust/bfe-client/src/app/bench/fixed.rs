//! Synchronous Instant Action playback for repeatable battle frames.
use crate::{
    generated_input::{ClientCommand, ClientFlightCommand, ClientLook},
    local::{launch::Prepared, options::Options, runtime::Runtime},
    online::replay::History,
    scene::Scene,
};
use bfe_content_native::setup::admission::{CLOCK_START_MS, CLOCK_UNIX_MS};
use bfe_presentation::network_layout::{net_kind as k, net_record as r};
use glow::HasContext;
use three::{Matrix4, Vector3};

pub(super) fn capture(
    opts: &super::super::cli::StandaloneOptions,
    map: &str,
    load: impl FnMut() -> glow::Context,
) -> Result<bool, String> {
    let root = opts
        .content_dir
        .as_deref()
        .ok_or("fixed battle needs content")?;
    let out = opts
        .shots_dir
        .as_deref()
        .ok_or("fixed battle needs output")?;
    let side = super::value("side").unwrap_or_else(|| "rep".into());
    let unit = super::value("unit");
    let desired = super::value("subject").unwrap_or_default();
    let tick = super::value("tick")
        .and_then(|t| t.parse::<u32>().ok())
        .unwrap_or(90)
        .max(2);
    let view_file = super::value("view").ok_or("fixed battle needs a camera file")?;
    let root_str = root.to_str().ok_or("invalid content path")?;
    let mut args: Vec<String> = [
        "--local", map, root_str, "--side", &side, "--seed", "7", "--ai", "classic", "--heroes",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    if let Some(name) = &unit {
        args.push("--unit".into());
        args.push(name.clone());
    }
    let options = Options::parse(&args, None)?.ok_or("invalid fixed battle options")?;
    let setup = Prepared::new(&options)?;
    let mut world = Runtime::new(
        *setup.engine,
        *setup.admission,
        CLOCK_START_MS,
        CLOCK_UNIX_MS,
    )?;
    let query = format!("?map={map}&side={side}&ai=classic&seed=7&heroes=on");
    let (mut scene, mut camera) = Scene::load_online(root, &query, opts.size, load)?;
    let online = scene.online.as_mut().ok_or("missing battle presentation")?;
    let replay = online
        .replay
        .take()
        .ok_or("missing battle prediction resources")?;
    let history = History::default();
    let mut subject = None;
    for t in 1..=tick {
        let command = ClientCommand {
            generation: world.receiver().own.generation,
            sequence: t as f64,
            ..Default::default()
        };
        world.step(
            &command,
            &ClientLook::default(),
            &ClientFlightCommand::default(),
        )?;
        let receiver = world.receiver_mut();
        receiver.clock.shown = receiver.latest;
        if t == tick.saturating_sub(24).max(1) {
            let online = scene.online.as_ref().unwrap();
            let mut census = std::collections::BTreeMap::new();
            let mut candidates = Vec::new();
            for row in receiver.active() {
                let kind = row[r::KIND] as u32;
                if kind > k::VEHICLE || row[r::DEAD] != 0. {
                    continue;
                }
                let name = if kind == k::VEHICLE {
                    "vehicle"
                } else {
                    online.class_name(row[r::CLASS_ID] as usize)
                };
                *census.entry(name.to_string()).or_insert(0usize) += 1;
                candidates.push((
                    row[r::ID] as u32,
                    name.to_string(),
                    std::array::from_fn::<_, 3, _>(|i| row[r::POSITION + i]),
                ));
            }
            candidates.sort_by_key(|(id, _, _)| *id);
            let picked = candidates
                .iter()
                .find(|(_, name, _)| !desired.is_empty() && name.contains(&desired))
                .or_else(|| candidates.first())
                .ok_or("fixed battle has no living subjects")?;
            let (_, name, p) = picked;
            let far = name == "vehicle";
            let dist = if far { 13. } else { 4.2 };
            let aim = Vector3::new(p[0], p[1] + if far { 1.8 } else { 0.9 }, p[2]);
            let eye = Vector3::new(aim.x - dist * 0.63, aim.y + dist * 0.27, aim.z + dist);
            let mut m = Matrix4::default();
            m.look_at(&eye, &aim, &Vector3::new(0., 1., 0.))
                .set_position_v3(&eye)
                .invert();
            let view = serde_json::json!({"position":[eye.x,eye.y,eye.z],"inverse":m.elements,"projection":camera.snapshot().values[..16].to_vec()});
            std::fs::write(&view_file, serde_json::to_vec(&view).unwrap())
                .map_err(|e| e.to_string())?;
            eprintln!("fixed battle {map} target {picked:?} census={census:?}");
            subject = Some(picked.0);
        }
        scene.present_network(receiver, &replay, &history, &mut camera, 1. / 60.);
        receiver.publications.clear();
    }
    if subject.is_none() {
        return Err("missing fixed camera subject".into());
    }
    let mut view = *camera.snapshot();
    super::apply_view(&mut view);
    scene.draw(&view)?;
    scene.canvas.show();
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    super::super::script::write_png(
        &out.join("world.png"),
        opts.size[0] as u32,
        opts.size[1] as u32,
        &scene.pixels()?,
    )?;
    let mut times = [0.; 48];
    for t in &mut times {
        let start = std::time::Instant::now();
        scene.draw(&view)?;
        unsafe {
            scene.bridge.r.core.gl.finish();
        }
        *t = start.elapsed().as_secs_f64() * 1000.;
    }
    times.sort_by(f64::total_cmp);
    eprintln!(
        "fixed battle {map} tick={tick} median_ms={} p95_ms={}",
        times[24], times[45]
    );
    Ok(true)
}
