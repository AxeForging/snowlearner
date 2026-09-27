//! Local control channel: `snowlearner say|summary|quit` talks to the running
//! app over 127.0.0.1. This is how Wayland users (no global hotkeys) bind a
//! system shortcut, and it doubles as a single-instance guard.

use anyhow::{Context, Result, bail};
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Challenge,
    Summary,
    Dismiss,
    /// Open the control panel window.
    Menu,
    /// Pause/resume the frost mage (meetings, focus time).
    Pause,
    /// Magic hand for 15 s: pick up the mage or the warrior (for desktops where
    /// holding Ctrl+Alt can't be detected, e.g. Wayland).
    Grab,
    Quit,
    /// Open the control panel on the PROGRESSO tab.
    Progress,
}

impl Command {
    pub fn parse(line: &str) -> Option<Command> {
        match line.trim() {
            "challenge" | "say" => Some(Command::Challenge),
            "summary" => Some(Command::Summary),
            "dismiss" => Some(Command::Dismiss),
            "menu" => Some(Command::Menu),
            "pause" => Some(Command::Pause),
            "grab" => Some(Command::Grab),
            "quit" => Some(Command::Quit),
            "progress" => Some(Command::Progress),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Command::Challenge => "challenge",
            Command::Summary => "summary",
            Command::Dismiss => "dismiss",
            Command::Menu => "menu",
            Command::Pause => "pause",
            Command::Grab => "grab",
            Command::Quit => "quit",
            Command::Progress => "progress",
        }
    }
}

const GREETING: &str = "snowlearner";

/// Binds the port (fails if another instance owns it) and serves commands on a
/// background thread.
pub fn serve(port: u16, on_command: impl Fn(Command) + Send + 'static) -> Result<SocketAddr> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .with_context(|| format!("port {port} is busy — is snowlearner already running? (`snowlearner quit`)"))?;
    let addr = listener.local_addr()?;
    std::thread::Builder::new().name("ipc".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut reader = BufReader::new(&stream);
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let reply = match Command::parse(&line) {
                Some(cmd) => {
                    on_command(cmd);
                    format!("{GREETING} ok\n")
                }
                None => format!("{GREETING} error unknown command {:?}\n", line.trim()),
            };
            let _ = (&stream).write_all(reply.as_bytes());
        }
    })?;
    Ok(addr)
}

/// Sends one command to the running app.
pub fn send(port: u16, cmd: Command) -> Result<()> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(800)).with_context(|| {
        format!("snowlearner is not running (nothing on 127.0.0.1:{port}) — start it with `snowlearner`")
    })?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(format!("{}\n", cmd.as_str()).as_bytes())?;
    let mut reply = String::new();
    BufReader::new(&stream).read_line(&mut reply)?;
    match reply.trim() {
        r if r == format!("{GREETING} ok") => Ok(()),
        r if r.starts_with(GREETING) => bail!("app refused the command: {r}"),
        r => bail!("port {port} is used by another program (got {r:?}); change `ipc_port` in the config"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn commands_round_trip_to_the_running_instance() {
        let (tx, rx) = mpsc::channel();
        let addr = serve(0, move |c| tx.send(c).unwrap()).unwrap();
        for cmd in [Command::Challenge, Command::Summary, Command::Menu, Command::Pause, Command::Quit] {
            send(addr.port(), cmd).unwrap();
            assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), cmd);
        }
    }

    #[test]
    fn a_second_instance_cannot_bind_the_same_port() {
        let addr = serve(0, |_| {}).unwrap();
        let err = format!("{:#}", serve(addr.port(), |_| {}).unwrap_err());
        assert!(err.contains("already running"), "{err}");
    }

    #[test]
    fn sending_with_nothing_listening_explains_how_to_start() {
        let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap().local_addr().unwrap().port();
        let err = format!("{:#}", send(port, Command::Challenge).unwrap_err());
        assert!(err.contains("not running"), "{err}");
    }

    #[test]
    fn a_foreign_program_on_the_port_is_detected() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let _ = s.write_all(b"HTTP/1.1 400 Bad Request\n");
            }
        });
        let err = format!("{:#}", send(port, Command::Challenge).unwrap_err());
        assert!(err.contains("another program"), "{err}");
    }

    #[test]
    fn say_is_an_alias_for_challenge() {
        assert_eq!(Command::parse("say\n"), Some(Command::Challenge));
        assert_eq!(Command::parse("dance"), None);
    }
}
