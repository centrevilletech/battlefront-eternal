import saira from "./fonts/saira-latin.woff2?url";

// The page theme's styles (index.js). The looks are custom properties on
// <html data-bfe-theme>, so the front end's dialogs, outside the page, take
// them too; the shapes each look changes (buttons, titles, tabs) follow
// its block of properties. One layout for all three: only colours and
// surfaces differ.

const LOOKS = `
html[data-bfe-theme="eternal"] {
  --bfe-backdrop: #030406;
  --bfe-movie-filter: saturate(1.05) contrast(1.06) brightness(.92);
  --bfe-side-wash: linear-gradient(180deg, rgba(0, 0, 0, .35) 0%, rgba(0, 0, 0, .05) 35%, rgba(0, 0, 0, .1) 65%, rgba(0, 0, 0, .55) 100%);
  --bfe-side-blend: normal;
  --bfe-scrim: radial-gradient(ellipse 30% 78% at 50% 50%, rgba(3, 4, 7, .93) 0%, rgba(3, 4, 7, .78) 50%, rgba(3, 4, 7, 0) 100%);
  --bfe-dim: rgba(2, 3, 6, .5);
  --bfe-pattern: repeating-linear-gradient(180deg, rgba(255, 255, 255, .035) 0 1px, transparent 1px 4px);
  --bfe-pattern-opacity: 1;
  --bfe-logo-face: #d7dde6;
  --bfe-logo-depth: 0 1px 0 #8a929e, 0 2px 0 #77808c, 0 3px 0 #646d7a, 0 4px 0 #535b67, 0 5px 0 #434a55, 0 6px 0 #353b45, 0 7px 0 #292e36, 0 8px 0 #1e222a, 0 16px 26px rgba(0, 0, 0, .6);
  --bfe-logo-fill: linear-gradient(180deg, #ffffff 0%, #f1f4f8 40%, #b9c2cf 72%, #8e98a7 100%);
  --bfe-tag-ink: #1a1004;
  --bfe-tag-fill: linear-gradient(180deg, #ffd27a 0%, #ffa42a 42%, #f57500 100%);
  --bfe-gap: clamp(8px, 1.2vh, 14px);
  --bfe-text: #e8ecf2;
  --bfe-text-dim: rgba(232, 236, 242, .66);
  --bfe-text-faint: rgba(232, 236, 242, .4);
  --bfe-accent: #ff9a1f;
  --bfe-accent-ink: #170d02;
  --bfe-accent-fill: linear-gradient(90deg, #ffbe55 0%, #ff9419 60%, #f57500 100%);
  --bfe-value-focus: #ffb347;
  --bfe-note: #ffb347;
  --bfe-panel-bg: linear-gradient(180deg, rgba(14, 17, 24, .88), rgba(8, 10, 15, .92));
  --bfe-panel-border: 1px solid rgba(255, 255, 255, .12);
  --bfe-panel-radius: 0;
  --bfe-panel-clip: polygon(18px 0, 100% 0, 100% calc(100% - 18px), calc(100% - 18px) 100%, 0 100%, 0 18px);
  --bfe-panel-shadow: 0 24px 70px rgba(0, 0, 0, .55);
  --bfe-row-line: rgba(255, 255, 255, .06);
  --bfe-row-focus: linear-gradient(90deg, rgba(255, 154, 31, .2), rgba(255, 154, 31, .03));
  --bfe-track: rgba(255, 255, 255, .14);
  --bfe-track-radius: 0;
  --bfe-thumb: #ffffff;
  --bfe-thumb-border: 0 solid transparent;
  --bfe-thumb-radius: 1px;
  --bfe-list-bg: rgba(8, 10, 15, .6);
  --bfe-list-hover: rgba(255, 255, 255, .06);
  --bfe-list-selected: rgba(255, 154, 31, .2);
  --bfe-tab-line: rgba(255, 255, 255, .12);
  --bfe-tab-bg: transparent;
  --bfe-tab-active-bg: linear-gradient(90deg, #ffbe55 0%, #ff9419 60%, #f57500 100%);
  --bfe-tab-active-ink: #170d02;
  --bfe-preview-border: 1px solid rgba(255, 255, 255, .18);
  --bfe-dialog-bg: #0c0f15;
  --bfe-dialog-button-bg: rgba(255, 255, 255, .06);
  --bfe-dialog-button-border: rgba(255, 255, 255, .22);
  --bfe-dialog-button-radius: 0;
  --bfe-scroll: rgba(255, 154, 31, .45);
  --bfe-overlay: rgba(3, 4, 7, .52);
}

html[data-bfe-theme="bf1"] {
  --bfe-backdrop: radial-gradient(ellipse at 50% 45%, #0c2858 0%, #061531 48%, #01040c 100%);
  --bfe-movie-filter: saturate(.9) contrast(1.05) brightness(.95);
  --bfe-side-wash: linear-gradient(180deg, rgba(40, 110, 230, .35), rgba(20, 60, 150, .3));
  --bfe-side-blend: soft-light;
  --bfe-scrim: radial-gradient(ellipse 30% 78% at 50% 50%, rgba(6, 26, 70, .9) 0%, rgba(4, 18, 52, .74) 50%, rgba(4, 18, 52, 0) 100%);
  --bfe-dim: rgba(2, 10, 30, .5);
  --bfe-pattern: linear-gradient(rgba(150, 205, 255, .16) 1px, transparent 1px) 0 0 / 46px 46px, linear-gradient(90deg, rgba(150, 205, 255, .16) 1px, transparent 1px) 0 0 / 46px 46px;
  --bfe-pattern-opacity: .55;
  --bfe-logo-face: #cfe4ff;
  --bfe-logo-depth: 0 1px 0 #6f97d8, 0 2px 0 #5d86cc, 0 3px 0 #4c75bf, 0 4px 0 #3c64b0, 0 5px 0 #2f55a0, 0 6px 0 #24478f, 0 7px 0 #1a3a7d, 0 8px 0 #122f6b, 0 16px 26px rgba(2, 10, 44, .6);
  --bfe-logo-fill: linear-gradient(180deg, #ffffff 0%, #e2f0ff 38%, #9fc6f5 70%, #6f9ee3 100%);
  --bfe-tag-ink: #13204a;
  --bfe-tag-fill: linear-gradient(180deg, #ffd27a 0%, #ffa42a 42%, #f57500 100%);
  --bfe-gap: clamp(7px, 1vh, 12px);
  --bfe-text: #dcefff;
  --bfe-text-dim: rgba(205, 228, 255, .72);
  --bfe-text-faint: rgba(205, 228, 255, .44);
  --bfe-accent: #ffa42a;
  --bfe-accent-ink: #13204a;
  --bfe-accent-fill: linear-gradient(180deg, #ffd27a 0%, #ffa42a 45%, #f57500 100%);
  --bfe-value-focus: #ffe04a;
  --bfe-note: #dfdf20;
  --bfe-panel-bg: linear-gradient(180deg, rgba(20, 60, 140, .55), rgba(6, 22, 60, .8));
  --bfe-panel-border: 1px solid rgba(140, 200, 255, .5);
  --bfe-panel-radius: 4px;
  --bfe-panel-clip: none;
  --bfe-panel-shadow: inset 0 0 40px rgba(60, 140, 255, .18), 0 20px 60px rgba(0, 0, 0, .5);
  --bfe-row-line: rgba(140, 200, 255, .12);
  --bfe-row-focus: linear-gradient(90deg, rgba(90, 170, 255, .32), rgba(90, 170, 255, .06));
  --bfe-track: rgba(140, 200, 255, .22);
  --bfe-track-radius: 4px;
  --bfe-thumb: #dff0ff;
  --bfe-thumb-border: 1px solid #7fb6ff;
  --bfe-thumb-radius: 3px;
  --bfe-list-bg: rgba(6, 22, 60, .55);
  --bfe-list-hover: rgba(120, 190, 255, .14);
  --bfe-list-selected: rgba(120, 190, 255, .3);
  --bfe-tab-line: rgba(140, 200, 255, .45);
  --bfe-tab-bg: rgba(20, 60, 140, .45);
  --bfe-tab-active-bg: linear-gradient(180deg, #ffd27a 0%, #ffa42a 45%, #f57500 100%);
  --bfe-tab-active-ink: #13204a;
  --bfe-preview-border: 1px solid rgba(140, 200, 255, .6);
  --bfe-dialog-bg: #0a1a3a;
  --bfe-dialog-button-bg: rgba(60, 130, 240, .3);
  --bfe-dialog-button-border: rgba(140, 200, 255, .55);
  --bfe-dialog-button-radius: 3px;
  --bfe-scroll: rgba(140, 200, 255, .5);
  --bfe-overlay: rgba(4, 16, 46, .5);
}

html[data-bfe-theme="bf2"] {
  --bfe-backdrop: radial-gradient(ellipse at 50% 45%, #1d3f73 0%, #10284f 50%, #050c1a 100%);
  --bfe-movie-filter: saturate(.85) contrast(1.08) brightness(.9);
  --bfe-side-wash: linear-gradient(180deg, rgba(0, 0, 0, .3), rgba(0, 0, 0, .05) 40%, rgba(0, 0, 0, .45));
  --bfe-side-blend: normal;
  --bfe-scrim: radial-gradient(ellipse 30% 78% at 50% 50%, rgba(16, 38, 74, .92) 0%, rgba(12, 30, 60, .76) 50%, rgba(12, 30, 60, 0) 100%);
  --bfe-dim: rgba(6, 14, 30, .5);
  --bfe-pattern: linear-gradient(rgba(190, 215, 255, .2) 1px, transparent 1px) 0 0 / 160px 160px, linear-gradient(90deg, rgba(190, 215, 255, .2) 1px, transparent 1px) 0 0 / 160px 160px,
    linear-gradient(rgba(190, 215, 255, .09) 1px, transparent 1px) 0 0 / 32px 32px, linear-gradient(90deg, rgba(190, 215, 255, .09) 1px, transparent 1px) 0 0 / 32px 32px;
  --bfe-pattern-opacity: .8;
  --bfe-logo-face: #e9edf2;
  --bfe-logo-depth: 0 1px 0 #9aa3ad, 0 2px 0 #86909b, 0 3px 0 #737d89, 0 4px 0 #616b77, 0 5px 0 #505965, 0 6px 0 #404853, 0 7px 0 #313842, 0 8px 0 #232931, 0 16px 26px rgba(0, 0, 0, .6);
  --bfe-logo-fill: linear-gradient(180deg, #ffffff 0%, #e4e8ec 30%, #a9b1ba 52%, #d9dee3 74%, #ffffff 100%);
  --bfe-tag-ink: #ffffff;
  --bfe-tag-fill: linear-gradient(180deg, #f0554a 0%, #d2281c 50%, #94150c 100%);
  --bfe-gap: clamp(8px, 1.3vh, 15px);
  --bfe-text: #ffffff;
  --bfe-text-dim: rgba(225, 232, 242, .76);
  --bfe-text-faint: rgba(225, 232, 242, .46);
  --bfe-accent: #e0362a;
  --bfe-accent-ink: #ffffff;
  --bfe-accent-fill: linear-gradient(180deg, #f0675a 0%, #cf2a1d 48%, #a5180e 52%, #d8483a 100%);
  --bfe-value-focus: #ffd75a;
  --bfe-note: #ffd75a;
  --bfe-panel-bg: linear-gradient(180deg, rgba(22, 46, 86, .84), rgba(10, 24, 48, .92));
  --bfe-panel-border: 2px solid rgba(190, 205, 225, .45);
  --bfe-panel-radius: 14px;
  --bfe-panel-clip: none;
  --bfe-panel-shadow: inset 0 1px 0 rgba(255, 255, 255, .15), 0 20px 60px rgba(0, 0, 0, .55);
  --bfe-row-line: rgba(190, 215, 255, .1);
  --bfe-row-focus: linear-gradient(90deg, rgba(224, 54, 42, .38), rgba(224, 54, 42, .05));
  --bfe-track: linear-gradient(180deg, #4b535c, #858d96);
  --bfe-track-radius: 6px;
  --bfe-thumb: linear-gradient(180deg, #ffffff, #b9c0c8);
  --bfe-thumb-border: 1px solid #23272d;
  --bfe-thumb-radius: 9px;
  --bfe-list-bg: rgba(8, 20, 42, .6);
  --bfe-list-hover: rgba(255, 255, 255, .08);
  --bfe-list-selected: rgba(255, 255, 255, .18);
  --bfe-tab-line: #b31d12;
  --bfe-tab-bg: linear-gradient(180deg, #3b4148, #1f2328);
  --bfe-tab-active-bg: linear-gradient(180deg, #e24a3d, #a5180e);
  --bfe-tab-active-ink: #ffffff;
  --bfe-preview-border: 2px solid #9aa3ad;
  --bfe-dialog-bg: #10223f;
  --bfe-dialog-button-bg: rgba(255, 255, 255, .08);
  --bfe-dialog-button-border: rgba(190, 205, 225, .5);
  --bfe-dialog-button-radius: 999px;
  --bfe-scroll: rgba(205, 214, 226, .55);
  --bfe-overlay: rgba(8, 20, 42, .5);
}
`;

