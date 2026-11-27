//! rotate's playlist position, fresh match token and admission deadline.
//! Scheduling is cold: keep the finished world visible until its delay expires.
use bfe_server::{
    config::{check_families, entry_has_content, names_swbf2, query_for_rotation_family, rotation_family, with_friendly_fire, ServerConfig},
    local_server::{random_match_hex, with_match_param},
};
use std::time::{Duration, Instant};

pub struct PendingMatch {
    pub config: ServerConfig,
    pub family: bfe_family::GameFamily,
    pub query: String,
    pub token: String,
    pub delay_ms: u64,
    at: Instant,
}

#[derive(Default)]
pub struct Rotation {
    index: usize,
    next: Option<PendingMatch>,
}

/// The entry from `first` on, in rotation order, that this server has prepared
/// content for, logging each one passed over. Only a rotation that names a
/// 2005 battle is searched: a 2004-only one starts at `first` as released, and
/// so does one with no entry that can start; the load reports what is missing.
fn present_from(config: &ServerConfig, first: usize) -> usize {
    let rotation = &config.rotation;
    if !names_swbf2(rotation) {
        return first;
    }
    let order = (0..rotation.len()).map(|step| (first + step) % rotation.len());
    let Some(index) = order.clone().find(|index| entry_has_content(&config.data, &rotation[*index], &config.ai)) else {
        return first;
    };
    for passed in order.take_while(|at| *at != index) {
        eprintln!("battle rotation: skipping {}: no prepared content for it", rotation[passed]);
    }
    index
}

/// Turns the rotation the service starts with to its first entry with content.
/// The saved configuration keeps its order.
pub fn skip_missing_first(config: &mut ServerConfig) {
    if !config.rotation.is_empty() {
        let first = present_from(config, 0);
        config.rotation.rotate_left(first);
    }
}

impl Rotation {
    pub fn pending(&self) -> bool {
        self.next.is_some()
    }

    /// A completed round advances; operator restart reloads the current entry.
    /// Match main.js ROTATION_DELAY_MS bounds and RESTART_DELAY_MS exactly.
    pub fn schedule(
        &mut self,
        config: &ServerConfig,
        advance: bool,
        now: Instant,
    ) -> Result<&PendingMatch, String> {
        if self.next.is_some() {
            return Err("Battle rotation already scheduled".into());
        }
        if config.rotation.is_empty() {
            return Err("No battle in the rotation".into());
        }
        let index = present_from(config, (self.index + usize::from(advance)) % config.rotation.len());
        let mut active = config.clone();
        active.rotation.rotate_left(index);
        let family = rotation_family(&active.rotation[0]);
        check_families(&active.rotation[..1], active.gate)?;
        let token = random_match_hex();
        let query = with_match_param(
            &with_friendly_fire(&query_for_rotation_family(family, &active.rotation[0], &active.ai)?, active.friendly_fire),
            &token,
        );
        let delay_ms = if advance {
            config
                .rotation_delay_ms
                .unwrap_or(10_000)
                .clamp(300, 60_000)
        } else {
            3_000
        };
        self.index = index;
        self.next = Some(PendingMatch {
            config: active,
            family,
            query,
            token,
            delay_ms,
            at: now + Duration::from_millis(delay_ms),
        });
        Ok(self.next.as_ref().unwrap())
    }

