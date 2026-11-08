//! STUN (RFC 8489) binding and address attribute encoding/decoding, matching
//! `src/net/stun.js`. Used on the shared game UDP sockets for reflexive
//! address discovery (`UdpEndpoint.reflexive`).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use crate::{Error, Result, crypto};

pub const MAGIC: u32 = 0x2112_a442;

pub mod method {
    pub const BINDING: u16 = 0x001;
    pub const ALLOCATE: u16 = 0x003;
    pub const REFRESH: u16 = 0x004;
    pub const SEND: u16 = 0x006;
    pub const DATA: u16 = 0x007;
    pub const CREATE_PERMISSION: u16 = 0x008;
    pub const CHANNEL_BIND: u16 = 0x009;
}

pub mod class {
    pub const REQUEST: u8 = 0;
    pub const INDICATION: u8 = 1;
    pub const SUCCESS: u8 = 2;
    pub const ERROR: u8 = 3;
}

pub mod attr {
    pub const MAPPED_ADDRESS: u16 = 0x0001;
    pub const USERNAME: u16 = 0x0006;
    pub const MESSAGE_INTEGRITY: u16 = 0x0008;
    pub const ERROR_CODE: u16 = 0x0009;
    pub const CHANNEL_NUMBER: u16 = 0x000c;
    pub const LIFETIME: u16 = 0x000d;
    pub const XOR_PEER_ADDRESS: u16 = 0x0012;
    pub const DATA: u16 = 0x0013;
    pub const REALM: u16 = 0x0014;
    pub const NONCE: u16 = 0x0015;
    pub const XOR_RELAYED_ADDRESS: u16 = 0x0016;
    pub const REQUESTED_ADDRESS_FAMILY: u16 = 0x0017;
    pub const REQUESTED_TRANSPORT: u16 = 0x0019;
    pub const XOR_MAPPED_ADDRESS: u16 = 0x0020;
    pub const SOFTWARE: u16 = 0x8022;
    pub const FINGERPRINT: u16 = 0x8028;
}

pub const fn message_type(method: u16, klass: u8) -> u16 {
    (method & 0x0f)
        | ((method & 0x70) << 1)
        | ((method & 0xf80) << 2)
        | (((klass & 1) as u16) << 4)
        | (((klass & 2) as u16) << 7)
}

pub const fn split_type(raw: u16) -> (u16, u8) {
    let method = (raw & 0x0f) | ((raw >> 1) & 0x70) | ((raw >> 2) & 0xf80);
    let klass = (((raw >> 4) & 1) | ((raw >> 7) & 2)) as u8;
    (method, klass)
}

pub fn is_stun(packet: &[u8]) -> bool {
    packet.len() >= 20
        && packet[0] < 4
        && u32::from_be_bytes(packet[4..8].try_into().unwrap()) == MAGIC
}

pub fn is_channel_data(packet: &[u8]) -> bool {
    packet.len() >= 4 && (0x40..=0x4f).contains(&packet[0])
}

pub fn new_transaction() -> Result<[u8; 12]> {
    crypto::random()
}

pub fn encode_address(addr: SocketAddr, transaction: &[u8; 12], xor: bool) -> Vec<u8> {
    let mut mask = [0u8; 16];
    mask[..4].copy_from_slice(&MAGIC.to_be_bytes());
    mask[4..16].copy_from_slice(transaction);
    let port = if xor {
        addr.port() ^ ((MAGIC >> 16) as u16)
    } else {
        addr.port()
    };
    match addr.ip() {
        IpAddr::V4(v4) => {
            let mut out = vec![0u8; 8];
            out[1] = 1;
            out[2..4].copy_from_slice(&port.to_be_bytes());
            let raw = v4.octets();
            for i in 0..4 {
                out[4 + i] = if xor { raw[i] ^ mask[i] } else { raw[i] };
            }
            out
        }
        IpAddr::V6(v6) => {
            let mut out = vec![0u8; 20];
            out[1] = 2;
            out[2..4].copy_from_slice(&port.to_be_bytes());
            let raw = v6.octets();
            for i in 0..16 {
                out[4 + i] = if xor { raw[i] ^ mask[i] } else { raw[i] };
            }
            out
        }
    }
}

pub fn decode_address(value: &[u8], transaction: &[u8; 12], xor: bool) -> Result<SocketAddr> {
    if value.len() < 8 {
        return Err(Error("invalid STUN address"));
    }
    let family = value[1];
    let length = match family {
        1 => 4,
        2 => 16,
        _ => return Err(Error("invalid STUN address")),
    };
    if value.len() < 4 + length {
        return Err(Error("invalid STUN address"));
    }
    let mut mask = [0u8; 16];
    mask[..4].copy_from_slice(&MAGIC.to_be_bytes());
    mask[4..16].copy_from_slice(transaction);
    let raw_port = u16::from_be_bytes([value[2], value[3]]);
    let port = if xor {
        raw_port ^ ((MAGIC >> 16) as u16)
    } else {
        raw_port
    };
    if length == 4 {
        let mut octets = [0u8; 4];
        for i in 0..4 {
            octets[i] = if xor {
                value[4 + i] ^ mask[i]
            } else {
                value[4 + i]
            };
        }
        Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::from(octets)), port))
    } else {
        let mut octets = [0u8; 16];
        for i in 0..16 {
            octets[i] = if xor {
                value[4 + i] ^ mask[i]
            } else {
                value[4 + i]
            };
        }
        Ok(SocketAddr::new(IpAddr::V6(Ipv6Addr::from(octets)), port))
    }
}

