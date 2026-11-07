//! The shell controller runs without a device: whole command records cover
//! track order, stale phase endings, empty groups and interval changes.
use audio_control::{shell_music::Music, shell_music_layout::{shell_music_row as r,
    shell_music_op as op, shell_music_phase as phase}};
fn run(m: &mut Music, operation: u32, dt: f64) {
    m.row[r::ENABLED] = 1.0;
    m.row[r::OP] = operation as f64; m.row[r::DT] = dt; m.frame();
}
#[test]
fn music_cycles_across_groups_and_shortens_its_gap() {
    let mut m = Music::new(vec![vec![0, 1, 3], vec![2]]);
    m.row[r::INTERVAL] = 10.0;
    run(&mut m, op::SET, 0.0);
    assert_eq!(m.row[r::START], 1.0);
    run(&mut m, op::END, 0.0);
    run(&mut m, op::TICK, 9.99);
    assert_eq!(m.row[r::START], 0.0);
    m.row[r::INTERVAL] = 0.0;
    run(&mut m, op::TICK, 0.0);
    assert_eq!(m.row[r::START], 2.0);
    m.row[r::GROUP] = 1.0;
    run(&mut m, op::SET, 0.0);
    assert_eq!(m.row[r::STOP], 1.0);
    assert_eq!(m.row[r::START], 3.0);
    run(&mut m, op::HALT, 0.0);
    run(&mut m, op::TICK, 100.0);
    assert_eq!(m.row[r::PHASE], phase::STOPPED as f64);
    assert_eq!(m.row[r::START], 0.0);
}
#[test]
fn music_handles_missing_groups_and_long_pauses_without_catchup_starts() {
    let mut m = Music::new(vec![vec![0, 1]]);
    m.row[r::GROUP] = f64::INFINITY;
    run(&mut m, op::SET, 0.0);
    for _ in 0..100_000 { run(&mut m, op::TICK, 1e20); }
    assert_eq!(m.row[r::PHASE], phase::GAP as f64);
    assert_eq!(m.row[r::START], 0.0);
    m.row[r::GROUP] = 0.0;
    run(&mut m, op::SET, 0.0);
    run(&mut m, op::END, 0.0);
    run(&mut m, op::TICK, 1e20);
    assert_eq!(m.row[r::START], 2.0);
    assert_eq!(m.row[r::CYCLE], 1.0, "one start, rather than an unbounded catchup loop");
}
