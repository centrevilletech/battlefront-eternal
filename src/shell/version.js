import { NETWORK_CONTRACT } from "../runtime/network-contract.js";

// The main menu's version line, bottom right: this build (the desktop
// client's preview number, else a web build), the simulation it runs
// (servers refuse another), and the generations of the game data in use
// (maps, sides and common, tools/content-version.json), with the ones this
// build wants where they differ. about: the desktop's bfeDesktop.about()
// ({ version, wanted, prepared }), or null.
export function versionLine(about, simulation = NETWORK_CONTRACT.simulation) {
  const parts = [];
  const preview = /^\d+\.\d+\.\d+[-~]preview\.(\d+)$/.exec(about?.version || "");
  parts.push(preview ? `PREVIEW ${preview[1]}` : about?.version ? `VERSION ${about.version}`.toUpperCase() : "WEB BUILD");
  const sim = /(\d+)$/.exec(simulation || "");
  parts.push(`SIMULATION ${sim ? sim[1] : String(simulation || "?").toUpperCase()}`);
  const generations = (g) => `M${g?.maps ?? "?"} S${g?.sides ?? "?"} C${g?.common ?? "?"}`;
  if (about?.prepared) {
    const data = `GAME DATA ${generations(about.prepared)}`;
    parts.push(about.prepared.current === false && about.wanted ? `${data} (NEEDS ${generations(about.wanted)})` : data);
  } else if (about) parts.push("GAME DATA NOT PREPARED");
  return parts.join("   ");
}

// Asks the desktop once; text() is the line as known so far.
export function versionText(desktop = globalThis.bfeDesktop) {
  let line = versionLine(null);
  if (typeof desktop?.about === "function") {
    desktop.about().then((about) => { if (about) line = versionLine(about); }, () => {});
  }
  return () => line;
}
