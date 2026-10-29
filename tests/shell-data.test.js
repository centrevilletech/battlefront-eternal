import test from "node:test";
import assert from "node:assert/strict";
import { FADES, loadShellData, readClip, readLoad, readMapLoad, readMaps, readMovies, readShellData, resolveUrl, textureFiles } from "../src/shell/data.js";

test("paths: shell.json's are relative to public/common/shell unless they start at the root", () => {
  assert.equal(resolveUrl("movies/main.webm"), "/common/shell/movies/main.webm");
  assert.equal(resolveUrl("./movies/main.webm"), "/common/shell/movies/main.webm");
  assert.equal(resolveUrl("/audio/shell/track1.ogg"), "/audio/shell/track1.ogg");
  assert.equal(resolveUrl(""), null);
  assert.equal(resolveUrl(null), null);
});

test("movies as tools/zero_shell.py writes them: the original's screen names, the port's ids for them too", () => {
  const movies = readMovies({
    ifs_start: { intro: null, loop: "movies/start_screen.webm", fadeIn: 0.1, fadeOut: 0.5 },
    ifs_main: { intro: "movies/main_trans.webm", loop: "movies/main.webm", fadeIn: 0, fadeOut: 0.5, introFadeIn: 0, introFadeOut: 0 },
    ifs_opt_contmain: { intro: "movies/coreship_in.webm", loop: "movies/coreship_screen.webm", fadeIn: 0.1, fadeOut: 0.5, introFadeIn: 0, introFadeOut: 0.2 },
    ifs_meta_load: { intro: null, loop: "movies/atat_screen.webm", fadeIn: 0.1, fadeOut: 0.5 },
    ifs_meta_main: { intro: null, loop: "movies/gc_screen.webm", fadeIn: 0.1, fadeOut: 0.5 },
  });
  assert.equal(movies.start.loop.url, "/common/shell/movies/start_screen.webm");
  assert.deepEqual(movies.main.intro, { url: "/common/shell/movies/main_trans.webm", fadeIn: 0, fadeOut: 0 });
  assert.equal(movies.options.intro.fadeOut, 0.2);
  assert.equal(movies.galaxy.loop.url, "/common/shell/movies/gc_screen.webm");
  assert.equal(movies.ifs_meta_load.loop.url, "/common/shell/movies/atat_screen.webm", "every entry under its own name");
  assert.deepEqual(movies.ifs_main, movies.main);
});

test("movies: by the port's screen ids or the original's names, with the movie config's fades where shell.json has none", () => {
  const movies = readMovies({
    ifs_main: { intro: "movies/main_trans.webm", loop: "movies/main.webm" },
    sp: { intro: "movies/atat_in.webm", loop: "movies/atat_screen.webm", fadeIn: 0.25, fadeOut: 0.75 },
    ifs_opt_top: { intro: { file: "movies/coreship_in.webm", fadeOut: 0.3 }, loop: { url: "/x/coreship.webm" } },
    ifs_start: { intro: null, loop: "movies/start_screen.webm" },
    unknown_screen: { loop: "movies/nothing.webm" },
  });
  assert.deepEqual(Object.keys(movies).filter((k) => !k.startsWith("ifs_") && k !== "unknown_screen").sort(), ["main", "options", "sp", "start"]);
  assert.deepEqual(movies.main.intro, { url: "/common/shell/movies/main_trans.webm", fadeIn: 0, fadeOut: 0 }, "the main menu's intro cuts into its loop");
  assert.deepEqual(movies.main.loop, { url: "/common/shell/movies/main.webm", fadeIn: 0, fadeOut: 0.5 });
  assert.deepEqual(movies.sp.intro, { url: "/common/shell/movies/atat_in.webm", fadeIn: FADES.intro[0], fadeOut: FADES.intro[1] });
  assert.deepEqual(movies.sp.loop, { url: "/common/shell/movies/atat_screen.webm", fadeIn: 0.25, fadeOut: 0.75 }, "an entry's own fades are its loop's");
  assert.deepEqual(movies.options.intro, { url: "/common/shell/movies/coreship_in.webm", fadeIn: 0, fadeOut: 0.3 });
  assert.deepEqual(movies.options.loop, { url: "/x/coreship.webm", fadeIn: 0.1, fadeOut: 0.5 });
  assert.equal(movies.start.intro, null);
  assert.equal(readClip(null, FADES.loop), null);
});

