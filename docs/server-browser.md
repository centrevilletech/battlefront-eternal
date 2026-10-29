# Server browser data

What the desktop gives a server browser: `bfeDesktop.network.request("servers")`
returns `{ servers: [...] }`, the master's listing (`GET /v1/servers`), each
server with a ping the player's own machine measured. A listing is renewed
by its server's heartbeat every 20 seconds and drops 60 seconds after the
last one.

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
| `query` | The battle a player's page loads to join (map, side, bots, seed, match token only) | The battle |
| `simulation`, `protocol`, `content` | The release it runs and its game data fingerprint | The engine |
| `relayPolicy`, `endpoints` | `never` lists the game port at each address it heartbeats from (IPv4, IPv6); others list none | The master |
| `latencyMs` | The player's ping: a TCP handshake with the game port, the lowest over its addresses, null for a server that lists none or does not answer within a second | The desktop (`electron/network.js`) |

Fields a server leaves out, or that fail the master's checks, are null; a
listing from an older server has none of the newer ones. A server's text is
the operator's: show it as text, never as markup.

The page cannot load remote images (its Content-Security-Policy allows only
`'self'`, `data:` and `blob:`), so a sponsor's logo has to come through the
desktop's main process (fetched there, its size and type checked, handed
over as a `data:` URL), which also keeps the logo's host from seeing every
player who opens the browser. Open sponsor links in the system browser
(`window.open` of an https address does).

The front end's browser (MULTIPLAYER under the page theme,
`src/shell/server-browser.js` and `server-browser-view.js`, docs/shell.md)
shows these. It asks for a sponsor's logo with
`bfeDesktop.network.request("logo", url)`; the GUI preview answers it, the
desktop does not yet (the page then shows the sponsor's name and site).
Fetching logos from the player's machine would let each sponsor's host see
the players who look at its server; a copy kept by the master would not.

Joining uses `query` (`src/shell/screens.js`, only the battle's own keys,
`src/runtime/battle-query.js`); a page that still built another battle loads
the server's from its welcome (`src/runtime/network-session.js`).

## For operators

In the server's `config.json` (battlefront-eternal-server README):

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
