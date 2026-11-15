//! Command line of `bfeserver`: what each flag does, and the help text that says so.

use std::path::PathBuf;

pub const USAGE: &str = "\
Usage: bfeserver [--dir DIR] [COMMAND]

Commands:
  (none)           open the interactive setup and administration menu
  --serve          run the dedicated server
  --admin [JSON]   send one operator command to the running server
                   (default {\"action\":\"status\"})
  --version        print the version
  -h, --help       print this help

Options:
  --dir DIR        state directory: config.json, identity, operator database.
                   Default: $BFE_STATE_DIR, else $XDG_DATA_HOME/bfeserver,
                   else ~/.local/share/bfeserver

Environment:
  BFE_STATE_DIR        state directory when --dir is not given
  BFE_DATA_DIR         prepared game data directory (overrides \"data\")
  BFE_GAME_PORT        UDP game port and /info port (overrides \"gamePort\")
  BFE_BIND             bind address for the game port (overrides \"bind\")
  BFE_DISCOVERY_PORT   LAN discovery port (overrides \"discoveryPort\")
  BFE_MASTER_URL       online directory URL (overrides \"master\")
  BFE_ACCEPT_ONLINE=1  accept the online processing notice
  BFE_RELAY_POLICY     auto, never or always (overrides \"relayPolicy\")
  PORT                 loopback port of /admin and /healthz (default 18450)
";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Menu,
    Serve,
    Admin(String),
    Version,
    Help,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cli {
    pub command: Command,
    pub dir: Option<PathBuf>,
}

/// Parses the arguments after the program name. Anything not listed in [`USAGE`] is an error,
/// so a mistyped flag never falls through to the menu (which writes a config file).
pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Cli, String> {
    let mut command = None;
    let mut dir = None;
    let mut it = args.into_iter().peekable();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Cli { command: Command::Help, dir }),
            "--serve" => set(Command::Serve, &mut command)?,
            "--version" => set(Command::Version, &mut command)?,
            "--admin" => {
                let json = match it.peek() {
                    Some(next) if !next.starts_with("--") => it.next().unwrap(),
                    _ => "{\"action\":\"status\"}".to_string(),
                };
                set(Command::Admin(json), &mut command)?
            }
            "--dir" => {
                let value = it.next().ok_or("missing value for --dir")?;
                dir = Some(non_empty_dir(&value)?);
            }
            other => {
                if let Some(value) = other.strip_prefix("--dir=") {
                    dir = Some(non_empty_dir(value)?);
                } else {
                    return Err(format!("unknown argument: {other}"));
                }
            }
        }
    }
    Ok(Cli { command: command.unwrap_or(Command::Menu), dir })
}

fn set(c: Command, command: &mut Option<Command>) -> Result<(), String> {
    if command.is_some() {
        return Err("only one command can be given".to_string());
    }
    *command = Some(c);
    Ok(())
}

fn non_empty_dir(value: &str) -> Result<PathBuf, String> {
    if value.trim().is_empty() {
        return Err("--dir needs a directory".to_string());
    }
    Ok(PathBuf::from(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Cli, String> {
        parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn flags_and_errors() {
        assert_eq!(p(&[]).unwrap().command, Command::Menu);
        let serve = p(&["--serve", "--dir", "/srv/state"]).unwrap();
        assert_eq!(serve.command, Command::Serve);
        assert_eq!(serve.dir, Some(PathBuf::from("/srv/state")));
        assert_eq!(p(&["--dir=/a", "--serve"]).unwrap().dir, Some(PathBuf::from("/a")));
        assert_eq!(p(&["--admin"]).unwrap().command, Command::Admin("{\"action\":\"status\"}".into()));
        let admin = p(&["--admin", "{\"action\":\"stop\"}", "--dir", "/b"]).unwrap();
        assert_eq!(admin.command, Command::Admin("{\"action\":\"stop\"}".into()));
        assert_eq!(admin.dir, Some(PathBuf::from("/b")));
        assert_eq!(p(&["--serve", "--help"]).unwrap().command, Command::Help);
        assert_eq!(p(&["-h"]).unwrap().command, Command::Help);
        assert!(p(&["--serv"]).is_err());
        assert!(p(&["serve"]).is_err());
        assert!(p(&["--dir"]).is_err());
        assert!(p(&["--dir", ""]).is_err());
        assert!(p(&["--serve", "--version"]).is_err());
    }
}
