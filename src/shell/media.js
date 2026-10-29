import { MoviePlayer } from "./movies.js";
import { ShellAudio, SOUNDTRACK } from "./audio.js";

// Each screen's movie and music, and the moves between screens, as the
// original's ifs_movietrans makes them (docs/shell.md):
//
// - Forward into a screen with an intro: if the screen's music isn't the
//   one playing, shellmusic dips to 0 over a second; the intro starts (the
//   old movie fading out over it) with the next whoosh; when it ends,
//   shellmusic comes back over a second, the screen's music is set, and the
//   screen shows.
// - Forward into a screen without one: its loop, its music, at once.
// - Back: the screen below comes back with its loop, no intro, no whoosh.
//
// movie: the shell.json movie it plays (null: none; the movie stops); music:
// what it sets (null keeps what plays, as a screen with no music field).
export const SCREEN_MEDIA = {
  start: { movie: "start", music: SOUNDTRACK }, // the click gate, in the profile screen's place
  profile: { movie: "start", music: SOUNDTRACK },
  main: { movie: "main", music: SOUNDTRACK },
  sp: { movie: "sp", music: SOUNDTRACK },
  options: { movie: "options", music: null },
  briefing: { movie: "briefing", music: SOUNDTRACK },
  meta: { movie: "meta", music: null },
  galaxy: { movie: "galaxy", music: "shell_galacticconquest" },
  // Instant Action and the multiplayer screens have no movie of their own:
  // a still backdrop (and Instant Action's flyover, preview()).
  ia: { movie: null, music: null },
  mp: { movie: null, music: null },
};

// Instant Action's preview: the highlighted map's flyover in the lower right,
// 0.5 s after the highlight moves, in the rectangle the original works out
// for the screen (530 x 400 at 800 x 600, wider for a wider target aspect).
export const PREVIEW_DELAY = 0.5;
export function previewRect(width, height, targetAspect = 4 / 3) {
  const w = 530 * (1 + targetAspect - width / height) * (width / 800);
  const h = 400 * (height / 600);
  return [width - w, height - h, w, h];
}

export class ShellMedia {
  // movies and audio: a MoviePlayer and a ShellAudio, made here unless
  // given (checks give stand-ins).
  // screens: each screen's movie and music (SCREEN_MEDIA, or the front
  // end's own changes to it).
  constructor({ data, settings = null, parent = null, movies = null, audio = null, screens = SCREEN_MEDIA } = {}) {
    this.data = data;
    this.screens = screens;
    this.movies = movies || new MoviePlayer(parent || document.body);
    this.audio = audio || new ShellAudio({ data, settings });
    this.screen = null;
    this.moves = 0; // forward moves made, for checks
  }

  movieOf(id) {
    const name = this.screens[id]?.movie;
    return name ? this.data.movies[name] || null : null;
  }

  // Into screen `id`; back: returning to it. Resolves when the screen may
  // show: after its intro going forward, at once otherwise.
  async enter(id, { back = false } = {}) {
    const media = this.screens[id] || { movie: null, music: null };
    const movie = this.movieOf(id);
    const audio = this.audio;
    this.screen = id;
    if (back || !movie?.intro) {
      this.movies.play(movie, { intro: false });
      if (!back) audio.fadeBus("shellmusic", 1, 1);
      audio.music.set(media.music);
      return true;
    }
    this.moves += 1;
    if (!audio.music.playing(media.music)) audio.fadeBus("shellmusic", 0, 1);
    const { started, ended } = this.movies.play(movie);
    started.then((ok) => ok && this.screen === id && audio.whoosh());
    const ok = await ended;
    if (this.screen !== id) return false;
    audio.fadeBus("shellmusic", 1, 1);
    audio.music.set(media.music);
    return ok;
  }

  // Instant Action's flyover for a map (null: none), after the delay.
  preview(mapId) {
    const fly = this.data.maps.find((map) => map.id === mapId)?.fly || null;
    return this.movies.play(fly ? { intro: null, loop: fly } : null, { rect: previewRect, fit: "fill", delay: PREVIEW_DELAY });
  }

  update(dt) {
    this.audio.update(dt);
  }
}
