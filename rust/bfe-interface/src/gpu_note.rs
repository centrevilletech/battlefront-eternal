//! The note under the QUALITY PRESET row: the GPU the client draws with, and a warning when it is a
//! software renderer (`gpuLabel`, `gpuWarning` and `gpuNote` in `src/gpu-info.js`).
//!
//! The browser reads Chromium's own GPU report as well (compositing in software, a second idle GPU);
//! a native window has only the GL renderer string, so only the software-renderer warning applies.

/// Renderer names that are a CPU rasteriser (`SOFTWARE` in `src/gpu-info.js`), tried in this order.
const SOFTWARE: [&str; 8] = [
    "SwiftShader",
    "llvmpipe",
    "softpipe",
    "lavapipe",
    "Software Rasterizer",
    "Basic Render Driver",
    "Microsoft Basic",
    "GDI Generic",
];

/// The leftmost software-renderer name in `name`, as spelled there (the regex alternation's match).
fn software_match(name: &str) -> Option<&str> {
    let lower = name.to_ascii_lowercase();
    SOFTWARE
        .iter()
        .filter_map(|word| lower.find(&word.to_ascii_lowercase()).map(|at| (at, word.len())))
        .min_by_key(|(at, _)| *at)
        .map(|(at, len)| &name[at..at + len])
}

/// The device name in a renderer string, without the driver's decoration (`gpuLabel`).
fn label(renderer: &str) -> String {
    let name = renderer.trim();
    if let Some(found) = software_match(name) {
        return format!("Software ({found})");
    }
    let mut name = name.to_string();
    if let Some(inner) = name.strip_prefix("ANGLE (").and_then(|rest| rest.strip_suffix(')')) {
        // Vendor, device (and backend), driver: split on the commas outside parentheses.
        let mut parts = Vec::new();
        let (mut depth, mut start) = (0i32, 0usize);
        for (i, c) in inner.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                ',' if depth == 0 => {
                    parts.push(inner[start..i].trim());
                    start = i + 1;
                }
                _ => {}
            }
        }
        parts.push(inner[start..].trim());
        name = if parts.len() > 1 { parts[1] } else { parts[0] }.to_string();
    }
    let name = strip_hex_ids(&name);
    let name = strip_shader_models(&name);
    let name = strip_backend_suffix(&name);
    let name = name.trim();
    if name.is_empty() { "Unknown GPU".into() } else { name.into() }
}

/// Removes every ` (0x1234)` group.
fn strip_hex_ids(name: &str) -> String {
    let mut out = String::new();
    let mut rest = name;
    while let Some(at) = rest.find("(0x") {
        match rest[at..].find(')') {
            Some(close) if rest[at + 3..at + close].chars().all(|c| c.is_ascii_hexdigit()) => {
                out.push_str(rest[..at].trim_end());
                rest = &rest[at + close + 1..];
            }
            _ => {
                out.push_str(&rest[..at + 3]);
                rest = &rest[at + 3..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Removes a trailing ` vs_4_1 ps_4_1`.
fn strip_shader_models(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    let n = words.len();
    if n >= 2 && words[n - 2].to_ascii_lowercase().starts_with("vs_") && words[n - 1].to_ascii_lowercase().starts_with("ps_") {
        return words[..n - 2].join(" ");
    }
    name.to_string()
}

/// Removes a trailing ` Direct3D11`, ` OpenGL ...`, ` Vulkan ...` or ` Metal ...`.
fn strip_backend_suffix(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    let mut cut = name.len();
    for (at, _) in name.match_indices(' ') {
        let tail = &lower[at + 1..];
        let api = tail.starts_with("direct3d")
            || ["opengl", "vulkan", "metal"].iter().any(|api| {
                tail.strip_prefix(api)
                    .is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_'))
            });
        if api {
            cut = at;
            break;
        }
    }
    name[..cut].to_string()
}

/// The warning to show beside the name, if any (`gpuWarning`).
fn warning(renderer: &str) -> Option<&'static str> {
    software_match(renderer).map(|_| "DRAWN IN SOFTWARE: UPDATE THE GRAPHICS DRIVER")
}

/// The QUALITY PRESET row's note for `renderer`, the GL renderer string (`gpuNote`).
///
/// "GPU: " and the name's first words up to 26 letters, then the warning when there is one. `None`
/// when the renderer is unknown and there is nothing to warn about.
pub fn gpu_note(renderer: &str) -> Option<String> {
    let warning = warning(renderer);
    if renderer.is_empty() && warning.is_none() {
        return None;
    }
    let upper = label(renderer).to_uppercase();
    let cleaned: String = upper
        .chars()
        .map(|c| if c.is_ascii_uppercase() || c.is_ascii_digit() || " ()+.-".contains(c) { c } else { ' ' })
        .collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let mut name = String::new();
    for word in &words {
        let needed = if name.is_empty() { 0 } else { name.len() + 1 } + word.len();
        if needed > 26 {
            break;
        }
        if !name.is_empty() {
            name.push(' ');
        }
        name.push_str(word);
    }
    if name.is_empty() {
        name = words.first().map_or(String::new(), |w| w.chars().take(26).collect());
    }
    Some(match warning {
        Some(warning) => format!("GPU: {name}, {warning}"),
        None => format!("GPU: {name}"),
    })
}

#[cfg(test)]
mod tests {
    use super::gpu_note;

    #[test]
    fn a_software_renderer_is_named_and_warned_about() {
        assert_eq!(
            gpu_note("llvmpipe (LLVM 15.0.7, 256 bits)").as_deref(),
            Some("GPU: SOFTWARE (LLVMPIPE), DRAWN IN SOFTWARE: UPDATE THE GRAPHICS DRIVER")
        );
        assert_eq!(
            gpu_note("Google SwiftShader").as_deref(),
            Some("GPU: SOFTWARE (SWIFTSHADER), DRAWN IN SOFTWARE: UPDATE THE GRAPHICS DRIVER")
        );
    }

    #[test]
    fn a_hardware_name_loses_its_decoration_and_is_cut_at_26_letters() {
        assert_eq!(
            gpu_note("ANGLE (Intel, Intel(R) HD Graphics 3000 Direct3D11 vs_4_1 ps_4_1, D3D11)").as_deref(),
            Some("GPU: INTEL(R) HD GRAPHICS 3000")
        );
        assert_eq!(
            gpu_note("NVIDIA GeForce RTX 3080 Ti Laptop GPU/PCIe/SSE2").as_deref(),
            Some("GPU: NVIDIA GEFORCE RTX 3080 TI")
        );
    }

    #[test]
    fn an_unknown_renderer_has_no_note() {
        assert_eq!(gpu_note(""), None);
    }
}
