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
pub mod stun;
pub mod server;
#[cfg(feature="node")]
mod node;

// Static string slices keep protocol errors copyable without allocating on the
// packet path when a malformed frame or exhausted stream budget triggers a close.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Error(pub &'static str);
impl std::fmt::Display for Error {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{f.write_str(self.0)}
}
impl std::error::Error for Error{}
// All fallible transport operations return the zero-allocation protocol Error.
pub type Result<T> = std::result::Result<T,Error>;

// Steady-state zero-allocation verification tests run under a counting global allocator.
#[cfg(test)]
mod allocation_tests;
