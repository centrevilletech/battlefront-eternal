//! Public synthetic oracle boundary; prepared assets are never required.
use std::io::{self, Read};
fn main() -> Result<(), String> {
    let mut text = String::new();
    io::stdin()
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let result = match v["kind"].as_str() {
        Some("foliage") => {
            let mut random = bfe_content_native::foliage::Seeded::new(37);
            let s = &v["sun"];
            let sun = three::Vector3::new(
                s[0].as_f64().unwrap(),
                s[1].as_f64().unwrap(),
                s[2].as_f64().unwrap(),
            );
            serde_json::json!(bfe_content_native::foliage::cards(
                &v["definition"],
                sun,
                &mut || random.next()
            )?)
        }
        Some("ocean") => {
            let s = bfe_content_native::water::ocean_geometry();
            serde_json::json!({"positions":s.vertices.chunks_exact(5).flat_map(|r|r[..3].iter().map(|v|*v as f64)).collect::<Vec<_>>(),"index":s.index})
        }
        Some("water") => {
            let s = bfe_content_native::water::surface(
                &v["data"],
                v["divisions"].as_u64().unwrap() as usize,
                v["rows"].as_u64().unwrap() as usize,
            )?;
            serde_json::json!({"vertices":s.vertices.iter().map(|v|*v as f64).collect::<Vec<_>>(),"index":s.index})
        }
        _ => return Err("unknown static fixture".into()),
    };
    println!("{result}");
    Ok(())
}
