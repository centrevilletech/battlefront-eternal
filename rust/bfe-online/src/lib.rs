#![deny(warnings, clippy::all)]

// Native online front-end crate for Battlefront Eternal:
// - `identity`: Persistent Ed25519 player identity, master request signing,
//   and online notice acknowledgement storage.
// - `master`: Signed HTTPS client for `GET /v1/servers`,
//   `POST /v1/relay-credentials`, and `POST /v1/signal-ticket`.
// - `ping`: Concurrent TCP handshake latency probing (`with_latency`) and
//   direct `/info` queries (`direct_info`).
// - `lan`: IPv4 broadcast and IPv6 link-local multicast `BFE-LAN/1` discovery
//   on UDP port 18452.
// - `direct`: Direct Connect IPv4/IPv6 address and port validation.
// - `browser`: Pure server browser model, filtering, sorting, selection,
//   compatibility checking, battle query sanitization, and GAME INFO facts.

pub mod browser;
pub mod direct;
pub mod identity;
pub mod lan;
pub mod master;
pub mod ping;

pub use browser::{
    battle_params, battle_summary_params, clean_sponsor, compatible, country_of, describe_server,
    era_label, error_text, flag_of, map_name, mode_label, ping_quality, uptime_text, BattleSummary,
    BrowserKey, BrowserState, EndpointAddr, EraOption, FilterUpdate, Filters, GameInfo,
    MapCatalogEntry, PingFilter, PingQuality, RawSponsor, RefreshKind, RotationItem, ServerBrowser,
    ServerBrowserService, ServerContract, ServerEntry, SortColumn, SortSpec, Source, SponsorInfo,
    REFRESH_EVERY_MS, RETRY_EVERY_MS,
};
pub use direct::{
    parse_direct_target, parse_strict_ip, resolve_host_port, strip_host_brackets,
    validate_direct_endpoint, DirectConnectError, DirectTarget, DEFAULT_DIRECT_PORT,
};
pub use identity::{
    default_consent_path, default_identity_path, read_online_consent, try_read_online_consent,
    write_online_consent, write_online_consent_at, PlayerIdentity, SignedMasterHeaders,
    ED25519_SPKI_PREFIX, ONLINE_NOTICE_VERSION,
};
pub use lan::{
    default_lan_targets, discover_lan, encode_discover_packet, parse_server_packet,
    try_encode_discover_packet, LanDiscoveryOptions, DEFAULT_LAN_TIMEOUT, LAN_MAGIC,
    LAN_MULTICAST_V6, LAN_PORT, MAX_LAN_PACKET, MAX_LAN_SERVERS,
};
pub use master::{
    master_host, normalize_base_url, resolve_master_url, unreachable_error, MasterClient,
    MasterIpFamily, RelayCredentials, SignalTicket, DEFAULT_MASTER_URL, MASTER_RETRY_DELAY,
    MASTER_TIMEOUT,
};
pub use ping::{
    direct_info, fetch_server_info, measure_endpoint_latency, measure_latency,
    measure_server_latency, ping_host_port, with_latency, DIRECT_INFO_TIMEOUT, MAX_PING_ENDPOINTS,
    PING_CONCURRENCY, PING_TIMEOUT,
};
