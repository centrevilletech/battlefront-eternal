//! Native BFE-UDP/1 protocol. Sockets and server admission remain outside the
//! packet owner; encrypted messages borrow fixed reservations until delivery.
pub mod layout;
pub mod codec;
pub mod crypto;
pub mod handshake;
pub mod streams;
pub mod state;
pub mod connection;
pub mod endpoint;
#[cfg(feature="node")]
mod node;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Error(pub &'static str);
impl std::fmt::Display for Error {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{f.write_str(self.0)}
}
impl std::error::Error for Error{}
pub type Result<T> = std::result::Result<T,Error>;

#[cfg(test)]
mod allocation_tests;
