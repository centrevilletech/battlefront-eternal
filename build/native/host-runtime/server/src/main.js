// Servers set up before the Rust server ran `node <root>/server/src/main.js`; their units and
// their updater (../../update) still start this file. It runs the bfeserver beside it with the
// same arguments and environment, passes stop signals on, and exits as it does.
import { spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const binary = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "bfeserver");
const child = spawn(binary, process.argv.slice(2), { stdio: "inherit" });
for (const signal of ["SIGTERM", "SIGINT", "SIGHUP"]) process.on(signal, () => child.kill(signal));
child.on("error", (error) => {
  console.error(`Cannot start ${binary}: ${error.message}`);
  process.exit(1);
});
child.on("exit", (code, signal) => process.exit(code ?? (signal ? 1 : 0)));
