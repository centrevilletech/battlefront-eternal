//! endpoint.js's direct-client socket host. Identity and application hellos
//! arrive from the caller; bans, admission, relay and service policy remain
//! at bfeserver. A live pump borrows messages and never creates a native world.
use std::{net::{SocketAddr,UdpSocket,ToSocketAddrs},time::{Instant,Duration}};
use crate::{Result,Error,crypto::Identity,handshake::{ClientHandshake,Event},connection::{Connection,Lane,Sink},layout::{limits as l,packet,common,close_code}};
struct Candidate{path:SocketAddr,socket:usize,token:[u8;l::MAX_TOKEN],length:usize,retries:u8,rejected:bool}
struct Socket{udp:UdpSocket,v6:bool}
struct Output<'a,F>{sockets:&'a[Socket],messages:&'a mut F,now:f64}
impl<F:FnMut(Lane,bool,&[u8],f64)> Sink for Output<'_,F>{
    fn packet(&mut self,bytes:&[u8],path:SocketAddr){
        if let Some(socket)=self.sockets.iter().find(|s|s.v6==path.is_ipv6()){
            // Socket pressure is packet loss. Reliable ranges stay tracked by
            // the connection and are retried; replaceable state is never queued
            // behind the OS send buffer or promoted to a reliable lane.
            send_datagram(&socket.udp,bytes,path);
        }
    }
    fn message(&mut self,lane:Lane,text:bool,bytes:&[u8]){(self.messages)(lane,text,bytes,self.now);}
}
pub struct Endpoint{
    sockets:Vec<Socket>,candidates:Vec<Candidate>,handshake:Option<ClientHandshake>,pub connection:Option<Connection>,
    start:Instant,last_handshake:f64,chosen:Option<usize>,ready:bool,pub server_hello:Vec<u8>,pub identity:[u8;32],
}
impl Endpoint{
    // DNS, sockets and identity proofs are connection-time work. The display
    // phase later iterates these already-open candidates without allocation.
    pub fn connect(address:&str,identity:Identity,expected:[u8;32],hello:&[u8])->Result<Self>{
        let paths=address.to_socket_addrs().map_err(|_|Error("cannot resolve server address"))?;
        let mut candidates=Vec::with_capacity(16);let mut sockets:Vec<Socket>=Vec::with_capacity(2);
        for path in paths.take(16){
            let path=match path{SocketAddr::V6(v)=>v.ip().to_ipv4_mapped().map_or(path,|ip|SocketAddr::from((ip,v.port()))),_=>path};
            if path.port()==0||path.ip().is_unspecified()||path.ip().is_multicast()||candidates.iter().any(|c:&Candidate|c.path==path){continue;}
            let socket=if let Some(i)=sockets.iter().position(|s|s.v6==path.is_ipv6()){i}else{
                let udp=UdpSocket::bind(if path.is_ipv6(){"[::]:0"}else{"0.0.0.0:0"}).map_err(|_|Error("cannot open UDP socket"))?;
                udp.set_nonblocking(true).map_err(|_|Error("cannot configure UDP socket"))?;sockets.push(Socket{udp,v6:path.is_ipv6()});sockets.len()-1
            };
            candidates.push(Candidate{path,socket,token:[0;l::MAX_TOKEN],length:0,retries:0,rejected:false});
        }
        if candidates.is_empty(){return Err(Error("no usable server address"));}let id=identity.id;
        Ok(Self{sockets,candidates,handshake:Some(ClientHandshake::new(identity,expected,hello)?),connection:None,start:Instant::now(),last_handshake:f64::NEG_INFINITY,chosen:None,ready:false,server_hello:Vec::with_capacity(l::MAX_HELLO),identity:id})
    }
    // A single monotonic origin stamps retries, RTT samples and application
    // receipts. Wall-clock corrections never move the transport backward.
    pub fn now(&self)->f64{self.start.elapsed().as_secs_f64()*1000.}
    pub fn ready(&self)->bool{self.ready}
    // Error formatting is terminal work. Preserve the authenticated CLOSE
    // text instead of hiding a kick or shutdown behind a generic socket line.
    pub fn failure(&self,error:Error)->String{
        self.connection.as_ref().and_then(|c|c.closed).map_or_else(||error.to_string(),|c|format!("server closed connection ({}): {}",c.code,String::from_utf8_lossy(c.reason())))
    }
    // Expose the selected bound socket only after path selection. Other
    // address-family candidates cannot impersonate that connection endpoint.
    pub fn local_addr(&self)->Option<SocketAddr>{self.chosen.and_then(|i|self.sockets[self.candidates[i].socket].udp.local_addr().ok())}
    // Connect is cold admission. Reserve packet/ring storage before returning
    // to the display loop, while retaining any early snapshot receipt times.
    pub fn establish(&mut self,timeout:Duration,messages:&mut impl FnMut(Lane,bool,&[u8],f64))->Result<()>{
        while !self.ready{
            self.pump(messages)?;if self.start.elapsed()>=timeout{return Err(Error("battle connection timed out"));}
            std::thread::sleep(Duration::from_millis(2));
        }Ok(())
    }
    // Queueing borrows application bytes just long enough to copy them into
    // the transport reservation; later frames may immediately reuse the source.
    pub fn send(&mut self,lane:Lane,text:bool,bytes:&[u8])->Result<bool>{self.connection.as_mut().map_or(Ok(false),|c|c.queue(lane,text,bytes))}
    /// Publish a completed application batch without another receive turn.
    /// No callback is needed: flush emits packets, never application messages.
    pub fn flush(&mut self)->Result<()>{
        let now=self.now();if let Some(c)=&mut self.connection{
            let mut ignore=|_,_,_:&[u8],_|{};
            c.flush(now,&mut Output{sockets:&self.sockets,messages:&mut ignore,now})?;
        }Ok(())
    }
    pub fn close(&mut self,code:u64,reason:&[u8]){if let Some(c)=&mut self.connection{c.close(code,reason);}}
    pub fn pump(&mut self,messages:&mut impl FnMut(Lane,bool,&[u8],f64))->Result<()>{
        let now=self.now();let mut bytes=[0;1500];
        for socket in 0..self.sockets.len(){
            // A datagram flood has a bounded turn, so native input and drawing
            // still get their next frame. Remaining packets stay in the kernel.
            for _ in 0..256{
                let (n,path)=match self.sockets[socket].udp.recv_from(&mut bytes){Ok(v)=>v,Err(e)if matches!(e.kind(),std::io::ErrorKind::WouldBlock|std::io::ErrorKind::TimedOut)=>break,Err(e)=>{eprintln!("client UDP receive local={:?} capacity={} raw_os_error={:?}: {e}",self.sockets[socket].udp.local_addr(),bytes.len(),e.raw_os_error());if datagram_drop(&e){continue;}return Err(Error("UDP receive failed"));}};
                let packet=&bytes[..n];if n<common::BYTES{continue;}
                if matches!(packet[0]as usize,packet::DATA|packet::DATA_PHASE){
                    if let Some(c)=&mut self.connection{
                        let mut out=Output{sockets:&self.sockets,messages,now};if c.receive(packet,path,now,&mut out){self.ready=true;}
                    }continue;
                }
                // Handshake replies are accepted only from an initiated path.
                // A different socket or address cannot spend another path's retry.
                let Some(index)=self.candidates.iter().position(|c|c.path==path&&c.socket==socket&&!c.rejected)else{continue;};
                if self.chosen.is_some_and(|chosen|chosen!=index){continue;}
                let Some(h)=&mut self.handshake else{continue;};
                match h.receive(packet){
                    Event::Ignore=>{},
                    Event::Retry=>{let candidate=&mut self.candidates[index];if candidate.retries>=2{candidate.rejected=true;}else{candidate.retries+=1;candidate.length=h.token().len();candidate.token[..candidate.length].copy_from_slice(h.token());self.last_handshake=f64::NEG_INFINITY;}},
                    Event::Version(_)|Event::IdentityMismatch=>{self.candidates[index].rejected=true;},
                    Event::Duplicate=>{send_datagram(&self.sockets[socket].udp,h.confirm(),path);},
                    Event::Established=>{
                        let Some(keys)=h.take_keys()else{continue;};self.server_hello.extend_from_slice(&h.server_hello);self.chosen=Some(index);
                        self.connection=Some(Connection::new(keys,path,now,now.max(1.)));send_datagram(&self.sockets[socket].udp,h.confirm(),path);self.last_handshake=now;
                    },
                }
            }
        }
        // After a response, resend the identical confirmation. Sending new
        // initiations would create needless half-open sessions at the server.
        if !self.ready&&now-self.last_handshake>=250.{
            let h=self.handshake.as_ref().ok_or(Error("handshake is unavailable"))?;
            if let Some(index)=self.chosen{let c=&self.candidates[index];send_datagram(&self.sockets[c.socket].udp,h.confirm(),c.path);}
            else{let mut init=[0;l::INITIATION_BYTES];let mut any=false;for c in &self.candidates{if !c.rejected{any=true;h.initiation(&c.token[..c.length],&mut init)?;send_datagram(&self.sockets[c.socket].udp,&init,c.path);}}
                if !any{return Err(Error("server identity or transport version does not match"));}}
            self.last_handshake=now;
        }
        if let Some(c)=&mut self.connection{
            let mut out=Output{sockets:&self.sockets,messages,now};
            if let Err(error)=c.poll(now,&mut out){c.close(close_code::PROTOCOL as u64,error.0.as_bytes());let _=c.flush(now,&mut out);return Err(error);}
            if c.closed.is_some(){return Err(Error("connection to server closed"));}
        }
        // Release handshake state once the first authenticated DATA packet arrives.
        if self.ready{self.handshake=None;}Ok(())
    }
}

