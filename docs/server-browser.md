# Server browser data

`bfe-online::ServerBrowser` (`rust/bfe-online`) queries the master server
directory (`GET /v1/servers`) and LAN discovery (`BFE-LAN/1` on UDP port
`18452`), measuring each listed server's ping from the player's own machine. A
listing is renewed by its server's heartbeat every 20 seconds and drops 60
seconds after the last one.

| Field | What | From |
| --- | --- | --- |
| `id`, `publicKey`, `fingerprint` | The server's identity | Its key |
| `name` | Server name (80 characters) | `config.json` `name` |
| `description` | Free text (280 characters), or null | `config.json` `description` |
| `sponsor` | `{ name, url, logo }` or null; `url` and `logo` are https addresses, or null | `config.json` `sponsor` |
| `location` | Free text (80), such as "Chicago, US", or null | `config.json` `location` |
| `players`, `maxPlayers` | People on it now, and room (64: 32 a side) | The game port |
| `battle` | `{ map, mapName, side, era, mode }`: the battle playing now, e.g. `bes2`, `BESPIN: CLOUD CITY`, `rep`, `cw`, `conquest` | Its rotation; the name from the prepared catalog |
| `rotation` | The playlist, the current battle first (`swbf1/<map>:<side>:<mode>`) | `config.json` `rotation` |
| `bots` | Whether bots fill the teams (`ai: "off"` fields none), or null | `config.json` `ai` |
| `autoAssignTeams` | Whether the server assigns teams (no team switch), or null | `config.json` |
| `uptime` | Seconds since the service started (it restarts for each update), or null | The service |
| `query` | The battle parameters needed to load and join (map, side, bots, seed, match token only) | The battle |
| `simulation`, `protocol`, `content` | The release it runs and its game data fingerprint | The engine |
| `relayPolicy`, `endpoints` | `never` lists the game port at each address it heartbeats from (IPv4, IPv6); others list none | The master |
| `latencyMs` | The player's ping: a TCP handshake with the game port, the lowest over its addresses, null for a server that lists none or does not answer within a second | `rust/bfe-online` |

Fields a server leaves out, or that fail the master's checks, are null; a
listing from an older server has none of the newer ones. A server's text is
the operator's: show it as plain text.

The front end's server browser (`rust/bfe-shell` and `rust/bfe-client`,
[shell.md](shell.md)) displays the server list, filters, sort order and
connection details. Sponsor links open in the system browser only after
validating an `https:` URL. Fetching logos directly from third-party hosts is
disabled so sponsor hosts cannot track players browsing the server list.

## For operators

In the server's `config.json` ([native-server.md](native-server.md)):

```json
{
  "name": "Example Server | Duels",
  "description": "Bespin: Cloud City only, Clone Wars then Galactic Civil War. No bots; teams are assigned.",
  "location": "Chicago, US",
  "sponsor": {
    "name": "Example Sponsor",
    "url": "https://example.com/",
    "logo": "https://example.com/images/logo.png"
  }
}
```

The server reads it when it starts (an update restarts it).
