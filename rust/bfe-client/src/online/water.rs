//! WaterState's authored cells are admitted once for private replay and item
//! presentation. Only the shared simulation owner advances waves or crossings.
use bfe_content_native::BattleContent;
pub fn new(content: &BattleContent) -> Option<bfe_sim::units::water::WaterState> {
    content.map.get("water").filter(|v| !v.is_null()).map(|w| {
        let cell = w["cell"].as_f64().unwrap_or(f64::NAN);
        let grid = w["grid"].as_f64().unwrap_or(0.);
        let cells = w["cells"]
            .as_array()
            .map_or(vec![], |cells| cells.iter().filter_map(|c| Some(c[1].as_f64()? * grid + c[0].as_f64()?)).collect());
        bfe_sim::units::water::WaterState {
            height: w["height"].as_f64().unwrap_or(0.),
            cell,
            grid,
            offset: [w["offset"][0].as_f64().unwrap_or(0.), w["offset"][1].as_f64().unwrap_or(0.)],
            cells: cells.into_iter().map(|n| (n + 0.).to_bits()).collect(),
            bobbing: w["look"]["OscillationEnable"][0].as_f64().unwrap_or(1.) > 0.,
            simulation_time: 0.,
            physics_wave: 0.,
        }
    })
}