// Unconnected UDP can report an ICMP rejection for just one candidate on
// Windows. Drop that receipt; handshake/idle deadlines own session failure.
fn datagram_drop(error:&std::io::Error)->bool{
    matches!(error.kind(),std::io::ErrorKind::Interrupted|std::io::ErrorKind::ConnectionReset|std::io::ErrorKind::ConnectionRefused)
}
// Verify that transient per-datagram socket errors are classified as drops rather than fatal failures.
// Handshake and established traffic use the same error diagnostics; successful
// sends remain allocation-free and retain ordinary packet-loss semantics.
fn send_datagram(socket:&UdpSocket,bytes:&[u8],path:SocketAddr){
    if let Err(error)=socket.send_to(bytes,path){
        eprintln!("client UDP send local={:?} to={path} bytes={} raw_os_error={:?}: {error}",socket.local_addr(),bytes.len(),error.raw_os_error());
    }
}

#[cfg(test)]mod tests{
    #[test]fn per_datagram_errors_do_not_end_a_session(){
        use std::io::{Error,ErrorKind};
        for kind in [ErrorKind::Interrupted,ErrorKind::ConnectionReset,ErrorKind::ConnectionRefused]{assert!(super::datagram_drop(&Error::from(kind)));}
        assert!(!super::datagram_drop(&Error::from(ErrorKind::PermissionDenied)));
    }
}
