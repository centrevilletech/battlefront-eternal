//! F2Sim.padPosts and foreignIn: bind authored post names once, then read
//! their current teams before each fleet span. A neutral post is present:
//! its zero team must override a pad's own team or its single class key.
use crate::abi::context::Context;
use crate::gen::{F2PadTeam, Post};
use crate::store::Ref;

#[derive(Default)]
pub(super) struct Owners {
    posts: Vec<Option<Ref<Post>>>,
}

impl Owners {
    /// Match CommandPosts.posts in its source order, as Array.find does.
    /// These links survive ownership changes; only the team is dynamic.
    pub(super) fn bind(context: &Context) -> Result<Self, String> {
        context
            .world(|w| Self {
                posts: crate::vehicles::module::f2::pad_post_names_in(&w.fleet)
                    .map(|name| {
                        let name = name?;
                        w.sim.store.command_posts.posts.iter().copied().find(|r| {
                            w.sim.store.post.get(*r).is_some_and(|p| {
                                String::from_utf16_lossy(w.sim.strings.units(p.name)).to_lowercase()
                                    == name
                            })
                        })
                    })
                    .collect(),
            })
            .map_err(|e| e.message().to_string())
    }

    pub(super) fn append(&self, context: &Context, out: &mut Vec<f64>) -> Result<(), String> {
        context
            .world(|w| {
                for post in &self.posts {
                    let team = post.and_then(|r| w.sim.store.post.get(r)).map(|p| p.team);
                    let pad = F2PadTeam {
                        has_team: team.is_some(),
                        team: team.unwrap_or(0.0),
                    };
                    let at = out.len();
                    out.resize(at + F2PadTeam::WORDS, 0.0);
                    pad.put(&mut out[at..], &mut Vec::new());
                }
            })
            .map_err(|e| e.message().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_links_keep_neutral_ownership_and_follow_capture() {
        let context = Context::new();
        let post = context
            .owned(|w| {
                let name = w.sim.strings.intern_str("Hangar");
                let post = w.sim.store.post.insert(Post {
                    name,
                    team: 0.0,
                    ..Default::default()
                });
                w.sim.store.command_posts.posts.push(post);
                let json = w
                    .sim
                    .strings
                    .intern_str(r#"[{"post":"hangar"},{"post":"missing"},{}]"#);
                crate::vehicles::module::f2::f2_init_in(w, json.0, 0.0, 0.0);
                post
            })
            .unwrap();
        let owners = Owners::bind(&context).unwrap();
        let mut words = Vec::new();
        owners.append(&context, &mut words).unwrap();
        let first = F2PadTeam::take(&words, &[]);
        assert!(first.has_team);
        assert_eq!(first.team, 0.0);
        assert!(!F2PadTeam::take(&words[F2PadTeam::WORDS..], &[]).has_team);
        assert!(!F2PadTeam::take(&words[2 * F2PadTeam::WORDS..], &[]).has_team);
        context
            .owned(|w| w.sim.store.post.get_mut(post).unwrap().team = 2.0)
            .unwrap();
        words.clear();
        owners.append(&context, &mut words).unwrap();
        assert_eq!(F2PadTeam::take(&words, &[]).team, 2.0);
    }
}