// The buttons, titles and tabs in each look's own shape.
const SHAPES = `
html[data-bfe-theme="eternal"] .bfe-btn { color: #e8ecf2; font-weight: 700; letter-spacing: .24em;
  border: 1px solid rgba(255, 255, 255, .13); background: linear-gradient(90deg, rgba(20, 24, 32, .88), rgba(20, 24, 32, .6));
  clip-path: polygon(14px 0, 100% 0, 100% calc(100% - 14px), calc(100% - 14px) 100%, 0 100%, 0 14px); }
html[data-bfe-theme="eternal"] .bfe-btn::before { content: ""; position: absolute; left: 0; top: 0; bottom: 0; width: 3px; background: #ff9a1f; opacity: .55; transition: opacity .14s ease; }
html[data-bfe-theme="eternal"] .bfe-btn.active { color: #170d02; border-color: #ffc46b;
  background: linear-gradient(90deg, #ffbe55 0%, #ff9419 60%, #f57500 100%); box-shadow: 0 0 28px rgba(255, 150, 30, .32); }
html[data-bfe-theme="eternal"] .bfe-btn.active::before { opacity: 0; }
html[data-bfe-theme="eternal"] .bfe-btn.small { letter-spacing: .2em; clip-path: polygon(10px 0, 100% 0, 100% calc(100% - 10px), calc(100% - 10px) 100%, 0 100%, 0 10px); }
html[data-bfe-theme="eternal"] .bfe-text-title span { display: inline-block; color: #fff; padding-bottom: .3em; text-shadow: 0 2px 18px rgba(0, 0, 0, .6);
  background: linear-gradient(90deg, transparent, #ff9a1f 25%, #ff9a1f 75%, transparent) bottom / 100% 3px no-repeat; }
html[data-bfe-theme="eternal"] .bfe-tab { clip-path: polygon(10px 0, 100% 0, 100% 100%, 0 100%, 0 10px); }

html[data-bfe-theme="bf1"] .bfe-btn { color: #dcefff; font-weight: 700; letter-spacing: .16em; border-radius: 3px;
  border: 1px solid rgba(140, 200, 255, .55); background: linear-gradient(180deg, rgba(60, 130, 240, .34), rgba(14, 52, 140, .6));
  text-shadow: 0 0 10px rgba(120, 190, 255, .75); box-shadow: inset 0 0 18px rgba(80, 160, 255, .22), 0 0 10px rgba(40, 120, 255, .18); }
html[data-bfe-theme="bf1"] .bfe-btn.active { color: #13204a; text-shadow: none; border-color: #ffe2a4;
  background: linear-gradient(180deg, #ffd27a 0%, #ffa42a 45%, #f57500 100%); box-shadow: 0 0 24px rgba(255, 170, 60, .55); }
html[data-bfe-theme="bf1"] .bfe-text-title span { display: inline-block; font-size: .78em; color: #13204a; padding: .2em 1.8em .2em .8em;
  background: linear-gradient(180deg, #ffd27a 0%, #ffa42a 42%, #f57500 100%); clip-path: polygon(0 0, calc(100% - 1em) 0, 100% 100%, 0 100%); }
html[data-bfe-theme="bf1"] .bfe-tab { border-radius: 3px 3px 0 0; }

html[data-bfe-theme="bf2"] .bfe-btn { color: #fff; font-weight: 800; letter-spacing: .02em; border-radius: 999px; font-size: clamp(15px, 2.2vh, 23px);
  border: 2px solid #23272d; background: linear-gradient(180deg, #f4f6f8 0%, #cfd4d9 16%, #a3aab2 48%, #858c95 52%, #b3b9c0 100%);
  box-shadow: inset 0 2px 0 rgba(255, 255, 255, .85), inset 0 -2px 0 rgba(0, 0, 0, .25), 0 3px 7px rgba(0, 0, 0, .6);
  text-shadow: -1px -1px 0 #111, 1px -1px 0 #111, -1px 1px 0 #111, 1px 1px 0 #111, 0 0 2px #000, 0 2px 4px rgba(0, 0, 0, .6); }
html[data-bfe-theme="bf2"] .bfe-btn .label, html[data-bfe-theme="bf2"] .bfe-tab .label { text-transform: capitalize; }
html[data-bfe-theme="bf2"] .bfe-btn.small { font-size: clamp(14px, 1.9vh, 19px); }
html[data-bfe-theme="bf2"] .bfe-btn.active { border-color: #7a0d06; transform: scale(1.03);
  background: linear-gradient(180deg, #ffd0c8 0%, #f0675a 16%, #cf2a1d 48%, #a5180e 52%, #d8483a 100%);
  box-shadow: inset 0 2px 0 rgba(255, 255, 255, .6), inset 0 -2px 0 rgba(0, 0, 0, .3), 0 0 18px rgba(230, 50, 35, .55), 0 3px 7px rgba(0, 0, 0, .6); }
html[data-bfe-theme="bf2"] .bfe-text-title span { display: inline-block; font-size: .74em; color: #fff; padding: .25em 1.2em; border-radius: .45em .45em 0 0;
  background: linear-gradient(180deg, #e24a3d, #a5180e); border: 2px solid #2b0703; border-bottom-width: 0;
  text-shadow: -1px -1px 0 #111, 1px -1px 0 #111, -1px 1px 0 #111, 1px 1px 0 #111; }
html[data-bfe-theme="bf2"] .bfe-tab { border-radius: 10px 10px 0 0; border: 2px solid #23272d; border-bottom: 0; color: #fff;
  text-shadow: -1px -1px 0 #111, 1px -1px 0 #111, -1px 1px 0 #111, 1px 1px 0 #111; }
html[data-bfe-theme="bf2"] .bfe-text-popup, html[data-bfe-theme="bf2"] .bfe-opt-label { text-shadow: 0 1px 2px rgba(0, 0, 0, .8); }
`;

