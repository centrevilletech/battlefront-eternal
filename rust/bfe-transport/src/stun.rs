//! STUN (RFC 8489) binding and address attribute encoding/decoding, matching
//! `src/net/stun.js`. Used on the shared game UDP sockets for reflexive
//! address discovery (`UdpEndpoint.reflexive`).
//!
//! Implements the subset of STUN/TURN framing required by BFE endpoints:
//! - 20-byte fixed header with RFC 8489 magic cookie (`0x2112a442`) and
//!   96-bit transaction ID.
//! - RFC 7983 first-byte multiplexing (`0..=3` for STUN, `0x40..=0x4f` for TURN
//!   ChannelData) so STUN and BFE game packets can share a single UDP socket.
//! - `XOR-MAPPED-ADDRESS` / `XOR-PEER-ADDRESS` / `XOR-RELAYED-ADDRESS` masking
//!   for both IPv4 and IPv6 socket addresses, plus `FINGERPRINT` (`0x8028`)
//!   CRC-32 generation and attribute boundary validation.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use crate::{Error, Result, crypto};

/// Fixed 32-bit STUN magic cookie (`0x2112a442`) from RFC 8489 Section 5.
pub const MAGIC: u32 = 0x2112_a442;

/// STUN and TURN 12-bit method numbers (`src/net/stun.js` `METHOD`).
pub mod method {
    /// RFC 8489 Binding method (`0x001`) for reflexive address discovery.
    /// RFC 8656 TURN Allocate (`0x003`), Refresh (`0x004`), Send (`0x006`),
    /// Data (`0x007`), CreatePermission (`0x008`), and ChannelBind (`0x009`).
    pub const BINDING: u16 = 0x001;
    pub const ALLOCATE: u16 = 0x003;
    pub const REFRESH: u16 = 0x004;
    pub const SEND: u16 = 0x006;
    pub const DATA: u16 = 0x007;
    pub const CREATE_PERMISSION: u16 = 0x008;
    pub const CHANNEL_BIND: u16 = 0x009;
}

/// 2-bit STUN message class values (`src/net/stun.js` `CLASS`).
pub mod class {
    /// Request (`0b00`), Indication (`0b01`), Success Response (`0b10`),
    /// and Error Response (`0b11`).
    pub const REQUEST: u8 = 0;
    pub const INDICATION: u8 = 1;
    pub const SUCCESS: u8 = 2;
    pub const ERROR: u8 = 3;
}