/// `src/net/stun.js` `encodeMessage(method, klass, transaction, attributes)`:
/// encodes a STUN message with 4-byte padded attributes and trailing
/// `FINGERPRINT` (`0x8028`).
pub fn encode_message(
    method: u16,
    klass: u8,
    transaction: &[u8; 12],
    attributes: &[(u16, &[u8])],
) -> Vec<u8> {
    let mut body = Vec::new();
    for &(attr_type, value) in attributes {
        body.extend_from_slice(&attr_type.to_be_bytes());
        body.extend_from_slice(&(value.len() as u16).to_be_bytes());
        body.extend_from_slice(value);
        let pad = (4 - (value.len() % 4)) % 4;
        body.resize(body.len() + pad, 0);
    }
    let mut out = Vec::with_capacity(20 + body.len() + 8);
    out.extend_from_slice(&message_type(method, klass).to_be_bytes());
    out.extend_from_slice(&((body.len() + 8) as u16).to_be_bytes());
    out.extend_from_slice(&MAGIC.to_be_bytes());
    out.extend_from_slice(transaction);
    out.extend_from_slice(&body);
    let crc = crc32fast::hash(&out) ^ 0x5354_554e;
    out.extend_from_slice(&attr::FINGERPRINT.to_be_bytes());
    out.extend_from_slice(&4u16.to_be_bytes());
    out.extend_from_slice(&crc.to_be_bytes());
    out
}

pub fn encode_binding_request(transaction: &[u8; 12]) -> Vec<u8> {
    encode_message(method::BINDING, class::REQUEST, transaction, &[])
}

pub fn encode_binding_response(transaction: &[u8; 12], mapped: SocketAddr) -> Vec<u8> {
    let encoded = encode_address(mapped, transaction, true);
    encode_message(
        method::BINDING,
        class::SUCCESS,
        transaction,
        &[(attr::XOR_MAPPED_ADDRESS, &encoded)],
    )
}

#[derive(Clone, Debug)]
pub struct StunMessage<'a> {
    pub method: u16,
    pub klass: u8,
    pub transaction: [u8; 12],
    pub attributes: Vec<(u16, &'a [u8])>,
    pub packet: &'a [u8],
    pub integrity_at: Option<usize>,
}

impl<'a> StunMessage<'a> {
    pub fn attr(&self, attr_type: u16) -> Option<&'a [u8]> {
        self.attributes
            .iter()
            .find(|&&(k, _)| k == attr_type)
            .map(|&(_, v)| v)
    }

    pub fn address(&self, attr_type: u16) -> Option<SocketAddr> {
        let value = self.attr(attr_type)?;
        decode_address(value, &self.transaction, attr_type != attr::MAPPED_ADDRESS).ok()
    }

    pub fn mapped_address(&self) -> Option<SocketAddr> {
        self.address(attr::XOR_MAPPED_ADDRESS)
            .or_else(|| self.address(attr::MAPPED_ADDRESS))
    }

    pub fn error_code(&self) -> Option<(u16, String)> {
        let value = self.attr(attr::ERROR_CODE)?;
        if value.len() < 4 {
            return None;
        }
        let code = ((value[2] & 7) as u16) * 100 + (value[3] as u16);
        let reason = String::from_utf8_lossy(&value[4..]).into_owned();
        Some((code, reason))
    }
}

pub fn decode_message(packet: &[u8]) -> Option<StunMessage<'_>> {
    if !is_stun(packet) {
        return None;
    }
    let length = u16::from_be_bytes([packet[2], packet[3]]) as usize;
    if !length.is_multiple_of(4) || 20 + length > packet.len() {
        return None;
    }
    let (method, klass) = split_type(u16::from_be_bytes([packet[0], packet[1]]));
    let transaction: [u8; 12] = packet[8..20].try_into().ok()?;
    let mut attributes = Vec::new();
    let mut offset = 20;
    let mut integrity_at = None;
    while offset + 4 <= 20 + length {
        let attr_type = u16::from_be_bytes([packet[offset], packet[offset + 1]]);
        let size = u16::from_be_bytes([packet[offset + 2], packet[offset + 3]]) as usize;
        if offset + 4 + size > 20 + length {
            return None;
        }
        if integrity_at.is_some() && attr_type != attr::FINGERPRINT {
            break;
        }
        if attr_type == attr::MESSAGE_INTEGRITY {
            integrity_at = Some(offset);
        }
        let value = &packet[offset + 4..offset + 4 + size];
        attributes.push((attr_type, value));
        offset += 4 + size + ((4 - (size % 4)) % 4);
    }
    Some(StunMessage {
        method,
        klass,
        transaction,
        attributes,
        packet: &packet[..20 + length],
        integrity_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stun_binding_round_trip_ipv4_and_ipv6() {
        let tx = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        let req = encode_binding_request(&tx);
        let decoded_req = decode_message(&req).unwrap();
        assert_eq!(decoded_req.method, method::BINDING);
        assert_eq!(decoded_req.klass, class::REQUEST);
        assert_eq!(decoded_req.transaction, tx);

        let v4 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 42)), 18451);
        let resp_v4 = encode_binding_response(&tx, v4);
        let decoded_v4 = decode_message(&resp_v4).unwrap();
        assert_eq!(decoded_v4.klass, class::SUCCESS);
        assert_eq!(decoded_v4.mapped_address(), Some(v4));

        let v6 = SocketAddr::new(
            IpAddr::V6(Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0x1234)),
            54321,
        );
        let resp_v6 = encode_binding_response(&tx, v6);
        let decoded_v6 = decode_message(&resp_v6).unwrap();
        assert_eq!(decoded_v6.mapped_address(), Some(v6));
    }
}
