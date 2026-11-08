//! Health endpoint client for `GET /healthz` (`server` handler in `src/main.js`).
//!
//! In `src/main.js`, the operator HTTP listener exposes `GET /healthz` without bearer
//! authentication so process supervisors and readiness probes can verify that:
//! 1. `service == "bfeserver"` and `status == "running"`
//! 2. `multiplayerReady == true` (meaning the UDP game port is bound AND the authoritative
//!    world has been admitted through `Context::world_admit`)
//! 3. `tick` is an integer simulation tick that strictly advances across consecutive samples.

use std::io::Read;
use std::time::Duration;

use serde_json::Value;
use ureq::config::Config;
use ureq::Agent;

/// Polls `url` (`http://127.0.0.1:18450/healthz`) `samples` times separated by `interval` and
/// verifies that `status == "running"`, `multiplayerReady == true`, and `tick` advances
/// (`GET /healthz` contract in `src/main.js`).
pub fn check_health_endpoint(url: &str, samples: usize, interval: Duration) -> Result<Value, String> {
    let config = Config::builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(5)))
        .build();
    let agent = Agent::new_with_config(config);
    let mut previous: Option<i64> = None;

    for sample in 0..samples {
        let mut response = agent
            .get(url)
            .call()
            .map_err(|e| format!("health HTTP request failed: {e}"))?;
        if response.status().as_u16() >= 400 {
            return Err("health HTTP response".to_string());
        }
        let mut raw = String::new();
        response
            .body_mut()
            .with_config()
            .limit(65536)
            .reader()
            .read_to_string(&mut raw)
            .map_err(|e| e.to_string())?;
        let value: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        // Verify `status === "running"` (`src/main.js`).
        if value.get("status").and_then(|v| v.as_str()) != Some("running") {
            let err = value
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("world not running");
            return Err(err.to_string());
        }
        // Verify `multiplayerReady === true`. A server whose world was not admitted via
        // `Context::world_admit` reports `multiplayerReady: false` and must fail readiness checks.
        if value.get("multiplayerReady").and_then(|v| v.as_bool()) != Some(true) {
            return Err("multiplayer listener ready".to_string());
        }
        // Verify `tick` is an integer and advances on each subsequent sample.
        let tick = value
            .get("tick")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| "integer simulation tick".to_string())?;
        if let Some(prev) = previous {
            if tick <= prev {
                return Err("simulation tick must advance".to_string());
            }
        }
        previous = Some(tick);
        if sample + 1 < samples && !interval.is_zero() {
            std::thread::sleep(interval);
        }
    }

    Ok(serde_json::json!({
        "ok": true,
        "tick": previous.unwrap_or(0),
        "advancing": true,
    }))
}