/// 16-bit STUN and TURN attribute type codes (`src/net/stun.js` `ATTR`).
pub mod attr {
    /// Comprehension-required (`0x0000..=0x7fff`) and comprehension-optional
    /// (`0x8000..=0xffff`) STUN/TURN attribute identifiers.
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

/// Interleaves a 12-bit STUN `method` and 2-bit `klass` into the 16-bit wire
/// message type field (`src/net/stun.js` `messageType`).
pub const fn message_type(method: u16, klass: u8) -> u16 {
    (method & 0x0f)
        | ((method & 0x70) << 1)
        | ((method & 0xf80) << 2)
        | (((klass & 1) as u16) << 4)
        | (((klass & 2) as u16) << 7)
}

/// Extracts the 12-bit `(method, klass)` pair from a 16-bit STUN message type
/// header word (`src/net/stun.js` `splitType`).
pub const fn split_type(raw: u16) -> (u16, u8) {
    let method = (raw & 0x0f) | ((raw >> 1) & 0x70) | ((raw >> 2) & 0xf80);
    let klass = (((raw >> 4) & 1) | ((raw >> 7) & 2)) as u8;
    (method, klass)
}

/// Returns `true` if `packet` has at least a 20-byte header, a leading byte in
/// `0..=3` (per RFC 7983), and the RFC 8489 `MAGIC` cookie at bytes `4..8`
/// (`src/net/stun.js` `isStun`).
pub fn is_stun(packet: &[u8]) -> bool {
    packet.len() >= 20
        && packet[0] < 4
        && packet.get(4..8) == Some(&MAGIC.to_be_bytes())
}

/// Returns `true` if `packet` starts with a TURN ChannelData channel number
/// in `0x4000..=0x4fff` (`src/net/stun.js` `isChannelData`).
pub fn is_channel_data(packet: &[u8]) -> bool {
    packet.len() >= 4 && (0x40..=0x4f).contains(&packet[0])
}

/// Generates a cryptographically random 96-bit (12-byte) STUN transaction ID
/// (`src/net/stun.js` `newTransaction`).
pub fn new_transaction() -> Result<[u8; 12]> {
    crypto::random()
}

/// Encodes an IPv4 (8-byte) or IPv6 (20-byte) STUN address attribute body
/// (`src/net/stun.js` `encodeAddress`). When `xor` is `true`, XORs the port
/// with the high 16 bits of `MAGIC` and the IP octets with `MAGIC || transaction`.
pub fn encode_address(addr: SocketAddr, transaction: &[u8; 12], xor: bool) -> Vec<u8> {
    let mut mask = [0u8; 16];
    mask[..4].copy_from_slice(&MAGIC.to_be_bytes());
    mask[4..16].copy_from_slice(transaction);
    let port = if xor {
        addr.port() ^ ((MAGIC >> 16) as u16)
    } else {
        addr.port()
    };
    // Format the 4-byte attribute header (`[0, family, port_hi, port_lo]`) followed
    // by 4 octets for IPv4 (`family = 1`) or 16 octets for IPv6 (`family = 2`).
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

/// Decodes an IPv4 (`family = 1`) or IPv6 (`family = 2`) STUN address attribute
/// payload (`src/net/stun.js` `decodeAddress`), reversing XOR masking when `xor` is `true`.
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
    // Reconstruct the 16-byte XOR pad (`MAGIC || transaction_id`) used to unmask
    // `XOR-MAPPED-ADDRESS`, `XOR-PEER-ADDRESS`, and `XOR-RELAYED-ADDRESS` attributes.
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
    // Serialize each `(type, length, value)` attribute and zero-pad its payload
    // to the next 4-byte boundary as required by RFC 8489 Section 14.
    let mut body = Vec::new();
    for &(attr_type, value) in attributes {
        body.extend_from_slice(&attr_type.to_be_bytes());
        body.extend_from_slice(&(value.len() as u16).to_be_bytes());
        body.extend_from_slice(value);
        let pad = (4 - (value.len() % 4)) % 4;
        body.resize(body.len() + pad, 0);
    }
    // Prepend the 20-byte STUN header (length includes the 8-byte trailing FINGERPRINT
    // attribute) and append the CRC-32 XOR 0x5354554e (`"STUN"`) fingerprint.
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

/// Encodes a minimal STUN Binding Request (`method::BINDING`, `class::REQUEST`)
/// with no body attributes beyond the trailing `FINGERPRINT` (`src/net/stun.js` `encodeBindingRequest`).
pub fn encode_binding_request(transaction: &[u8; 12]) -> Vec<u8> {
    encode_message(method::BINDING, class::REQUEST, transaction, &[])
}

/// Encodes a STUN Binding Success Response carrying `mapped` inside an
/// `XOR-MAPPED-ADDRESS` (`0x0020`) attribute (`src/net/stun.js` `encodeBindingResponse`).
pub fn encode_binding_response(transaction: &[u8; 12], mapped: SocketAddr) -> Vec<u8> {
    let encoded = encode_address(mapped, transaction, true);
    encode_message(
        method::BINDING,
        class::SUCCESS,
        transaction,
        &[(attr::XOR_MAPPED_ADDRESS, &encoded)],
    )
}

/// Zero-copy view over a decoded STUN packet and its TLV attributes
/// (`src/net/stun.js` `decodeMessage` return object).
#[derive(Clone, Debug)]
pub struct StunMessage<'a> {
    /// Decoded 12-bit STUN method (`method::BINDING`, etc.).
    /// Decoded 2-bit STUN message class (`class::REQUEST`, `class::SUCCESS`, etc.).
    /// 96-bit transaction identifier from bytes `8..20` of the STUN header.
    /// Ordered list of `(attr_type, raw_unpadded_value_slice)` attributes.
    /// Complete STUN packet slice (`header + body`, excluding any trailing bytes).
    /// Byte offset of `MESSAGE-INTEGRITY` (`0x0008`), if present in the packet.
    pub method: u16,
    pub klass: u8,
    pub transaction: [u8; 12],
    pub attributes: Vec<(u16, &'a [u8])>,
    pub packet: &'a [u8],
    pub integrity_at: Option<usize>,
}

impl<'a> StunMessage<'a> {
    /// Returns the raw byte slice of the first attribute matching `attr_type`.
    pub fn attr(&self, attr_type: u16) -> Option<&'a [u8]> {
        self.attributes
            .iter()
            .find(|&&(k, _)| k == attr_type)
            .map(|&(_, v)| v)
    }

    /// Decodes an address attribute (`XOR_MAPPED_ADDRESS`, `MAPPED_ADDRESS`,
    /// `XOR_PEER_ADDRESS`, or `XOR_RELAYED_ADDRESS`) using this message's transaction ID.
    pub fn address(&self, attr_type: u16) -> Option<SocketAddr> {
        let value = self.attr(attr_type)?;
        decode_address(value, &self.transaction, attr_type != attr::MAPPED_ADDRESS).ok()
    }

    /// Extracts the reflexive address from `XOR_MAPPED_ADDRESS`, falling back
    /// to legacy `MAPPED_ADDRESS` if present.
    pub fn mapped_address(&self) -> Option<SocketAddr> {
        self.address(attr::XOR_MAPPED_ADDRESS)
            .or_else(|| self.address(attr::MAPPED_ADDRESS))
    }

    /// Parses an RFC 8489 `ERROR-CODE` (`0x0009`) attribute into `(status_code, reason_phrase)`
    /// where `status_code = class * 100 + number` (`src/net/stun.js` `errorCode`).
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

/// Validates and parses a STUN packet (`src/net/stun.js` `decodeMessage`).
/// Enforces 4-byte message length alignment, bounds-checks each TLV attribute,
/// and stops parsing attributes after `MESSAGE_INTEGRITY` except for `FINGERPRINT`.
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
    // Walk the 4-byte-aligned attribute TLV stream starting immediately after
    // the 20-byte fixed header, recording `integrity_at` if MESSAGE-INTEGRITY is seen.
    let mut attributes = Vec::new();
    let mut offset = 20;
    let mut integrity_at = None;
    while offset + 4 <= 20 + length {
        let attr_type = u16::from_be_bytes([packet[offset], packet[offset + 1]]);
        let size = u16::from_be_bytes([packet[offset + 2], packet[offset + 3]]) as usize;
        if offset + 4 + size > 20 + length {
            return None;
        }
        // Per RFC 8489 Section 14.5, ignore any attributes appearing after
        // MESSAGE-INTEGRITY other than the trailing FINGERPRINT attribute.
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

    // Verifies STUN Binding Request and Success Response round-trip encoding
    // and `XOR-MAPPED-ADDRESS` decoding for both IPv4 and IPv6 socket addresses.
    #[test]
    fn stun_binding_round_trip_ipv4_and_ipv6(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let tx = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        let req = encode_binding_request(&tx);
        let decoded_req = decode_message(&req).ok_or("expected decoded request")?;
        assert_eq!(decoded_req.method, method::BINDING);
        assert_eq!(decoded_req.klass, class::REQUEST);
        assert_eq!(decoded_req.transaction, tx);

        // Verify IPv4 `XOR-MAPPED-ADDRESS` masking and unmasking on RFC 5737 TEST-NET-3.
        let v4 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 42)), 18451);
        let resp_v4 = encode_binding_response(&tx, v4);
        let decoded_v4 = decode_message(&resp_v4).ok_or("expected decoded v4 response")?;
        assert_eq!(decoded_v4.klass, class::SUCCESS);
        assert_eq!(decoded_v4.mapped_address(), Some(v4));

        // Verify IPv6 `XOR-MAPPED-ADDRESS` masking across all 16 bytes of `MAGIC || tx`.
        let v6 = SocketAddr::new(
            IpAddr::V6(Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0x1234)),
            54321,
        );
        let resp_v6 = encode_binding_response(&tx, v6);
        let decoded_v6 = decode_message(&resp_v6).ok_or("expected decoded v6 response")?;
        assert_eq!(decoded_v6.mapped_address(), Some(v6));
        Ok(())
    }
}
