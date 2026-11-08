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
        let delay = delay
            .filter(|d| d.is_finite())
            .unwrap_or(5000.)
            .clamp(0., 120000.);
        Ok(Self {
            query,
            ready_at: now + delay,
        })
    }
}
pub struct Probe {
    received: mpsc::Receiver<()>,
    cancelled: Arc<AtomicBool>,
}
impl Probe {
    // scheduleRotation polls readiness away from the window thread. The
    // endpoint must advertise the announced match before content is reloaded;
    // a stale /info response still belongs to the previous battle.
    pub fn start(address: String, query: String) -> Self {
        let (send, received) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        std::thread::spawn(move || {
            for _ in 0..120 {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                if Server::read(&address).is_ok_and(|s| s.query == query) {
                    let _ = send.send(());
                    break;
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        });
        Self {
            received,
            cancelled,
        }
    }
    pub fn ready(&self) -> bool {
        self.received.try_recv().is_ok()
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