test("maps: names, extraction, eras, the flyover's 1 s fades, and the loading screen's pictures", () => {
  const strings = { "planets.geonosis.name": "GEONOSIS", "planets.geonosis.mapname1": "GEONOSIS: SPIRE" };
  const [geo1, nab1] = readMaps([
    { id: "GEO1", nameKey: "planets.geonosis.mapname1", extracted: true, eras: { cw: { teams: ["rep", "cis"], attacker: 1 } },
      fly: "movies/geo1fly.webm",
      load: { planet: "planets.geonosis.name", stages: [
        { image: "load/geo1/galaxy.png", target: [390, 737, 37, 43] },
        { image: "load/geo1/locale.png", target: { x: 360, y: 555, w: 38, h: 37 } },
        { image: "load/geo1/landsat.png", target: [360, 555, 38, 37] },
        { image: "load/geo1/level.png", target: [0, 0, 0, 0] },
      ] } },
    { id: "nab1" },
  ], strings);
  assert.equal(geo1.id, "geo1");
  assert.equal(geo1.name, "GEONOSIS: SPIRE");
  assert.equal(geo1.extracted, true);
  assert.deepEqual(geo1.eras.cw, { teams: ["rep", "cis"], attacker: 1 });
  assert.deepEqual(geo1.fly, { url: "/common/shell/movies/geo1fly.webm", fadeIn: 1, fadeOut: 1 });
  assert.equal(geo1.load.planet, "GEONOSIS");
  assert.deepEqual(geo1.load.stages.map((s) => s.target), [[390, 737, 37, 43], [360, 555, 38, 37], [360, 555, 38, 37], [0, 0, 0, 0]]);
  assert.equal(geo1.load.stages[0].url, "/common/shell/load/geo1/galaxy.png");
  assert.equal(nab1.name, "NAB1");
  assert.equal(nab1.extracted, null, "not said: not known");
  assert.equal(nab1.fly, null);
  assert.equal(nab1.load, null);
  assert.equal(readMapLoad({ planet: "RHEN VAR", stages: ["a.png"] }).planet, "RHEN VAR");
  assert.equal(readMapLoad({ planetKey: "planets.x.name", name: "X", stages: [] }, {}).planet, "X");
  assert.deepEqual(readMaps({ kam1: { name: "KAMINO: TIPOCA CITY" } }).map((m) => [m.id, m.name]), [["kam1", "KAMINO: TIPOCA CITY"]], "an object of maps");
});

test("the loading screen's art and timing: load.lvl's 3.5 s of tracking and 1 s zooms by default, no hold", () => {
  assert.deepEqual(readLoad(undefined), {
    backdrop: null, disk: null, leds: [], edges: { horz: null, vert: null, corner: null }, text: "common.loading", track: 3.5, zoom: 1, hold: 0,
  });
  // As tools/zero_shell.py writes it: common.json texture names, the
  // crosshair's, the timing pair as targetTime, LOADING's string key.
  const file = textureFiles({ textures: { load_rotating_disk: "textures/load_rotating_disk.png", load_led_0: "textures/load_led_0.png",
    load_edge_horz: "textures/load_edge_horz.png" } }, { textures: { "textures/load_rotating_disk.png": { file: "load_rotating_disk-1-4x.webp" } } });
  const named = readLoad({ backdrop: "load/load_simple_backdrop.png", disk: "load_rotating_disk", leds: ["load_led_0", "load_led_9"],
    crosshair: { horizontal: "load_edge_horz", vertical: "load_edge_vert" }, text: "common.loading", targetTime: [3.5, 1] }, file);
  assert.equal(named.backdrop, "/common/shell/load/load_simple_backdrop.png");
  assert.equal(named.disk, "/common/raster-hud/load_rotating_disk-1-4x.webp", "its 4x upscale");
  assert.deepEqual(named.leds, ["/common/textures/load_led_0.png"], "a texture common.json lacks is left out");
  assert.equal(named.edges.horz, "/common/textures/load_edge_horz.png");
  assert.equal(named.edges.vert, null);
  assert.equal(named.track, 3.5);
  const load = readLoad({ disk: "load/load_rotating_disk.png", leds: ["load/led0.png", "load/led1.png", "load/led2.png"], timing: [4, 1.5],
    edges: { horz: "load/h.png", vert: "load/v.png" } });
  assert.equal(load.disk, "/common/shell/load/load_rotating_disk.png");
  assert.equal(load.leds.length, 3);
  assert.equal(load.track, 4);
  assert.equal(load.zoom, 1.5);
  assert.equal(load.hold, 0, "the sequence ends with the last zoom unless the timing says");
  assert.equal(readLoad({ timing: [4, 1.5, 2] }).hold, 2);
  assert.equal(load.edges.vert, "/common/shell/load/v.png");
  assert.equal(readLoad({ timing: { stage: 3, zoom: 0.5 } }).zoom, 0.5);
  assert.equal(readLoad({ timing: { track: 3, zoom: 0.5 } }).track, 3);
});

