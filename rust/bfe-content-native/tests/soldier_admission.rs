use bfe_content::{actor::build_actor, clips::load_clips, json::parse};
use bfe_content_native::units::{descriptor, soldier};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::rc::Rc;

fn corpus() -> String {
    let mut output = String::new();
    for n in 0..32_usize {
        let mut bones = Vec::new();
        let mut names = Vec::new();
        for j in 0..n % 5 + 1 {
            let name = format!("joint-{}-{}", n % 3, j);
            bones.push(json!({"name": name, "parent": j as i32 - 1, "local": [1,0,0,0,0,1,0,0,0,0,1,0,0.25,j,0,1]}));
            names.push(name);
        }
        let actor = build_actor(
            &parse(&json!({"bones":bones,"parts":[]}).to_string()).unwrap(),
            &[],
        )
        .unwrap();
        let mut records = serde_json::Map::new();
        let mut samples = Vec::new();
        for j in 0..n % 7 {
            let action = format!("gesture-{n}-{j}");
            let frames = j % 3 + 2;
            let tracks = [j % names.len(), (j + n) % (names.len() + 3)];
            records.insert(action, json!({"frames":frames,"tracks":tracks,"offset":samples.len(),"flags":j%3,"travel":[j,0,0.375]}));
            samples.extend((0..frames * tracks.len() * 7).map(|i| (i as i32 - 13) as f32 / 16.));
        }
        // Exercise both sides of the released gait and alert lookups. Clip
        // identifiers follow the format; every track and sample is generated.
        for (i, name) in [
            "stand_idle_emote",
            "stand_walkforward",
            "stand_runforward",
            "stand_runforward_alert",
            "stand_idle_emote_alert",
            "crouch_walkforward",
            "stand_getdown_prone",
            "prone_getup_stand",
            "jump",
            "fall",
            "landhard",
        ]
        .iter()
        .enumerate()
        {
            if (i + n) % 4 == 0 {
                continue;
            }
            let suffix = if (n + i) % 3 == 0 { "_full" } else { "" };
            let action = format!("human_rifle_{name}{suffix}");
            let frames = 2 + (i + n) % 4;
            records.insert(action,json!({"frames":frames,"tracks":[0],"offset":samples.len(),"flags":0,"travel":[0,0,0]}));
            samples.extend((0..frames * 7).map(|v| (v as f32 + n as f32) / 17.));
        }
        let meta = json!({"fps":7+n,"bones":names,"clips":records});
        let bank = match n % 4 {
            1 => json!({"banks":false}),
            2 => {
                json!({"banks":{"copper":{"weapon":null},"ivory":{"loco":{"stand":{"walkforward":format!("gesture-{n}-0")}},"alert":{"runforward":format!("gesture-{n}-1")}}}})
            }
            3 => {
                json!({"banks":{"pewter":{"weapon":{"stand":{"shoot":format!("gesture-{n}-0")}}}}})
            }
            _ => json!(null),
        };
        let bank = parse(&bank.to_string()).unwrap();
        let clips = load_clips(
            &parse(&meta.to_string()).unwrap(),
            &Rc::from(samples),
            Some(&names),
        )
        .unwrap();
        let input = soldier(&actor, &clips, (n % 4 != 0).then_some(&bank));
        let bytes = serde_json::to_vec(&descriptor(&input)).unwrap();
        output.push_str(&format!("{n:02} {:x}\n", Sha256::digest(bytes)));
    }
    output
}

#[test]
fn admission_corpus() {
    assert_eq!(corpus(), include_str!("fixtures/soldier_admission.sha256"));
}
