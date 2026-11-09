//! The browser host dialog promises that human players replace bots.
use bfe_server::world_worker::NativeWorldHost;
use bfe_sim::{
    gen::{BlockHeader, F4RosterCommand, F4RosterResult, RoOperation},
    world::admit::{Call, Returned},
};
fn counts(host: &NativeWorldHost) -> F4RosterResult {
    let command = F4RosterCommand {
        op: RoOperation::Counts,
        ..Default::default()
    };
    let mut row = vec![0.; F4RosterCommand::WORDS];
    let mut heap = Vec::new();
    command.put(&mut row, &mut heap);
    let Returned::Row(words) = host
        .context
        .invoke_export(
            "f4_roster_command",
            &Call {
                row,
                heap,
                ..Default::default()
            },
        )
        .unwrap()
    else {
        panic!("missing roster result")
    };
    F4RosterResult::take(
        &words[BlockHeader::WORDS..],
        &words[words[BlockHeader::HEAP] as usize..],
    )
}
#[test]
fn hosted_humans_replace_bots_without_changing_the_team_population() {
    let Some(root) = std::env::var_os("BFE_CONTENT_DIR") else {
        return;
    };
    let mut host = NativeWorldHost::load_from_content(
        std::path::Path::new(&root),
        "?map=nab2&side=rep&ai=classic&seed=1",
        1,
    )
    .unwrap();
    assert!(host.world_admitted);
    let population = bfe_sim::battle::roster::MAX_TEAM_SIZE;
    let initial = counts(&host);
    assert_eq!(initial.counts.len(), 2);
    assert!(initial
        .counts
        .iter()
        .all(|team| team.total == Some(population) && team.humans == 0.));
    host.add_player("first", "PLAYER TWO", false, false, false)
        .unwrap();
    let joined = counts(&host);
    assert_eq!(
        joined.counts.iter().map(|team| team.humans).sum::<f64>(),
        1.
    );
    assert!(joined
        .counts
        .iter()
        .all(|team| team.total == Some(population)));
    host.remove_player("first");
    assert!(counts(&host)
        .counts
        .iter()
        .all(|team| team.total == Some(population) && team.humans == 0.));
}