    /// Take a plan once. Listener teardown and admission run on the service
    /// thread, leaving the operator HTTP listener available during loading.
    pub fn take_ready(&mut self, now: Instant) -> Option<PendingMatch> {
        if self.next.as_ref().is_some_and(|next| now >= next.at) {
            self.next.take()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_round_announces_the_exact_next_match_before_admission() {
        let config = ServerConfig {
            rotation: vec![
                "swbf1/nab2:rep:conquest".into(),
                "swbf1/hot1:imp:conquest".into(),
            ],
            ..ServerConfig::default()
        };
        let now = Instant::now();
        let mut rotation = Rotation::default();
        let next = rotation.schedule(&config, true, now).unwrap();
        assert_eq!(next.delay_ms, 10_000);
        assert!(next
            .query
            .starts_with("?map=hot1&side=imp&ai=enhanced&seed=1&match="));
        assert_eq!(next.token.len(), 32);
        let announced = next.query.clone();
        assert!(rotation.schedule(&config, true, now).is_err());
        assert!(rotation
            .take_ready(now + Duration::from_millis(9_999))
            .is_none());
        let admitted = rotation.take_ready(now + Duration::from_secs(10)).unwrap();
        assert_eq!(admitted.query, announced);
        assert_eq!(admitted.family, bfe_family::GameFamily::Swbf1);
        assert_eq!(admitted.config.rotation[0], config.rotation[1]);
        assert!(rotation.take_ready(now + Duration::from_secs(11)).is_none());
        let next = rotation.schedule(&config, true, now).unwrap();
        assert!(next.query.starts_with("?map=nab2&side=rep"));
        assert_ne!(next.token, admitted.token);
    }

    // A rotation that names a 2005 battle passes over what this server has no
    // content for; a 2004-only one keeps the released order and load errors.
    #[test]
    fn only_a_rotation_that_names_a_2005_battle_passes_over_missing_content() {
        let dir = std::env::temp_dir().join(format!("bfe-rotation-skip-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("maps/nab2")).unwrap();
        std::fs::create_dir_all(dir.join("maps/hot1")).unwrap();
        std::fs::write(dir.join("maps.json"), br#"[{"id":"nab2"},{"id":"hot1"}]"#).unwrap();
        let config = |rotation: &[&str]| ServerConfig {
            data: dir.to_string_lossy().into_owned(),
            rotation: rotation.iter().map(|entry| entry.to_string()).collect(),
            ..ServerConfig::default()
        };
        let now = Instant::now();
        let ends = |rotation: &mut Rotation, config: &ServerConfig| {
            let next = rotation.schedule(config, true, now).unwrap().query.clone();
            rotation.take_ready(now + Duration::from_secs(60)).unwrap();
            next
        };

        let mut mixed = config(&["swbf2/kam9:gcw:hunt", "swbf1/nab2:rep:conquest", "swbf1/cor1:rep:conquest", "swbf1/hot1:imp:conquest"]);
        skip_missing_first(&mut mixed);
        assert_eq!(mixed.rotation[0], "swbf1/nab2:rep:conquest");
        // Past cor1 to hot1, then past kam9 to nab2 again.
        let mut rotation = Rotation::default();
        assert!(ends(&mut rotation, &mixed).starts_with("?map=hot1&side=imp"));
        assert!(ends(&mut rotation, &mixed).starts_with("?map=nab2&side=rep"));
        // Nothing that can start: the order stays and the load reports it.
        let mut none = config(&["swbf1/cor1:rep:conquest", "swbf2/kam9:gcw:hunt"]);
        skip_missing_first(&mut none);
        assert_eq!(none.rotation[0], "swbf1/cor1:rep:conquest");
        let mut rotation = Rotation { index: 1, next: None };
        assert!(ends(&mut rotation, &none).starts_with("?map=cor1&side=rep"));

        let mut released = config(&["swbf1/kam9:rep:conquest", "swbf1/nab2:rep:conquest"]);
        skip_missing_first(&mut released);
        assert_eq!(released.rotation[0], "swbf1/kam9:rep:conquest");
        let mut rotation = Rotation { index: 1, next: None };
        assert!(ends(&mut rotation, &released).starts_with("?map=kam9&side=rep"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restart_stays_on_the_current_entry_and_rotation_delay_is_bounded() {
        let mut config = ServerConfig::default();
        let now = Instant::now();
        let mut rotation = Rotation::default();
        let restart = rotation.schedule(&config, false, now).unwrap();
        assert_eq!(restart.delay_ms, 3_000);
        assert!(restart.query.starts_with("?map=nab2&side=rep"));
        rotation.take_ready(now + Duration::from_secs(3)).unwrap();
        config.rotation_delay_ms = Some(0);
        assert_eq!(rotation.schedule(&config, true, now).unwrap().delay_ms, 300);
        rotation
            .take_ready(now + Duration::from_millis(300))
            .unwrap();
        config.rotation_delay_ms = Some(u64::MAX);
        assert_eq!(
            rotation.schedule(&config, true, now).unwrap().delay_ms,
            60_000
        );
    }
}
