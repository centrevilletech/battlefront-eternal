# Slint Screen Port Pattern (`rust/bfe-ui`)

Every native menu, setup, browser, and battle screen port follows the single pattern established by `main.slint`.

## 1. Where a Screen Lives
- Screen `.slint` files live under `rust/bfe-ui/ui/screens/<group>.slint` (for example `rust/bfe-ui/ui/screens/main.slint`) and are re-exported from `rust/bfe-ui/ui/gallery.slint` and `rust/bfe-client/ui/overlay.slint`.
- `rust/bfe-client/build.rs` compiles `ui/overlay.slint` with `with_include_paths(vec![PathBuf::from("../bfe-ui/ui")])`, sharing the single `Theme` global and font registrations across `bfe-ui` and `bfe-client`.
- **Component & Token Rule**: Screens may only compose `bfe-ui` components (`ScreenFrame`, `PageBrand`, `Wordmark`, `PageTitle`, `PageText`, `PageButton`, `PageTab`, `TabStrip`, `PageList`, `PageListRow`, `OptionRow`, `OptionArrow`, `PageToggle`, `InputField`, `SelectField`, `PageSlider`, `PopupBox`, `PageDialog`, `OfferDialog`, `MovieLayer`, `BackdropMovies`, `PreviewFrame`) and `Theme` tokens (`Theme.menu-theme`, `Theme.bfe-*`). Never hardcode colors, gradients, or font names inside a screen file.

## 2. How the Rust Model Feeds It
- Existing Rust state machines (`ShellApp` in `bfe-shell`, `OptionsController` in `bfe-interface`, `FirstRun` / `ImportWork` in `bfe-client`, and `BattleController` in `bfe-client`) remain the single source of truth.
- `SlintHost::sync_shell` in `rust/bfe-client/src/app/slint_ui.rs` (or a per-screen helper module `rust/bfe-client/src/app/slint_<group>.rs` reading/writing an exported `<Group>Model` global) projects the active Rust model into Slint properties on each frame and after each input event:
  - `Theme.menu-theme` and `theme-index` sync from `shell.app.settings.menu_theme` (`"bf1"`, `"bf2"`, `"eternal"`).
  - Screen visibility (`active-screen`) and modal flags (`profile-editing`, `discord-offer-open`, `steam-offer-open`, `popup-open`) gate which Slint subtree is instantiated.
  - Row lists (`ModelRc<MenuRowItem>`) and selection indices (`selected-row`, `*-selected-button`) bind directly from `ShellApp`. **Important**: Cache the previous `Vec<...>` in Rust and only call `set_rows(ModelRc::new(VecModel::from(...)))` when the slice contents change; replacing a `ModelRc` on every `PointerPressed` event tears down Slint `for` repeaters mid-click and drops `clicked` callbacks.
- Callbacks emitted by Slint push typed `UiAction` variants into `SlintHost::actions`, which `SlintHost::drain_actions` applies back onto `ShellHost` / `ShellApp` / `AppSettingsStore`.

## 3. Input, Focus, Escape, and Sounds
- **Pointer Hit-Testing**: `GameUi.controls-at(x, y)` returns `true` whenever a visible Slint screen, modal, or dropdown contains the cursor. `SlintHost::event` consumes pointer presses, releases, and wheel scrolling over active Slint controls while leaving 3D combat mouselook (`pointer_locked == true`) untouched.
- **Keyboard & Focus**:
  - When a Slint `InputField` is active (`GameUi.text-focused == true`, e.g., `ProfileModal`), `SlintHost::event` routes key presses, modifiers, clipboard shortcuts, and `WindowEvent::Ime` (`Preedit` / `Commit`) to Slint's `TextInput`.
  - `Escape` during modal editing is allowed to fall through (or invoke `cancelled`) so `ShellHost::handle_key_with_shift` cancels the active dialog cleanly.
  - Non-text menu navigation (`ArrowUp`, `ArrowDown`, `ArrowLeft`, `ArrowRight`, `Tab`, `Enter`, `Escape`) routes through `ShellHost::handle_key_with_shift` onto `ShellApp`, whose updated cursor/selection is immediately synced back to Slint via `sync_shell`.
- **Sounds**: `SlintHost::new` wires `Theme.on_sound_requested` to `ShellApp.sound_queue` (`MenuSound::Select -> "shell_select_change"`, `Accept -> "shell_menu_accept"`, `Back -> "shell_menu_cancel"`, `Error -> "shell_menu_error"`), deduplicating against cues already queued by `ShellApp` in the same step and flushing through `SharedAudioRouter`.

## 4. Backdrop Movies, Blur, and 3D Underlays
- `ShellHost::step_and_draw` decodes and uploads the left/right backdrop movies (`MovieTexture`), vertical look wash, elliptical center scrim, and scanline pattern into `shell.canvas.framebuffer`.
- `GameUi` in `rust/bfe-client/ui/overlay.slint` separates the base page + `THEME` picker (`base-layer`, `render-pass == 1`) from modal/popup dialogs and their dimmed backdrop (`modal-layer`, `render-pass == 2`).
- When no modal is open (`render-pass == 0`), `SlintHost::render` blits `shell.canvas.framebuffer` into window framebuffer `0` inside `RenderingState::BeforeRendering`, renders the Slint tree over it, and blits the composited result back into `shell.canvas.framebuffer`.
- When a modal or popup is open (`ui.modal_open()`), `StandaloneWindowApp` renders in two passes on the shared GL context: Pass 1 (`ui.render_pass(fb, sz, 1)`) composites `base-layer` (including the wordmark, menu rows, `THEME` picker, and version text) over the movie backdrop and blits back into `shell.canvas.framebuffer`; `shell.apply_modal_blur()` runs `PageBlur::apply(..., 4.0)` over `shell.canvas.framebuffer`; Pass 2 (`ui.render_pass(fb, sz, 2)`) composites `modal-layer` (`Theme.dialog-backdrop` / `Theme.popup-backdrop` dim + sharp dialog box) over the blurred framebuffer and swaps.
- For sub-rect movies or 3D previews inside a screen (such as the Instant Action / Server Browser map flyover or spawn turntable), either leave a transparent viewport cutout over the native quad or bind the GL texture via `slint::BorrowedOpenGLTextureBuilder`.

## 5. Deleting Old Painters
- In the same commit that ports a screen to Slint, delete its legacy quad/glyph painter branches and unused `Hit` variants from `rust/bfe-shell/src/page.rs` and `rust/bfe-shell/src/page/*.rs`.
- Keep `Page::draw`'s shared movie backdrop underlay and any unported screen branches intact until their respective screen ports land.

## 6. Capturing Matched Pairs (`origin/main` `65299190b` vs Native Slint)
1. **Browser Reference (`65299190b`)**: Run the reference Electron/browser app from `origin/main` (`65299190b`) at `1280x720` on a virtual X11 display across all three looks (`eternal`, `bf1`, `bf2`), driving real pointer/keyboard states (`idle`, `hover`, `pressed`, `focus`) via `xdotool` and saving `ref-<state>.png` + `ref-<state>.json`.
2. **Native Slint (`bfe-client`)**: Run the release binary on X11 (`1280x720`) and native Wayland at scale 2 (`weston --backend=x11 --renderer=pixman --width=1280 --height=720 --scale=2`, with `DISPLAY` unset for the client), driving real input (`xdotool`) across the same screens, states, and looks, saving `native-<state>.png` and side-by-side `pair-<state>.png`, and stopping all display/client processes before finishing.
