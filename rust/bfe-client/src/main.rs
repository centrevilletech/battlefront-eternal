//! Standalone native game binary (`bfe-client`).
// On Windows a windowed program: a player who starts it from the Start Menu gets the game's
// window only, not a console beside it full of log lines.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    // Started from a terminal (`bfe-client --version`, a scripted run), print there as before.
    #[cfg(windows)]
    attach_parent_console();
    bfe_client::app::run(true);
}

#[cfg(windows)]
fn attach_parent_console() {
    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    // SAFETY: AttachConsole takes a process id and only fails when there is no parent console.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}