const LAYOUT = `
@font-face { font-family: "BFE Saira"; src: url("${saira}") format("woff2"); font-weight: 100 900; font-stretch: 50% 125%; font-display: block; }

/* --bfe-gutter: the page's side margin; --bfe-page-frame: the width the page's own
   corners (the logo, the look's dropdown, the version), its screens and the battle's
   own screens (the spawn screen, the scoreboard) line up in, --bfe-edge where it
   starts; --bfe-frame: a screen's, its content and bottom buttons sharing their
   edges (an option page's is narrower, centred). */
html[data-bfe-theme] { --bfe-gutter: clamp(20px, 3vw, 56px); --bfe-page-frame: min(calc(100vw - 2 * var(--bfe-gutter)), 1680px);
  --bfe-edge: calc((100vw - var(--bfe-page-frame)) / 2); }
.bfe-page { position: fixed; inset: 0; z-index: 1; overflow: hidden; background: var(--bfe-backdrop); color: var(--bfe-text);
  font-family: "BFE Saira", system-ui, sans-serif; user-select: none; -webkit-user-select: none; }
.bfe-page *, .bfe-page *::before, .bfe-page *::after { box-sizing: border-box; }
.bfe-page button { font: inherit; color: inherit; }
.bfe-page [hidden] { display: none !important; }
.bfe-page.instant *, .bfe-page.instant *::before { transition: none !important; }
.bfe-page *, html[data-bfe-theme] dialog { scrollbar-width: thin; scrollbar-color: var(--bfe-scroll) transparent; }

/* The backdrop: a movie fading in from each side. */
.bfe-page-back, .bfe-backdrop, .bfe-page-stage { position: absolute; inset: 0; }
.bfe-backdrop-side { position: absolute; top: 0; bottom: 0; width: 56%; overflow: hidden; }
.bfe-backdrop-side.left { left: 0; animation: bfe-side-left 2.6s cubic-bezier(.16, .84, .24, 1) .1s both;
  -webkit-mask-image: linear-gradient(90deg, #000 0%, #000 30%, rgba(0, 0, 0, .6) 58%, transparent 94%);
  mask-image: linear-gradient(90deg, #000 0%, #000 30%, rgba(0, 0, 0, .6) 58%, transparent 94%); }
.bfe-backdrop-side.right { right: 0; animation: bfe-side-right 2.6s cubic-bezier(.16, .84, .24, 1) .5s both;
  -webkit-mask-image: linear-gradient(270deg, #000 0%, #000 30%, rgba(0, 0, 0, .6) 58%, transparent 94%);
  mask-image: linear-gradient(270deg, #000 0%, #000 30%, rgba(0, 0, 0, .6) 58%, transparent 94%); }
@keyframes bfe-side-left { from { opacity: 0; transform: translateX(-9%); } }
@keyframes bfe-side-right { from { opacity: 0; transform: translateX(9%); } }
.bfe-backdrop-movies { position: absolute; inset: 0; filter: var(--bfe-movie-filter);
  transform: translateX(var(--part, 0%)) scale(calc(1 + var(--depth, 0) * .035)); transition: transform 1.6s cubic-bezier(.2, .8, .2, 1); }
.bfe-page.wide .bfe-backdrop-side.left .bfe-backdrop-movies { --part: -7%; }
.bfe-page.wide .bfe-backdrop-side.right .bfe-backdrop-movies { --part: 7%; }
.bfe-backdrop-side::after { content: ""; position: absolute; inset: 0; background: var(--bfe-side-wash); mix-blend-mode: var(--bfe-side-blend); pointer-events: none; }
.bfe-movie { position: absolute; inset: 0; opacity: 0; transition: opacity var(--cross, 1.2s) ease; }
.bfe-movie.on { opacity: 1; }
.bfe-movie > canvas, .bfe-movie > video { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; display: block; }
.bfe-page-scrim, .bfe-page-dim, .bfe-page-pattern { position: absolute; inset: 0; pointer-events: none; }
.bfe-page-scrim { background: var(--bfe-scrim); }
.bfe-page-dim { background: var(--bfe-dim); opacity: 0; transition: opacity .45s ease; }
.bfe-page.dim .bfe-page-dim { opacity: 1; }
.bfe-page-pattern { background: var(--bfe-pattern); opacity: var(--bfe-pattern-opacity);
  -webkit-mask-image: radial-gradient(ellipse 30% 62% at 50% 50%, #000 0%, rgba(0, 0, 0, .5) 60%, transparent 100%);
  mask-image: radial-gradient(ellipse 30% 62% at 50% 50%, #000 0%, rgba(0, 0, 0, .5) 60%, transparent 100%); }

/* The logo: BATTLEFRONT over the ETERNAL bar. */
.bfe-logo { display: inline-grid; justify-items: end; text-transform: uppercase; line-height: .84; filter: drop-shadow(0 .14em .22em rgba(0, 0, 0, .55)); }
.bfe-logo .word { position: relative; display: block; font-weight: 900; font-stretch: 125%; letter-spacing: -.01em;
  color: var(--bfe-logo-face); text-shadow: var(--bfe-logo-depth); transform: perspective(1100px) rotateX(16deg); }
.bfe-logo .word::after { content: attr(data-text); position: absolute; inset: 0; color: transparent; text-shadow: none;
  background: var(--bfe-logo-fill); -webkit-background-clip: text; background-clip: text; }
.bfe-logo .tag { margin-top: .42em; margin-right: .1em; padding: .18em 2.4em .18em .9em; font-size: .25em; font-weight: 800; font-stretch: 112%;
  letter-spacing: .5em; line-height: 1.2; color: var(--bfe-tag-ink); background: var(--bfe-tag-fill);
  clip-path: polygon(0 0, calc(100% - 1.5em) 0, 100% 100%, 0 100%); }
.bfe-page-brand { position: absolute; top: 2.6vh; left: var(--bfe-edge); z-index: 2; font-size: min(1.7vw, 3.2vh, 32px); opacity: 0; pointer-events: none; }
.bfe-page-brand.on { opacity: .95; }

/* A screen: its title, its body, its buttons along the bottom. */
.bfe-screen { --bfe-frame: var(--bfe-page-frame); position: absolute; inset: 0; display: flex; flex-direction: column; align-items: center;
  padding: 7vh 0 calc(6vh + clamp(34px, 4.6vh, 46px)); }
.bfe-screen[data-layout="options"] { --bfe-frame: min(calc(100vw - 2 * var(--bfe-gutter)), 1000px); }
.bfe-screen.inert, .bfe-screen.leaving, .bfe-popup.leaving { pointer-events: none; }
@keyframes bfe-fade { from { opacity: 0; } }
.bfe-screen-head { flex: none; display: flex; justify-content: center; margin-bottom: 3.6vh; }
.bfe-screen-body { flex: 1 1 auto; min-height: 0; width: 100%; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 1.6vh; }
.bfe-screen-foot { position: absolute; left: calc((100vw - var(--bfe-frame)) / 2); right: calc((100vw - var(--bfe-frame)) / 2); bottom: 3vh;
  display: grid; grid-template-columns: 1fr auto 1fr; align-items: end; gap: 2vw; }
.bfe-screen-foot > div { display: flex; gap: 1vw; }
.bfe-screen-foot > .left { justify-self: start; }
.bfe-screen-foot > .middle { justify-self: center; }
.bfe-screen-foot > .right { justify-self: end; }
.bfe-screen-corner { position: absolute; inset: 0; pointer-events: none; }
.bfe-screen[data-layout="rows"] { justify-content: center; }
.bfe-screen[data-layout="rows"] .bfe-screen-head { margin-bottom: 5vh; }
.bfe-screen[data-layout="rows"] .bfe-screen-body { flex: 0 1 auto; }

/* The main menu and the gate: the logo high in the middle, still between them. */
.bfe-screen[data-layout="home"] { padding-top: 16vh; }
.bfe-screen[data-layout="home"] .bfe-screen-head { margin-bottom: 6.5vh; }
.bfe-screen[data-layout="home"] .bfe-screen-head .bfe-logo { font-size: min(5.5vw, 10vh, 104px); }
.bfe-screen[data-layout="home"] .bfe-screen-body { justify-content: flex-start; }
.bfe-gate-line { position: absolute; left: 0; right: 0; top: 74vh; margin: 0; text-align: center; font-size: clamp(14px, 2.1vh, 21px); font-weight: 700;
  letter-spacing: .32em; color: var(--bfe-text); text-shadow: 0 2px 12px rgba(0, 0, 0, .8); animation: bfe-pulse 1.4s ease-in-out infinite; }
@keyframes bfe-pulse { 0%, 100% { opacity: 1; } 50% { opacity: .3; } }

/* Moving between screens (index.js move()): the screen left cascades out, its chosen
   item flashing last; the next cascades in from the side it is reached from. Entrances
   name only where they start, so an item's own state (dimmed) holds after them. */
.bfe-screen.leaving .bfe-screen-head { animation: bfe-head-out .2s ease-in both; }
.bfe-screen.leaving[data-layout="home"] .bfe-screen-head { animation: none; }
.bfe-screen.leaving .bfe-rows .bfe-btn { animation: bfe-item-out .24s cubic-bezier(.55, 0, .75, .2) both; animation-delay: calc(min(var(--i, 0), 8) * 18ms); }
.bfe-screen.leaving[data-dir="back"] .bfe-rows .bfe-btn { animation-name: bfe-item-out-back; }
.bfe-screen.leaving[data-dir="forward"] .bfe-rows .bfe-btn.active { animation: bfe-chosen .36s ease-out both; }
.bfe-screen.leaving:not([data-layout="rows"]):not([data-layout="home"]) .bfe-screen-body { animation: bfe-body-out .22s ease-in both; }
.bfe-screen.leaving .bfe-screen-foot .bfe-btn, .bfe-screen.leaving .bfe-screen-corner, .bfe-screen.leaving .bfe-gate-line { animation: bfe-foot-out .2s ease-in both; }
.bfe-screen.entering .bfe-screen-head { animation: bfe-head-in .5s cubic-bezier(.2, .8, .2, 1) .1s both; }
.bfe-screen.entering[data-layout="home"] .bfe-screen-head { animation: none; }
.bfe-screen.entering .bfe-rows .bfe-btn { animation: bfe-item-in .42s cubic-bezier(.2, .8, .2, 1) both; animation-delay: calc(.12s + min(var(--i, 0), 10) * 38ms); }
.bfe-screen.entering[data-dir="back"] .bfe-rows .bfe-btn { animation-name: bfe-item-in-back; }
.bfe-screen.entering:not([data-layout="rows"]):not([data-layout="home"]) .bfe-screen-body { animation: bfe-body-in .5s cubic-bezier(.2, .8, .2, 1) .08s both; }
.bfe-screen.entering .bfe-opt { animation: bfe-row-in .34s cubic-bezier(.2, .8, .2, 1) both; animation-delay: calc(.2s + min(var(--i, 0), 14) * 22ms); }
.bfe-screen.entering .bfe-screen-foot > :not(.steady) .bfe-btn, .bfe-screen.entering .bfe-screen-corner { animation: bfe-foot-in .4s cubic-bezier(.2, .8, .2, 1) .22s both; }
.bfe-screen.entering .bfe-text-title span { animation: bfe-title-in .55s cubic-bezier(.2, .8, .2, 1) .12s both; }
@keyframes bfe-head-out { to { opacity: 0; transform: translateY(-1.2vh); } }
@keyframes bfe-head-in { from { opacity: 0; transform: translateY(-1.6vh); } }
@keyframes bfe-title-in { from { letter-spacing: .32em; filter: blur(3px); } }
@keyframes bfe-item-out { to { opacity: 0; transform: translateX(-5vw); } }
@keyframes bfe-item-out-back { to { opacity: 0; transform: translateX(5vw); } }
@keyframes bfe-item-in { from { opacity: 0; transform: translateX(6vw); } }
@keyframes bfe-item-in-back { from { opacity: 0; transform: translateX(-6vw); } }
@keyframes bfe-chosen { 30% { opacity: 1; transform: scale(1.06); filter: brightness(1.7); } to { opacity: 0; transform: scale(1.1); } }
@keyframes bfe-body-out { to { opacity: 0; transform: scale(.97); } }
@keyframes bfe-body-in { from { opacity: 0; transform: translateY(2vh) scale(.97); } }
@keyframes bfe-row-in { from { opacity: 0; transform: translateX(1.4vw); } }
@keyframes bfe-foot-in { from { opacity: 0; transform: translateY(2.4vh); } }
@keyframes bfe-foot-out { to { opacity: 0; transform: translateY(1.6vh); } }

/* Titles, headings, labels. */
.bfe-text { margin: 0; }
.bfe-text-title { font-size: clamp(26px, 4.6vh, 50px); font-weight: 800; font-stretch: 118%; letter-spacing: .06em; line-height: 1; text-transform: uppercase; text-align: center; }
.bfe-text-heading { align-self: flex-start; margin: 1.4vh 0 .2vh; font-size: clamp(13px, 1.9vh, 19px); font-weight: 800; letter-spacing: .22em; text-transform: uppercase; color: var(--bfe-accent); }
.bfe-text-label { font-size: clamp(12px, 1.7vh, 17px); font-weight: 700; letter-spacing: .2em; text-transform: uppercase; color: var(--bfe-text-dim); }
.bfe-text-corner { position: absolute; font-size: 12px; font-weight: 600; letter-spacing: .08em; color: var(--bfe-text-faint); white-space: pre; }

/* Buttons: menu rows, popups' and the bottom row's. */
.bfe-btn { appearance: none; margin: 0; padding: 0 1.4em; position: relative; display: flex; align-items: center; justify-content: center;
  height: clamp(38px, 5.6vh, 56px); font-size: clamp(14px, 2vh, 21px); cursor: pointer; outline: none; white-space: nowrap;
  transition: background .14s ease, color .14s ease, box-shadow .14s ease, transform .14s ease, border-color .14s ease, opacity .14s ease; }
.bfe-btn .label { text-transform: uppercase; overflow: hidden; text-overflow: ellipsis; }
.bfe-btn.small { height: clamp(34px, 4.6vh, 46px); font-size: clamp(13px, 1.7vh, 18px); min-width: clamp(120px, 11vw, 200px); }
.bfe-btn.disabled { opacity: .4; cursor: default; }
.bfe-rows { position: relative; display: flex; flex-direction: column; gap: var(--bfe-gap); width: max-content; min-width: min(400px, 30vw); max-width: 86vw;
  max-height: 100%; padding: 4px 8px; }
.bfe-rows.scroll { overflow-y: auto; overflow-x: hidden; }
.bfe-btn { overflow: hidden; }
/* A sheen across the item the focus comes to. */
.bfe-btn::after { content: ""; position: absolute; inset: 0; pointer-events: none; transform: translateX(-120%);
  background: linear-gradient(105deg, transparent 35%, rgba(255, 255, 255, .3) 50%, transparent 65%); }
.bfe-btn.active::after { animation: bfe-sheen .75s cubic-bezier(.2, .7, .2, 1) .04s; }
@keyframes bfe-sheen { to { transform: translateX(120%); } }

/* Option pages: one panel, label | value rows. */
.bfe-screen[data-layout="options"] .bfe-screen-body { flex: 0 1 auto; margin-block: auto; width: var(--bfe-frame); overflow-y: auto; overflow-x: hidden; align-items: stretch;
  justify-content: flex-start; gap: 1vh; padding: 2.4vh 2.2vw; background: var(--bfe-panel-bg); border: var(--bfe-panel-border);
  border-radius: var(--bfe-panel-radius); clip-path: var(--bfe-panel-clip); box-shadow: var(--bfe-panel-shadow);
  -webkit-backdrop-filter: blur(10px); backdrop-filter: blur(10px); scrollbar-width: thin; }
.bfe-options { display: flex; flex-direction: column; }
.bfe-opt { position: relative; display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1.1fr); align-items: center; gap: 2vw;
  min-height: clamp(36px, 4.8vh, 50px); padding: 0 1.2vw; border-bottom: 1px solid var(--bfe-row-line); cursor: pointer; transition: background .12s ease; }
.bfe-opt.bare { grid-template-columns: 1fr; justify-items: center; }
.bfe-opt-label { font-size: clamp(13px, 1.75vh, 18px); font-weight: 700; letter-spacing: .12em; text-transform: uppercase; color: var(--bfe-text-dim); }
.bfe-opt-value { display: flex; align-items: center; justify-content: flex-end; gap: .7em; min-width: 0; font-size: clamp(13px, 1.75vh, 18px);
  font-weight: 700; letter-spacing: .08em; text-transform: uppercase; color: var(--bfe-text); }
.bfe-opt-text { display: inline-block; min-width: 9em; text-align: center; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.bfe-opt-text.changed { animation: bfe-value .32s cubic-bezier(.2, .8, .2, 1.3); }
@keyframes bfe-value { from { transform: scale(1.22); color: var(--bfe-accent); } }
.bfe-opt[data-kind="slider"] .bfe-opt-text, .bfe-opt[data-kind="fixed"] .bfe-opt-text { min-width: 4.5em; text-align: right; }
.bfe-opt.active { background: var(--bfe-row-focus); }
.bfe-opt.active::before { content: ""; position: absolute; left: 0; top: 0; bottom: 0; width: 3px; background: var(--bfe-accent); }
.bfe-opt.active .bfe-opt-label { color: var(--bfe-text); }
.bfe-opt.active .bfe-opt-value { color: var(--bfe-value-focus); }
.bfe-opt.disabled { opacity: .42; cursor: default; }
.bfe-opt[data-kind="fixed"] { cursor: default; }
.bfe-opt.dim .bfe-opt-value { opacity: .5; }
.bfe-opt-arrow { appearance: none; border: 0; margin: 0; padding: 0 .35em; background: none; font-size: 1.5em; line-height: 1; color: var(--bfe-text-faint); cursor: pointer; }
.bfe-opt.active .bfe-opt-arrow { color: var(--bfe-accent); }
.bfe-opt-arrow:hover { color: var(--bfe-text); }
.bfe-opt-action { padding: .45em 1.3em; border: 1px solid var(--bfe-accent); color: var(--bfe-accent); letter-spacing: .14em; white-space: nowrap; }
.bfe-opt.active .bfe-opt-action { background: var(--bfe-accent-fill); color: var(--bfe-accent-ink); border-color: transparent; }
.bfe-slider { position: relative; flex: 1 1 auto; min-width: 110px; max-width: 300px; height: 8px; background: var(--bfe-track); border-radius: var(--bfe-track-radius); cursor: pointer; touch-action: none; }
.bfe-slider::before { content: ""; position: absolute; inset: -12px 0; }
.bfe-slider-fill { position: absolute; left: 0; top: 0; bottom: 0; background: var(--bfe-accent-fill); border-radius: inherit; }
.bfe-slider-thumb { position: absolute; top: 50%; width: 14px; height: 22px; transform: translate(-50%, -50%); background: var(--bfe-thumb);
  border: var(--bfe-thumb-border); border-radius: var(--bfe-thumb-radius); box-shadow: 0 1px 4px rgba(0, 0, 0, .6); }
.bfe-options-note { min-height: 1.4em; margin: 1.2vh 0 0; font-size: clamp(11px, 1.5vh, 15px); font-weight: 700; letter-spacing: .12em;
  text-transform: uppercase; color: var(--bfe-note); text-align: center; }

/* Tabs. */
.bfe-tabs { display: flex; gap: .5vw; border-bottom: 2px solid var(--bfe-tab-line); }
.bfe-tab { appearance: none; margin: 0; border: 0; padding: .75em 1.8em; font-size: clamp(12px, 1.6vh, 16px); font-weight: 800; letter-spacing: .16em;
  color: var(--bfe-text-dim); background: var(--bfe-tab-bg); cursor: pointer; outline: none; }
.bfe-tab .label { text-transform: uppercase; }
.bfe-tab.active { color: var(--bfe-tab-active-ink); background: var(--bfe-tab-active-bg); }
.bfe-tab.focused { box-shadow: inset 0 0 0 2px var(--bfe-accent); }
.bfe-tab { position: relative; transition: background .18s ease, color .18s ease; }
.bfe-tab.disabled { opacity: .5; cursor: default; }
.bfe-tab-badge { margin-left: .8em; padding: .1em .45em; font-size: .7em; letter-spacing: .12em; border: 1px solid currentColor; opacity: .8; }

/* Lists. */
.bfe-list { display: flex; flex-direction: column; min-height: 0; overflow-y: auto; overflow-x: hidden; padding: 4px; background: var(--bfe-list-bg); border: var(--bfe-panel-border); }
.bfe-list-row { flex: none; display: grid; grid-template-columns: var(--columns, 1fr) auto; align-items: center; gap: 1em; min-height: clamp(30px, 4vh, 42px); padding: 0 .9em;
  font-size: clamp(12px, 1.65vh, 17px); font-weight: 700; letter-spacing: .08em; text-transform: uppercase; color: var(--bfe-text); cursor: pointer; }
.bfe-list-row:hover { background: var(--bfe-list-hover); }
.bfe-list-row.selected { background: var(--bfe-list-selected); }
.bfe-list.focused .bfe-list-row.selected { background: var(--bfe-accent-fill); color: var(--bfe-accent-ink); text-shadow: none; }
.bfe-list-row.disabled { opacity: .4; }
.bfe-list-cell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.bfe-list-cell.end { text-align: right; }
.bfe-list-icons { display: flex; gap: .4em; }
.bfe-icon { font-size: .72em; padding: .12em .5em; border: 1px solid currentColor; letter-spacing: .1em; opacity: .85; }
.bfe-list.empty { min-height: 8vh; }

/* Side by side: Instant Action. */
.bfe-screen[data-layout="wide"] .bfe-screen-body { width: var(--bfe-frame); justify-content: stretch; }
.bfe-columns { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1.25fr) minmax(0, 1fr); gap: 2.2vw; width: 100%; height: 100%; min-height: 0; }
.bfe-column { display: flex; flex-direction: column; gap: 1.2vh; min-height: 0; }
/* A column's heading row: a label, and a dropdown beside it (Instant Action's game). */
.bfe-column-head, .bfe-column > .bfe-text-label { flex: none; display: flex; align-items: center; justify-content: space-between; gap: 1vw; min-height: clamp(30px, 3.8vh, 38px); }
.bfe-column-head > .bfe-text-label { flex: none; white-space: nowrap; letter-spacing: .16em; }
.bfe-column-head .bfe-field { flex: 0 1 auto; min-width: 0; }
.bfe-column-head .bfe-field select { width: 100%; max-width: 100%; text-overflow: ellipsis; }
.bfe-column > .bfe-list { flex: 1 1 auto; background: var(--bfe-panel-bg); -webkit-backdrop-filter: blur(10px); backdrop-filter: blur(10px); }
.bfe-column[data-column="1"] { justify-content: center; align-items: center; gap: 2.4vh; }
.bfe-preview { margin: 0; width: 100%; display: flex; flex-direction: column; gap: 1.2vh; }
.bfe-preview-frame { position: relative; width: 100%; aspect-ratio: 3 / 2; overflow: hidden; background: #000; border: var(--bfe-preview-border); box-shadow: 0 12px 44px rgba(0, 0, 0, .6); }
.bfe-preview figcaption { min-height: 1.2em; text-align: center; font-size: clamp(14px, 2.2vh, 23px); font-weight: 800; letter-spacing: .14em; text-transform: uppercase; color: var(--bfe-text); }
.bfe-hbuttons { display: flex; flex-wrap: wrap; gap: 1.2vw; justify-content: center; }

/* Dropdowns and text fields in the look (the server browser's filters, Instant Action's game). */
.bfe-field { position: relative; display: inline-flex; }
.bfe-field select, .bfe-input { appearance: none; height: clamp(30px, 3.8vh, 38px); padding: 0 2.2em 0 .9em; font: inherit; font-size: clamp(11px, 1.45vh, 14px); font-weight: 700; letter-spacing: .1em;
  text-transform: uppercase; color: var(--bfe-text); background: rgba(0, 0, 0, .35); border: 1px solid var(--bfe-dialog-button-border); border-radius: var(--bfe-dialog-button-radius); cursor: pointer; outline: none; }
.bfe-field::after { content: ""; position: absolute; right: .9em; top: 50%; width: .45em; height: .45em; border-right: 2px solid var(--bfe-accent); border-bottom: 2px solid var(--bfe-accent);
  transform: translateY(-70%) rotate(45deg); pointer-events: none; }
.bfe-field select option { background: var(--bfe-dialog-bg); color: var(--bfe-text); }
.bfe-field select option:disabled { color: var(--bfe-text-faint); }
.bfe-field select:focus, .bfe-input:focus { border-color: var(--bfe-accent); box-shadow: 0 0 0 1px var(--bfe-accent); }

/* A popup over the screen. */
.bfe-popup { position: absolute; inset: 0; z-index: 5; display: grid; place-items: center; background: rgba(0, 0, 0, .5);
  -webkit-backdrop-filter: blur(4px); backdrop-filter: blur(4px); animation: bfe-fade .2s ease both; }
.bfe-popup.leaving { animation: bfe-popup-out .22s ease-in both; }
.bfe-popup.leaving .bfe-popup-box { animation: bfe-pop-out .22s ease-in both; }
@keyframes bfe-popup-out { to { opacity: 0; } }
@keyframes bfe-pop-out { to { opacity: 0; transform: scale(.94); } }
.bfe-popup-box { display: flex; flex-direction: column; align-items: center; gap: 3.2vh; min-width: min(480px, 86vw); max-width: min(700px, 92vw);
  padding: 4.5vh 3vw 3.5vh; background: var(--bfe-panel-bg); border: var(--bfe-panel-border); border-radius: var(--bfe-panel-radius);
  clip-path: var(--bfe-panel-clip); box-shadow: 0 20px 60px rgba(0, 0, 0, .6); animation: bfe-pop .26s cubic-bezier(.2, .8, .2, 1.1) both; }
@keyframes bfe-pop { from { opacity: 0; transform: scale(.94); } }
.bfe-text-popup { white-space: pre-line; text-align: center; font-size: clamp(15px, 2.1vh, 21px); font-weight: 700; letter-spacing: .06em; line-height: 1.45; color: var(--bfe-text); }

/* Over a battle (overlay): the frozen world through a darkened blur; gone, and letting
   the pointer through, with no screen up, but for its prompt. */
.bfe-page.overlay { z-index: 20; background: transparent; transition: opacity .25s ease; }
.bfe-page.overlay .bfe-page-scrim { background: var(--bfe-scrim), var(--bfe-overlay); -webkit-backdrop-filter: blur(6px); backdrop-filter: blur(6px); }
.bfe-page.overlay.empty { pointer-events: none; }
.bfe-page.overlay.empty:not(.prompting) { opacity: 0; visibility: hidden; transition: opacity .25s ease, visibility 0s linear .25s; }
.bfe-page.overlay.empty > :not(.bfe-page-prompt) { opacity: 0; transition: opacity .25s ease; }
.bfe-page-prompt { display: none; position: absolute; left: 0; right: 0; top: 50%; margin: 0; transform: translateY(-50%); text-align: center; font-size: clamp(14px, 2.1vh, 21px);
  font-weight: 700; letter-spacing: .32em; color: var(--bfe-text); text-shadow: 0 2px 12px rgba(0, 0, 0, .9); animation: bfe-pulse 1.4s ease-in-out infinite; pointer-events: none; }
.bfe-page.prompting .bfe-page-prompt { display: block; }
/* The battle's audio panel over the page, in the look. */
html[data-bfe-theme] #audio-panel { z-index: 30 !important; background: var(--bfe-dialog-bg) !important; border: var(--bfe-panel-border) !important;
  border-radius: var(--bfe-panel-radius) !important; color: var(--bfe-text) !important; font-family: "BFE Saira", system-ui, sans-serif !important; }
html[data-bfe-theme] #audio-panel button { font-family: inherit !important; font-weight: 700; letter-spacing: .06em; color: var(--bfe-text); cursor: pointer;
  background: var(--bfe-dialog-button-bg); border: 1px solid var(--bfe-dialog-button-border); border-radius: var(--bfe-dialog-button-radius); padding: 3px 8px; }

/* The battle's chat in the look: its font, and the entry line on a panel (the
   channels keep their colours). */
html[data-bfe-theme] .bfe-chat { font-family: "BFE Saira", system-ui, sans-serif !important; font-weight: 600; letter-spacing: .02em; }
html[data-bfe-theme] .bfe-chat-entry { background: var(--bfe-panel-bg) !important; border: var(--bfe-panel-border); border-radius: var(--bfe-panel-radius); padding: 8px 12px !important; }

/* The look's dropdown (for now). */
.bfe-page-theme { position: absolute; top: 18px; right: var(--bfe-edge); z-index: 10; display: flex; align-items: center; gap: 10px;
  font-size: 12px; font-weight: 700; letter-spacing: .24em; text-transform: uppercase; color: rgba(255, 255, 255, .7); }
.bfe-page-theme select { font: 600 13px/1 "BFE Saira", system-ui, sans-serif; letter-spacing: .04em; color: #fff; background: rgba(8, 10, 16, .78);
  border: 1px solid rgba(255, 255, 255, .28); border-radius: 4px; padding: 7px 10px; cursor: pointer; }
.bfe-page-theme select option { background: #0b0f17; color: #fff; }

/* The front end's dialogs (profile, hosting, direct connection, notices) in the look. */
html[data-bfe-theme] dialog { background: var(--bfe-dialog-bg) !important; color: var(--bfe-text) !important; border: var(--bfe-panel-border) !important;
  border-radius: var(--bfe-panel-radius) !important; font-family: "BFE Saira", system-ui, sans-serif !important; box-shadow: 0 24px 80px rgba(0, 0, 0, .7); padding: 28px 32px !important; }
html[data-bfe-theme] dialog::backdrop { background: rgba(0, 0, 0, .6) !important; -webkit-backdrop-filter: blur(4px); backdrop-filter: blur(4px); }
html[data-bfe-theme] dialog h2 { font-family: inherit !important; font-weight: 800 !important; letter-spacing: .06em; text-transform: uppercase; }
html[data-bfe-theme] dialog button { font-family: inherit !important; font-weight: 700 !important; letter-spacing: .1em; text-transform: uppercase; cursor: pointer;
  border-radius: var(--bfe-dialog-button-radius) !important; border: 1px solid var(--bfe-dialog-button-border) !important;
  background: var(--bfe-dialog-button-bg) !important; color: var(--bfe-text) !important; padding: 10px 18px !important; margin: 8px 8px 0 0; }
html[data-bfe-theme] dialog button[data-accept], html[data-bfe-theme] dialog button[type="submit"] { background: var(--bfe-accent-fill) !important;
  color: var(--bfe-accent-ink) !important; border-color: transparent !important; }
html[data-bfe-theme] dialog input, html[data-bfe-theme] dialog select { font: 600 16px "BFE Saira", system-ui, sans-serif; color: var(--bfe-text);
  background: rgba(0, 0, 0, .35); border: 1px solid var(--bfe-dialog-button-border); border-radius: 3px; padding: 8px 10px; margin-left: 8px; }
`;

export const STYLE = LOOKS + LAYOUT + SHAPES;
