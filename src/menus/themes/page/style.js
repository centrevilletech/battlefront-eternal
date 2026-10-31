import SHARED from "../../../../electron/ui/looks.css?inline";

// The page theme's styles (index.js). The looks are custom properties on
// <html data-bfe-theme>, so the front end's dialogs, outside the page, take
// them too; the shapes each look changes (buttons, titles, tabs) follow
// its block of properties. One layout for all three: only colours and
// surfaces differ.
//
// The looks, the font, the logo and those shapes are shared with the
// desktop app's own pages, which the app serves without a build, so they
// live beside them (electron/ui/looks.css). Vite inlines the file here and
// emits its font as an asset.

const LAYOUT = `
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

/* Headings, labels. */
.bfe-text-heading { align-self: flex-start; margin: 1.4vh 0 .2vh; font-size: clamp(13px, 1.9vh, 19px); font-weight: 800; letter-spacing: .22em; text-transform: uppercase; color: var(--bfe-accent); }
.bfe-text-label { font-size: clamp(12px, 1.7vh, 17px); font-weight: 700; letter-spacing: .2em; text-transform: uppercase; color: var(--bfe-text-dim); }
.bfe-text-corner { position: absolute; font-size: 12px; font-weight: 600; letter-spacing: .08em; color: var(--bfe-text-faint); white-space: pre; }

.bfe-rows { position: relative; display: flex; flex-direction: column; gap: var(--bfe-gap); width: max-content; min-width: min(400px, 30vw); max-width: 86vw;
  max-height: 100%; padding: 4px 8px; }
.bfe-rows.scroll { overflow-y: auto; overflow-x: hidden; }

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
/* The note under the panel (index.js), the two centred together. */
.bfe-screen > .bfe-options-note { flex: none; width: var(--bfe-frame); margin-bottom: auto; text-shadow: 0 1px 6px rgba(0, 0, 0, .9); }
.bfe-screen[data-layout="options"] > .bfe-screen-body:has(+ .bfe-options-note) { margin-bottom: 0; }

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

export const STYLE = SHARED + LAYOUT;
