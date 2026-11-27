//! The earlier build host dialog promises that human players replace bots.
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

// A 2004 join is the released one: it does not look at the publication bind's
// answer, so a refused bind does not refuse the player (a 2005 join does, and
// gives back what it took).
#[test]
fn a_2004_join_ignores_the_publication_binds_answer_as_released() {
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
    host.publication_session = 999_999;
    let joined = host.add_player("first", "PLAYER TWO", false, false, false);
    assert!(joined.is_ok(), "{joined:?}");
    assert_eq!(
        (host.mailbox_by_native.len(), host.player_by_native.len(), host.edges_by_native.len()),
        (1, 1, 1)
    );
}
