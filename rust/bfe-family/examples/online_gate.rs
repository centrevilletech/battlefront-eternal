//! Prints the games the online gate plays. The release check builds it with
//! and without the developer switch (`build/ci/check-release-gate.sh`).

use bfe_family::{online_gate, GameFamily};

fn main() {
    let gate = online_gate();
    let plays = GameFamily::ALL.into_iter().filter(|family| gate.plays(*family));
    println!("{gate:?} plays {}", plays.map(GameFamily::as_str).collect::<Vec<_>>().join(" "));
}
