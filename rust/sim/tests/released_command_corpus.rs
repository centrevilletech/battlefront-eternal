//! The released controller's command functions on odd inputs: signed zero,
//! infinities, NaN, every tenth from -20 to 40 with both neighbouring
//! doubles, and 100,000 seeded bit patterns. The expected values were
//! captured by running this file against the released build; the goal bots
//! must leave these functions exactly as released.

use bfe_wide::Wide;
use bfe_sim::ai::control::controller::{goal, goal_state, mobile_goal_of_state, policy_goal_state, CommandQueue, SourceGoal};
use bfe_sim::ai::control::world::Policy;

fn corpus() -> Vec<f64> {
    let mut values = vec![f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -0.0, f64::MIN_POSITIVE];
    for i in -200..401 {
        let x = f64::from(i) / 10.0;
        values.extend([x, x.next_up(), x.next_down()]);
    }
    let mut seed = 17u64;
    for _ in 0..100_000 {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        values.push(f64::from_bits(seed));
    }
    values
}

/// Every output for one input, as bits.
fn outputs(v: f64) -> [Wide; 8] {
    let mut queue = CommandQueue::new();
    queue.current = SourceGoal { post: Some(53), ..SourceGoal::new(v) };
    let copy = queue.current_copy();
    [
        Wide::from_f64_bits(copy.kind),
        copy.post.map_or(Wide::MAX, |p| Wide::from_u32(p as u32)),
        Wide::from_f64_bits(goal_state(v)),
        mobile_goal_of_state(v).map_or(Wide::MAX, Wide::from_f64_bits),
        Wide::from_f64_bits(policy_goal_state(Policy::None, v)),
        Wide::from_f64_bits(policy_goal_state(Policy::Stationary, v)),
        Wide::from_f64_bits(policy_goal_state(Policy::Fly, v)),
        Wide::from_f64_bits(policy_goal_state(Policy::Mobile, v)),
    ]
}

fn digest(values: &[f64]) -> Wide {
    let mut h = Wide::new(0xcbf2_9ce4, 0x8422_2325);
    for &v in values {
        for x in outputs(v) {
            h = h.xor(x).wrapping_mul(Wide::new(0x0100, 0x0000_01b3));
        }
    }
    h
}

#[test]
fn released_command_functions_keep_their_outputs() {
    let values = corpus();
    assert_eq!(values.len(), 101_809);
    let rows: Vec<(f64, [f64; 7])> = [0.0, -0.0, 1.0, 3.0, 4.0, 8.0, 9.0, 10.0, 11.0, 10.000000000000002]
        .iter()
        .map(|&v| {
            let o = outputs(v);
            let f = |bits: Wide| if bits == Wide::MAX { -1.0 } else { bits.to_f64_bits() };
            (v, [f(o[0]), f(o[2]), f(o[3]), f(o[4]), f(o[5]), f(o[6]), f(o[7])])
        })
        .collect();
    println!("released-command-corpus {} {:016x} {:?}", values.len(), digest(&values), rows);
    assert_eq!(goal::HOLD, 10.0);
    assert_eq!(digest(&values), EXPECTED_DIGEST, "outputs captured from the released build");
    assert_eq!(rows, expected_rows());
}

/// Captured from the released build.
const EXPECTED_DIGEST: Wide = Wide::new(0xd493_70a9, 0xdedc_4426);

/// Input, then: the copied kind, `goal_state`, `mobile_goal_of_state` (-1
/// for none) and `policy_goal_state` for no policy, stationary, flying and
/// mobile seats. Captured from the released build.
fn expected_rows() -> Vec<(f64, [f64; 7])> {
    vec![
        (0.0, [0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0]),
        (-0.0, [-0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0]),
        (1.0, [1.0, 1.0, 1.0, 1.0, 1.0, 0.0, 1.0]),
        (3.0, [3.0, 3.0, 3.0, 3.0, 1.0, 7.0, 3.0]),
        (4.0, [4.0, 4.0, 4.0, 4.0, 0.0, 4.0, 4.0]),
        (8.0, [8.0, 9.0, -1.0, 9.0, 0.0, 0.0, 9.0]),
        (9.0, [9.0, 10.0, 8.0, 10.0, 0.0, 0.0, 10.0]),
        (10.0, [10.0, 11.0, 9.0, 11.0, 0.0, 11.0, 11.0]),
        (11.0, [0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 0.0]),
        (10.000000000000002, [0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0]),
    ]
}
