#![cfg_attr(not(target_os = "linux"), allow(dead_code))]
//! Linux: the window system's libraries are opened by name when the window is
//! made, not linked, so a missing one used to surface as a panic deep inside
//! the windowing library. Before the window is made they are looked for: when
//! the X11 set is incomplete but Wayland's is there, the window uses Wayland
//! (and the other way round); when neither session can be served, the player
//! reads which libraries are missing and which packages hold them.

/// Library file and the Debian/Ubuntu package that holds it.
type Needed = (&'static str, &'static str);

const X11: &[Needed] = &[
    ("libX11.so.6", "libx11-6"),
    ("libX11-xcb.so.1", "libx11-xcb1"),
    ("libxcb.so.1", "libxcb1"),
    ("libXcursor.so.1", "libxcursor1"),
    ("libXi.so.6", "libxi6"),
    ("libxkbcommon.so.0", "libxkbcommon0"),
    ("libxkbcommon-x11.so.0", "libxkbcommon-x11-0"),
];

const WAYLAND: &[Needed] = &[
    ("libwayland-client.so.0", "libwayland-client0"),
    ("libwayland-cursor.so.0", "libwayland-cursor0"),
    ("libwayland-egl.so.1", "libwayland-egl1"),
    ("libxkbcommon.so.0", "libxkbcommon0"),
    ("libEGL.so.1", "libegl1"),
];

#[derive(Debug, PartialEq, Eq)]
pub enum Backend {
    /// Whatever the windowing library picks (Wayland when the session has it).
    Default,
    X11,
    Wayland,
}

/// What the session offers and which of each set's libraries are missing.
pub fn decide(
    wayland_session: bool,
    x11_session: bool,
    missing_wayland: &[Needed],
    missing_x11: &[Needed],
) -> Result<Backend, String> {
    let wayland = wayland_session && missing_wayland.is_empty();
    let x11 = x11_session && missing_x11.is_empty();
    match (wayland, x11) {
        (true, _) if !x11_session || x11 => Ok(Backend::Default),
        (true, _) => Ok(Backend::Wayland),
        (false, true) if wayland_session => Ok(Backend::X11),
        (false, true) => Ok(Backend::Default),
        // No session at all: the windowing library says so in its own words.
        _ if !wayland_session && !x11_session => Ok(Backend::Default),
        _ => {
            let mut missing: Vec<Needed> = Vec::new();
            for (session, list) in [(wayland_session, missing_wayland), (x11_session, missing_x11)] {
                if session {
                    for item in list {
                        if !missing.contains(item) {
                            missing.push(*item);
                        }
                    }
                }
            }
            let files: Vec<&str> = missing.iter().map(|(file, _)| *file).collect();
            let packages: Vec<&str> = missing.iter().map(|(_, package)| *package).collect();
            Err(format!(
                "Battlefront Eternal cannot open its window: the system is missing {}.\n\
                 Install what is missing with your package manager (on Debian and Ubuntu: sudo apt install {}) and start the game again.",
                files.join(", "),
                packages.join(" ")
            ))
        }
    }
}

#[cfg(target_os = "linux")]
fn missing(list: &[Needed]) -> Vec<Needed> {
    list.iter()
        // SAFETY: these are the system's window libraries, which the window would load anyway.
        .filter(|(file, _)| unsafe { libloading::Library::new(file) }.is_err())
        .copied()
        .collect()
}

/// The backend the window should use on this system, or the message for a player who cannot get a window.
#[cfg(target_os = "linux")]
pub fn choose() -> Result<Backend, String> {
    let set = |key: &str| std::env::var_os(key).is_some_and(|v| !v.is_empty());
    let wayland_session = set("WAYLAND_DISPLAY") || set("WAYLAND_SOCKET");
    let x11_session = set("DISPLAY");
    decide(
        wayland_session,
        x11_session,
        &if wayland_session { missing(WAYLAND) } else { Vec::new() },
        &if x11_session { missing(X11) } else { Vec::new() },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const XKB_X11: Needed = ("libxkbcommon-x11.so.0", "libxkbcommon-x11-0");
    const WL: Needed = ("libwayland-client.so.0", "libwayland-client0");

    #[test]
    fn a_missing_library_picks_the_other_session_or_names_the_package() {
        assert_eq!(decide(true, true, &[], &[]), Ok(Backend::Default));
        assert_eq!(decide(false, true, &[], &[]), Ok(Backend::Default));
        // X11 without xkbcommon-x11 used to panic; with Wayland there, Wayland serves.
        assert_eq!(decide(true, true, &[], &[XKB_X11]), Ok(Backend::Wayland));
        assert_eq!(decide(true, true, &[WL], &[]), Ok(Backend::X11));
        let message = decide(false, true, &[], &[XKB_X11]).unwrap_err();
        assert!(message.contains("libxkbcommon-x11.so.0") && message.contains("sudo apt install libxkbcommon-x11-0"), "{message}");
        let both = decide(true, true, &[WL], &[XKB_X11]).unwrap_err();
        assert!(both.contains("libwayland-client0 libxkbcommon-x11-0"), "{both}");
        assert_eq!(decide(false, false, &[], &[]), Ok(Backend::Default));
    }
}
