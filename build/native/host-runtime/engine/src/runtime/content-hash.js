// The updater checks every battle of a server's rotation here before it moves the server to a
// new release. bfeserver fingerprints a battle's files itself when it starts it (a failure is a
// fault on /healthz, and the updater moves the server back); this checks the map is prepared.
export async function contentHash(query, fetcher) {
  const map = new URLSearchParams(query).get("map") || "nab2";
  if (!/^[a-z0-9_-]{1,64}$/.test(map)) throw Error(`Invalid map ${JSON.stringify(map)}`);
  const response = await fetcher(`maps/${map}/map.json`);
  if (!response.ok) throw Error(`The prepared game data has no ${map}`);
  return map;
}
