// The updater (../../../update/server.js) waits for a new server to report this contract on
// /healthz. bfeserver prints its own, so the two always agree.
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const binary = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "server", "bfeserver");
export const NETWORK_CONTRACT = Object.freeze(JSON.parse(execFileSync(binary, ["--contract"], { encoding: "utf8" })));
