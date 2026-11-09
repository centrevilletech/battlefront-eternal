//! main.js rotate's playlist position, fresh match token and admission deadline.
//! Scheduling is cold: keep the finished world visible until its delay expires.
use bfe_server::{
    config::{query_for_rotation, ServerConfig},
    local_server::{random_match_hex, with_match_param},
};
use std::time::{Duration, Instant};

pub struct PendingMatch {
    pub config: ServerConfig,
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
        let index = (self.index + usize::from(advance)) % config.rotation.len();
        let mut active = config.clone();
        active.rotation.rotate_left(index);
        let token = random_match_hex();
        let query = with_match_param(
            &query_for_rotation(&active.rotation[0], &active.ai)?,
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
            .starts_with("?map=hot1&side=imp&ai=classic&seed=1&match="));
        assert_eq!(next.token.len(), 32);
        let announced = next.query.clone();
        assert!(rotation.schedule(&config, true, now).is_err());
        assert!(rotation
            .take_ready(now + Duration::from_millis(9_999))
            .is_none());
        let admitted = rotation.take_ready(now + Duration::from_secs(10)).unwrap();
        assert_eq!(admitted.query, announced);
        assert_eq!(admitted.config.rotation[0], config.rotation[1]);
        assert!(rotation.take_ready(now + Duration::from_secs(11)).is_none());
        let next = rotation.schedule(&config, true, now).unwrap();
        assert!(next.query.starts_with("?map=nab2&side=rep"));
        assert_ne!(next.token, admitted.token);
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
