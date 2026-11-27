//! Capture ownership and partial capture clocks for synthetic Conquest posts.
use bfe_sim::{battle::posts, gen::Post};
use std::fmt::Write;

#[test]
fn conquest_post_transitions_match_the_released_corpus() {
    let mut result = String::new();
    for owner in [0., 1., 2.] {
        for (allies, enemies) in [(0., 0.), (1., 0.), (0., 1.), (1., 1.), (0., 3.)] {
            let mut post = Post {
                active: true, team: owner, capture_time: 2., neutralize_time: 1.,
                ..Post::default()
            };
            writeln!(result, "owner={owner} present={allies},{enemies}").unwrap();
            for (dt, first, second) in [(0.25, allies, enemies), (0.75, allies, enemies),
                (0.5, enemies, allies), (1.5, enemies, allies)] {
                post.counts.set(1., first);
                post.counts.set(2., second);
                let change = posts::update_post(&mut post, dt);
                writeln!(result, "{change:?} {} {} {} {:016x} {:016x}", post.team,
                    post.contested, post.toward, post.neutralize.to_bits(), post.progress.to_bits()).unwrap();
            }
        }
    }
    assert_eq!(result, include_str!("fixtures/conquest-posts.txt"));
}
