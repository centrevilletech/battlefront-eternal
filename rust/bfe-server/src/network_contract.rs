//! Network contract metadata and validation (`src/network-contract.js`).
//!
//! The authoritative multiplayer simulation and the `BFE-UDP/1` transport share a versioned
//! contract (`NETWORK_CONTRACT` in `src/runtime/network-contract.js`, generated from
//! `tools/transport-schema.mjs` into `bfe_transport::layout::network_contract`):
//! - `manifestVersion: 1`
//! - `serverAdapterABI: 4` (implemented by this server adapter, not negotiated with clients)
//! - `protocol: 3` (`BFE-UDP/1` native UDP transport)
//! - `profile: "snapshot-delta-v1"` (binary `BNS1`/`BNO1` snapshot delta and `BNI1`/`BNA1` input wire profile)
//! - `simulation: "bfe-sim-<N>"` (matches `/^bfe-sim-[1-9][0-9]{0,5}$/`)
//! - `ownCorrectionSchema: 1..=65535` (checked against `OWN_PLAYER_SCHEMA` in `world-worker.js`)
//!
//! All contract values are sourced directly from `bfe_transport::layout::network_contract`
//! rather than retyped literals.

use bfe_transport::layout::network_contract as contract;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Full multiplayer engine network contract (`validateNetworkContract` return shape in `src/network-contract.js`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkContract {
    pub manifest_version: usize,
    pub protocol: usize,
    pub profile: String,
    pub simulation: String,
    #[serde(rename = "serverAdapterABI")]
    pub server_adapter_abi: usize,
    pub own_correction_schema: usize,
}

/// Subset of the network contract advertised on `/info`, LAN discovery, master listings, and the
/// `BFE-UDP/1` handshake (`export function wireCompatibility(contract)` in `src/network-contract.js`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireCompatibility {
    pub protocol: usize,
    pub profile: String,
    pub simulation: String,
}

impl Default for NetworkContract {
    fn default() -> Self {
        Self::current()
    }
}

impl NetworkContract {
    /// Returns the active compiled network contract (`loadNetworkContract` in `src/network-contract.js`).
    pub fn current() -> Self {
        Self {
            manifest_version: contract::MANIFEST_VERSION,
            protocol: contract::PROTOCOL,
            profile: contract::PROFILE.to_string(),
            simulation: contract::SIMULATION.to_string(),
            server_adapter_abi: contract::SERVER_ADAPTER_ABI,
            own_correction_schema: contract::OWN_CORRECTION_SCHEMA,
        }
    }

    /// Serializes the contract to JSON for `/healthz` (`networkContract` field in `src/main.js`).
    pub fn to_json(&self) -> Value {
        json!({
            "manifestVersion": self.manifest_version,
            "protocol": self.protocol,
            "profile": self.profile,
            "simulation": self.simulation,
            "serverAdapterABI": self.server_adapter_abi,
            "ownCorrectionSchema": self.own_correction_schema,
        })
    }
}

/// Validates a network contract JSON object (`export function validateNetworkContract(value)` in `src/network-contract.js`).
///
/// Enforces:
/// - `manifestVersion === 1` (`"Unsupported engine network manifest version"`)
/// - `serverAdapterABI === 4` (`"Unsupported engine server adapter ABI"`)
/// - `protocol === 3 && profile === "snapshot-delta-v1"` (`"Unsupported engine transport contract"`)
/// - `simulation` matches `/^bfe-sim-[1-9][0-9]{0,5}$/` (`"Invalid engine simulation revision"`)
/// - `ownCorrectionSchema` is an integer in `1..=65535` (`"Invalid engine own correction schema"`)
pub fn validate_network_contract(value: &Value) -> Result<NetworkContract, String> {
    let Some(obj) = value.as_object() else {
        return Err("Missing engine network contract".to_string());
    };
    if obj.get("manifestVersion").and_then(Value::as_u64) != Some(contract::MANIFEST_VERSION as u64)
    {
        return Err("Unsupported engine network manifest version".to_string());
    }
    if obj.get("serverAdapterABI").and_then(Value::as_u64)
        != Some(contract::SERVER_ADAPTER_ABI as u64)
    {
        return Err("Unsupported engine server adapter ABI".to_string());
    }
    if obj.get("protocol").and_then(Value::as_u64) != Some(contract::PROTOCOL as u64)
        || obj.get("profile").and_then(Value::as_str) != Some(contract::PROFILE)
    {
        return Err("Unsupported engine transport contract".to_string());
    }
    let sim = obj
        .get("simulation")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !valid_simulation_revision(sim) {
        return Err("Invalid engine simulation revision".to_string());
    }
    let schema = obj
        .get("ownCorrectionSchema")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if !(1..=65535).contains(&schema) {
        return Err("Invalid engine own correction schema".to_string());
    }
    Ok(NetworkContract {
        manifest_version: contract::MANIFEST_VERSION,
        protocol: contract::PROTOCOL,
        profile: contract::PROFILE.to_string(),
        simulation: sim.to_string(),
        server_adapter_abi: contract::SERVER_ADAPTER_ABI,
        own_correction_schema: schema as usize,
    })
}

/// Extracts `{ protocol, profile, simulation }` for wire-version admission checks
/// (`export function wireCompatibility(contract)` in `src/network-contract.js`).
pub fn wire_compatibility(contract: &NetworkContract) -> WireCompatibility {
    WireCompatibility {
        protocol: contract.protocol,
        profile: contract.profile.clone(),
        simulation: contract.simulation.clone(),
    }
}

/// Matches `/^bfe-sim-[1-9][0-9]{0,5}$/` from `validateNetworkContract` in `src/network-contract.js`.
fn valid_simulation_revision(sim: &str) -> bool {
    let Some(digits) = sim.strip_prefix("bfe-sim-") else {
        return false;
    };
    if digits.is_empty() || digits.len() > 6 {
        return false;
    }
    let mut bytes = digits.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    if !(b'1'..=b'9').contains(&first) {
        return false;
    }
    bytes.all(|b| b.is_ascii_digit())
}
