# Native dedicated server (`bfeserver`)

`bfeserver` (`rust/bfe-server`) is the standalone Rust dedicated server binary.
It drives the authoritative simulation (`bfe-sim`), content loaders
(`bfe-content` and `bfe-content-native`), and `BFE-UDP/1` transport
(`bfe-transport`) in a single native binary.

Build with `cargo build --manifest-path rust/Cargo.toml -p bfe-server --release`
and run `bfeserver --serve`, `bfeserver --admin '{"action":"status"}'`,
`bfeserver --version`, `bfeserver --contract` (the network contract as JSON, as
`/healthz` reports it), or `bfeserver` for the interactive setup and
administration menu. `bfeserver --help` prints every flag and environment
variable; an unknown flag prints the same text and exits with status 2.

## State directory and configuration

`--dir <state dir>` (or `BFE_STATE_DIR`) selects the server state directory
(defaulting to `$XDG_DATA_HOME/bfeserver` or `$HOME/.local/share/bfeserver`
with mode `0700`). When `--serve` cannot host a match it says why on stderr
and in the `error` field of `/healthz`: status `setup` while no game data is
configured, `fault` when `config.json` cannot be read or admission fails.
`bfeserver` reads and writes `config.json` with the following keys and defaults:

| Key | Default | Description |
| --- | --- | --- |
| `name` | `"Battlefront Eternal server"` | Public server title (up to 80 characters) |
| `engine` | `""` | Accepted for compatibility with existing configs and ignored |
| `data` | `""` | Path to prepared game content (`BFE_DATA_DIR` overrides) |
| `rotation` | `["swbf1/nab2:rep:conquest"]` | Map rotation entries (`swbf1/<map>:<side>:<mode>`) |
| `master` | `"https://online.battlefronteternal.com"` | Online directory master URL (`BFE_MASTER_URL` overrides) |
| `onlineNoticeAccepted` | `false` | Enables master registration and global ban fetches (`BFE_ACCEPT_ONLINE=1` overrides) |
| `relayPolicy` | `"auto"` | `"auto"`, `"never"`, or `"always"` (`BFE_RELAY_POLICY` overrides) |
| `honorGlobalBans` | `true` | Consults the master's signed ban list during admission |
| `maxPlayers` | `32` | Maximum concurrent connected players (`1..=64`) |
| `maxPlayersPerAddress` | `4` | Maximum concurrent sessions per IPv4 address or IPv6 `/64` prefix |
| `stateBytesPerSecond` | `100000` | Per-peer outgoing state budget in bytes/s (`16384..=1048576`) |
| `autoSpawn` | `false` | Spawns newly joined players immediately |
| `autoAssignTeams` | `false` | Disables manual team selection on join |
| `ai` | `"enhanced"` | Bots: `"enhanced"` (the Enhanced AI), `"classic"` (the original game's AI) or `"off"` (no bots). A new config gets `"enhanced"`; a `config.json` written by an older server has no `ai` key and keeps the Classic bots it had. The setup menu sets it (`a  Bots`) |
| `gamePort` | `18451` | UDP game port and TCP `/info` port (`BFE_GAME_PORT` overrides) |
| `bind` | `null` | Optional bind address for the UDP game sockets (`BFE_BIND` overrides) |
| `discovery` | `true` | Enables the `BFE-LAN/1` UDP discovery responder |
| `discoveryPort` | `18452` | UDP port for `BFE-LAN/1` discovery (`BFE_DISCOVERY_PORT` overrides) |
| `stun` | `["stun.cloudflare.com:3478"]` | STUN servers for reflexive address discovery |
| `rotationDelayMs` | `8000` | Delay before advancing to the next rotation entry after victory |
| `description`, `sponsor`, `location` | omitted | Sanitized metadata for the server list |

## Identity and global bans

`identity.json` stores the server's long-term Ed25519 keypair: `publicKey` is
base64 SPKI DER (`44` bytes) and `privateKey` is PKCS#8 PEM (`48` bytes). The
server ID is the lowercase SHA-256 hex digest of `publicKey`. Both `BFE-UDP/1`
handshakes and online master requests sign with this key.

When `honorGlobalBans` is enabled, `BanCache` fetches `/v1/info` and `/v1/bans`
from the master, pins the master's Ed25519 public key on first use in
`master-ban-key` (`0600`), verifies the Ed25519 signature over the canonical
JSON payload (`bans`, `issued`, `expires`), and rejects expired or tampered ban
lists.

## Game port, LAN discovery, and online directory

The game port (default `18451`) runs both the dual-stack `BFE-UDP/1` listener
(`bfe_transport::server::ServerEndpoint`) and the public HTTP `GET /info`
listener.

- **Admission and sessions**: `ServerEndpoint` validates `INITIATION` and
  `CONFIRM` handshakes, issues stateless HMAC-SHA256 `RETRY` cookies under load,
  enforces per-prefix handshake rate limits and session caps, and hands admitted
  peers to `GameSession` (`reconnectGraceMs = 15000`).
- **State lane policing**: `UdpPeerState` validates incoming `BNI1` (entity
  input) and `BNA1` (snapshot ack keepalive) datagrams (`10..=1100` bytes),
  tracks congestion hysteresis (`CONGESTED_MS = 120`, `CLEAR_MS = 50` above
  `32768` bytes backlog), drops state publications older than `100` ms, and
  polices outgoing `BNS1`/`BNO1` datagrams with a token bucket at
  `stateBytesPerSecond` (`burst = max(4096, rate * 0.25)`). Reliable `events`
  messages are bounded to `262144` bytes so full mission `entity-registry`
  descriptors transmit without truncation.
- **LAN discovery**: `LanDiscoveryResponder` listens on UDP `discoveryPort`
  (default `18452`) over IPv4 broadcast and IPv6 link-local multicast
  (`ff02::bf:1`), rate-limits per-address probes to one every `250` ms, and
  replies with sanitized server metadata (`description` and `sponsor` stripped).
- **Online master and signaling**: `OnlineServer` probes reflexive UDP endpoints
  via STUN (`rfc8489`), registers `/v1/servers` (`POST`/`PUT`/`DELETE`) with
  signed canonical envelopes (`BFE-MASTER-V1`), refreshes heartbeats every `20`
  seconds across IPv4 and IPv6 address families, and connects to `/v1/signal`
  over WebSocket (`tungstenite` + `rustls`) to answer `offer`, `candidate`, and
  `matchProbe` messages and schedule UDP hole-punch bursts (`0, 40, 100, 200,
  400, 800, 1500` ms).

## Authoritative world host

`NativeWorldHost` loads mission content through `bfe_content` and
`bfe_content_native`, computes the canonical content fingerprint, builds the
`entity-registry` descriptor from `bfe_content::defs::Defs`, and steps
`bfe_sim::abi::context::Context` at `60` Hz (`world_step_init`, `world_step`,
`world_publish_into`, `run_operation_into`, `read_bank`, `players_*`). Each tick
decodes `WirePublicationOutput` into shared `BNS1` entity snapshot groups,
per-client `BNO1` own-state datagrams, `input-ack`, `entity-edge-results`, and
`gameplay-events`, and fans them out to connected peers without steady-state
heap allocations on the transport packet path.

## Operator HTTP and SQLite persistence

`OperatorHttpServer` binds loopback TCP port `PORT` (default `18450`):

- `GET /healthz`: unauthenticated JSON readiness probe reporting `ok`,
  `service` (`"bfeserver"`), `status` (`"setup"`, `"loading"`, `"running"`, or
  `"fault"`), `multiplayerReady`, `tick`, `error`, and `networkContract`.
  `ok` is false only in a fault.
- `POST /admin`: bearer-authenticated (`operator-token`, mode `0600`) JSON
  actions (`status`, `kick`, `ban`, `unban`, `stats`, `message`, `restart`,
  `stop`) backed by `operator.sqlite` (`bans`, `scores`, and `audit` tables in
  WAL mode).