test("all of it, from nothing at all: the front end still has its fonts and strings", async () => {
  const common = {
    strings: { "common.back": "BACK" },
    fonts: { gamefont_medium: { lineHeight: 23, pages: ["gamefont_medium_tex0"], glyphs: [] } },
    textures: { gamefont_medium_tex0: "textures/gamefont_medium_tex0.png" },
  };
  const raster = { textures: { "textures/gamefont_medium_tex0.png": { file: "gamefont_medium_tex0-abc-4x.webp" } } };
  const data = await loadShellData({ fetchJson: async (url) => (url.endsWith("common.json") ? common : url.endsWith("manifest.json") ? raster : null) });
  assert.equal(data.missing, true);
  assert.deepEqual(data.movies, {});
  assert.deepEqual(data.maps, []);
  assert.deepEqual(data.music, []);
  assert.deepEqual(data.whooshes, []);
  assert.equal(data.strings["common.back"], "BACK");
  assert.equal(data.pages.gamefont_medium_tex0, "/common/raster-hud/gamefont_medium_tex0-abc-4x.webp", "the 4x page");
  assert.equal(readShellData(null, { ...common, textures: { gamefont_medium_tex0: "textures/x.png" } }, null).pages.gamefont_medium_tex0,
    "/common/textures/x.png", "no upscale: the original page");
});

test("music, whooshes and sounds: paths or objects", () => {
  const data = readShellData({
    music: [{ name: "shell_soundtrack_1", file: "/audio/shell/m1.ogg" }, "/audio/shell/m2.ogg", { name: "bad" }],
    whooshes: ["/audio/shell/w1.ogg", { file: "/audio/shell/w2.ogg" }, null],
    sounds: { shell_select_change: { file: "/audio/shell/select.ogg", pitch: 1.5 }, shell_menu_exit: "/audio/shell/exit.ogg" },
    strings: { "common.loading": "LOADING" },
  });
  assert.deepEqual(data.music.map((t) => [t.name, t.url]), [["shell_soundtrack_1", "/audio/shell/m1.ogg"], ["track2", "/audio/shell/m2.ogg"]]);
  assert.equal(data.music[0].properties, null);
  assert.deepEqual(readShellData({ music: [{ name: "gc", file: "/a.opus", properties: ["shell_galacticconquest"] }] }).music[0].properties,
    ["shell_galacticconquest"]);
  assert.deepEqual(data.whooshes, ["/audio/shell/w1.ogg", "/audio/shell/w2.ogg"]);
  assert.deepEqual(data.sounds.shell_select_change, { url: "/audio/shell/select.ogg", gain: 1, pitch: 1.5 });
  assert.deepEqual(data.sounds.shell_menu_exit, { url: "/audio/shell/exit.ogg", gain: 1, pitch: 1 });
  assert.equal(data.strings["common.loading"], "LOADING", "shell.json's strings join common.json's");
  assert.equal(data.missing, false);
});
