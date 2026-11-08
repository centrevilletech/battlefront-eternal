//! GPU feature status filtering, device summary normalization, and discrete GPU
//! switch selection.
//!
//! Mirrors `electron/gpu.js` (`gpuSwitches`, `FEATURES`, `gpuReport`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// GPU feature keys reported to the VIDEO OPTIONS diagnostics view.
///
/// Mirrors `FEATURES` in `electron/gpu.js`.
pub const FEATURES: &[&str] = &["webgl", "webgl2", "gpu_compositing", "rasterization"];

/// Returns the GPU command-line switches for `platform`.
///
/// On Windows laptops with both an integrated and a discrete GPU, requests the
/// high-performance discrete adapter (`force_high_performance_gpu`). On Linux,
/// the system's own PRIME offload environment variables govern adapter choice.
///
/// Mirrors `gpuSwitches` in `electron/gpu.js`.
pub fn gpu_switches(platform: &str) -> Vec<Vec<String>> {
    if platform == "win32" {
        vec![vec!["force_high_performance_gpu".to_string()]]
    } else {
        Vec::new()
    }
}

/// Single GPU adapter entry inside [`GpuReport`].
///
/// Mirrors the normalized `devices` items built in `gpuReport` in `electron/gpu.js`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuDevice {
    /// PCI vendor ID (`0` when unknown).
    pub vendor_id: u32,
    /// PCI device ID (`0` when unknown).
    pub device_id: u32,
    /// Whether this adapter is currently active.
    pub active: bool,
    /// Driver version string truncated to at most 64 characters.
    pub driver_version: Option<String>,
}

/// Complete GPU diagnostics report for the VIDEO OPTIONS screen.
///
/// Mirrors the return object of `gpuReport` in `electron/gpu.js`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuReport {
    /// Filtered feature status map containing only keys in [`FEATURES`].
    pub status: BTreeMap<String, String>,
    /// Up to 4 normalized GPU device entries.
    pub devices: Vec<GpuDevice>,
    /// Flat list of active GPU switch names for `platform`.
    pub switches: Vec<String>,
}

/// Builds a normalized [`GpuReport`] from raw feature statuses and adapter info.
///
/// Retains only [`FEATURES`] keys in `status`, keeps at most 4 devices, and
/// truncates `driver_version` strings to 64 characters.
///
/// Mirrors `gpuReport` in `electron/gpu.js`.
pub fn gpu_report(
    raw_status: &BTreeMap<String, String>,
    raw_devices: &[GpuDevice],
    platform: &str,
) -> GpuReport {
    // Filter feature status map down to the four frame-rate-critical keys.
    let mut status = BTreeMap::new();
    for &feature in FEATURES {
        if let Some(val) = raw_status.get(feature) {
            status.insert(feature.to_string(), val.clone());
        }
    }

    // Keep at most 4 devices and clamp driverVersion to 64 characters.
    let devices = raw_devices
        .iter()
        .take(4)
        .map(|d| GpuDevice {
            vendor_id: d.vendor_id,
            device_id: d.device_id,
            active: d.active,
            driver_version: d
                .driver_version
                .as_ref()
                .map(|s| s.chars().take(64).collect()),
        })
        .collect();

    // Flatten the switch tuples to their leading switch names.
    let switches = gpu_switches(platform)
        .into_iter()
        .filter_map(|mut pair| pair.drain(..1).next())
        .collect();

    GpuReport {
        status,
        devices,
        switches,
    }
}
