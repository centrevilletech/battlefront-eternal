//! NetworkSession's match notification and readiness polling. Loading the next
//! battle uses typed launch state; browser URLs and navigation stay out of it.
use super::info::Server;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};
#[derive(Debug)]
pub struct NextMatch {
    pub query: String,
    pub family: bfe_family::GameFamily,
    pub map: String,
    pub side: String,
    pub era: String,
    pub ready_at: f64,
}
impl NextMatch {
    // NetworkSession.receive clamps the server's delay and requires a fresh
    // match token. A malformed message never discards the displayed end table.
    pub fn new(query: String, delay: Option<f64>, now: f64) -> Result<Self, &'static str> {
        let params = bfe_content::mission::Query::parse(&query);
        let token = params.get("match");
        if query.len() > 2048
            || token.len() != 32
            || !token
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("invalid next match");
        }
        // A battle in a namespace this build cannot load is refused when it is
        // announced: the end table's normal error path, before any wait, probe
        // or content. A 2005 battle is joined as a current one is: played or
        // sat out as the gate decides.
        let next = super::selection::Selection::announced(&query)?;
        let delay = delay
            .filter(|d| d.is_finite())
            .unwrap_or(5000.)
            .clamp(0., 120000.);
        Ok(Self {
            query,
            family: next.family,
            map: next.map,
            side: next.side,
            era: next.era,
            ready_at: now + delay,
        })
    }
}
/// What a probe found: whether the server named the battle asked about, or
/// that it runs another version, which no waiting can change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    Decided(bool),
    Mismatch(String),
}
pub struct Probe {
    received: mpsc::Receiver<Answer>,
    cancelled: Arc<AtomicBool>,
    found: std::cell::RefCell<Option<Answer>>,
}
impl Probe {
    // scheduleRotation polls readiness away from the window thread. The
    // endpoint must advertise the announced match before content is reloaded;
    // a stale /info response still belongs to the previous battle.
    pub fn start(address: String, query: String) -> Self {
        // A listed server reached through the master is asked through the master's signalling
        // whether it runs the announced match (its listing may carry no battle query), and its
        // listing's version is compared with this build's: another version is final at once.
        if let Some(id) = address.strip_prefix(super::info::LISTED) {
            let token = bfe_content::mission::Query::parse(&query).get("match").to_string();
            let (send, received) = mpsc::channel();
            let cancelled = Arc::new(AtomicBool::new(false));
            let stop = cancelled.clone();
            let id = id.to_string();
            std::thread::spawn(move || {
                match super::info::listed_match_ready(&id, &token, &stop) {
                    Answer::Decided(false) => {}
                    answer => { let _ = send.send(answer); }
                }
            });
            return Self { received, cancelled, found: Default::default() };
        }
        Self::spawn(address, move |s| (s.query == query).then_some(true))
    }
    // After a join refused for content: true once /info names another battle
    // than the one loaded, false while it still names that one (a real mismatch).
    pub fn changed(address: String, loaded: String) -> Self {
        Self::spawn(address, move |s| Some(s.query != loaded))
    }
    fn spawn(address: String, decide: impl Fn(&Server) -> Option<bool> + Send + 'static) -> Self {
        let (send, received) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        std::thread::spawn(move || {
            for _ in 0..120 {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                match super::info::read_target(&address) {
                    Ok(server) => if let Some(answer) = decide(&server) {
                        let _ = send.send(Answer::Decided(answer));
                        break;
                    },
                    // Another version is final: say so at once instead of
                    // polling a server this build cannot join.
                    Err(text) if text.starts_with("This server runs ") => {
                        let _ = send.send(Answer::Mismatch(text));
                        break;
                    }
                    Err(_) => {}
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        });
        Self {
            received,
            cancelled,
            found: Default::default(),
        }
    }
    fn found(&self) -> Option<Answer> {
        let mut found = self.found.borrow_mut();
        if found.is_none() { *found = self.received.try_recv().ok(); }
        found.clone()
    }
    pub fn ready(&self) -> bool {
        self.answer() == Some(true)
    }
    pub fn answer(&self) -> Option<bool> {
        match self.found() { Some(Answer::Decided(answer)) => Some(answer), _ => None }
    }
    /// The server's version text, once the probe found it runs another version.
    pub fn mismatch(&self) -> Option<String> {
        match self.found() { Some(Answer::Mismatch(text)) => Some(text), _ => None }
    }
}
impl Drop for Probe {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readiness_ignores_the_previous_match() {
        use bfe_transport::layout::network_contract as contract;
        use std::{
            io::{Read, Write},
            net::{Ipv4Addr, TcpListener},
            time::Instant,
        };
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let query = "?map=tat1&match=0123456789abcdef0123456789abcdef";
        let (sent, first) = mpsc::channel();
        let server = std::thread::spawn(move || {
            for (i, q) in ["?map=nab2&match=00000000000000000000000000000000", query]
                .into_iter()
                .enumerate()
            {
                let (mut socket, _) = listener.accept().unwrap();
                // write_fmt may send HTTP in several TCP writes. Read the
                // complete header before closing the mock response socket;
                // unread request bytes would turn close into a TCP reset.
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                let mut chunk = [0; 256];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = socket.read(&mut chunk).unwrap();
                    assert!(n > 0);
                    request.extend_from_slice(&chunk[..n]);
                }
                let body=serde_json::json!({"id":"0".repeat(64),"protocol":contract::PROTOCOL,"profile":contract::PROFILE,"simulation":contract::SIMULATION,"content":"fixture","query":q}).to_string();
                write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
                if i == 0 {
                    sent.send(()).unwrap();
                }
            }
        });
        let probe = Probe::start(address, query.into());
        first.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(!probe.ready());
        let deadline = Instant::now() + Duration::from_secs(10);
        while !probe.ready() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        server.join().unwrap();
    }

    // A server that now runs another version answers both probes at once,
    // with the player's version text, instead of after a minute of polling.
    #[test]
    fn another_version_is_reported_at_once() {
        use bfe_transport::layout::network_contract as contract;
        use std::{io::{Read, Write}, net::{Ipv4Addr, TcpListener}, time::Instant};
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let server = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().unwrap();
                socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                let mut request = Vec::new();
                let mut chunk = [0; 256];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = socket.read(&mut chunk).unwrap();
                    assert!(n > 0);
                    request.extend_from_slice(&chunk[..n]);
                }
                let body = serde_json::json!({"id": "0".repeat(64), "protocol": contract::PROTOCOL, "profile": contract::PROFILE,
                    "simulation": "bfe-sim-1", "content": "fixture", "query": "?map=tat1&match=0123456789abcdef0123456789abcdef"}).to_string();
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
        });
        let started = Instant::now();
        for probe in [Probe::start(address.clone(), "?map=tat1&match=0123456789abcdef0123456789abcdef".into()),
            Probe::changed(address.clone(), "?map=nab2".into())]
        {
            let deadline = Instant::now() + Duration::from_secs(5);
            while probe.mismatch().is_none() {
                assert!(Instant::now() < deadline, "no answer");
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(probe.mismatch().unwrap().starts_with("This server runs an older version"));
            assert_eq!((probe.answer(), probe.ready()), (None, false));
        }
        assert!(started.elapsed() < Duration::from_secs(5));
        server.join().unwrap();
    }

    #[test]
    fn a_refused_join_follows_only_a_changed_battle() {
        use bfe_transport::layout::network_contract as contract;
        use std::{
            io::{Read, Write},
            net::{Ipv4Addr, TcpListener},
            time::Instant,
        };
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let now = "?map=tat1&match=0123456789abcdef0123456789abcdef";
        let server = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().unwrap();
                socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                let mut request = Vec::new();
                let mut chunk = [0; 256];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = socket.read(&mut chunk).unwrap();
                    assert!(n > 0);
                    request.extend_from_slice(&chunk[..n]);
                }
                let body=serde_json::json!({"id":"0".repeat(64),"protocol":contract::PROTOCOL,"profile":contract::PROFILE,"simulation":contract::SIMULATION,"content":"fixture","query":now}).to_string();
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
        });
        let answer = |probe: Probe| {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(answer) = probe.answer() {
                    return answer;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        // The server still plays the loaded battle: a real content mismatch.
        assert!(!answer(Probe::changed(address.clone(), now.into())));
        // It plays another battle now: reload into it.
        assert!(answer(Probe::changed(address, "?map=nab2&match=00000000000000000000000000000000".into())));
        server.join().unwrap();
    }

    #[test]
    fn match_notification_validates_token_and_clamps_delay() {
        let query = "?map=tat1&side=rep&match=0123456789abcdef0123456789abcdef";
        assert_eq!(
            NextMatch::new(query.into(), Some(-1.), 10.)
                .unwrap()
                .ready_at,
            10.
        );
        assert_eq!(
            NextMatch::new(query.into(), Some(200000.), 10.)
                .unwrap()
                .ready_at,
            120010.
        );
        assert_eq!(
            NextMatch::new(query.into(), None, 10.).unwrap().ready_at,
            5010.
        );
        assert!(NextMatch::new("?map=tat1".into(), None, 0.).is_err());
    }
}
