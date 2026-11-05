# Audio

The port's sound has two modes, switched live in Settings → Audio:

- **Original (2004)** (CLASSIC in the menus) plays what the Steam build
  plays, when and how it plays it: the same sounds from the same archives,
  picked, gated, mixed, and placed by the game's sound library's rules,
  through what the build's OpenAL Soft 1.20.1 output does with them.
  `docs/formats.md` ("Sound") has the rules. On stereo speakers it plays
  through the light mix (below): the same mix without the reverb, summed by
  the Rust output engine. Both modes use Rust for every playback and DSP operation.
- **Enhanced (2026)** keeps every one of those decisions (which sound, when,
  how loud in the mix, from where) and renders them for a 2026 room: a
  spatial sound stage for headphones, stereo, 5.1, 7.1 and 7.1.4, rooms and
  occlusion worked out from the map's collision, an LFE, a loudness-managed
  master, and the game's samples upscaled offline.

The two modes play at the same loudness (within half a LU on the same
battle), so switching between them compares the sound, not the level.

Nothing here commits retail data. The decoded and upscaled sound lives in
`public/audio/` (gitignored), and so do the headphone filters.

## Getting the sound

```
python3 tools/extract.py nab2 --side cis      # the map, the side, and now its sound
python3 tools/zero_sound.py nab2 --side rep   # the sound alone
```

Either writes `public/audio/`:

- `index.json`: each mission and map to its sound sub-level (`nab2c` →
  `nab2cw`).
- `nab2cw/sound.json` and `bank.bin`: every sound object of `core.lvl` and
  the sub-level, the mission's audio calls, each class's sound keys, the
  map's emitters, regions and material boxes, and the sub-level's 543
  samples as 16-bit PCM (24 MB).
- `streams/<name>.json` and `.bin`: each stream the mission opens, as stored
  (Xbox IMA ADPCM; 45 MB for Theed's).

Optional, for the enhanced mode (a Python venv; see the licenses below):

```
python3 -m venv .venv-audio
.venv-audio/bin/pip install -r tools/requirements-audio.txt
.venv-audio/bin/python tools/enhance_audio.py nab2cw --jobs 8   # upscaled samples and streams (needs ffmpeg with libopus)
.venv-audio/bin/python tools/make_binaural.py                   # the headphone decoder (downloads SADIE II D1)
```

These write `public/audio/enhanced/nab2cw/` (41 MB of FLAC samples, 86 MB
of Opus streams, and a manifest) and `public/audio/hrtf/`. Without the
enhanced folder the enhanced mode renders the original samples; without
the headphone filters it uses the Rust IRCAM HRTF decoder on headphones.

Each sample and stream segment is made once, in `public/audio/enhanced/samples/`
and `streams/<stream>/` (with a `.json` beside it recording its settings
and report), and hard-linked (copied where links are not possible) into
every sub-level's folder that uses it: the samples all come from one bank,
and an era's voice-over streams (`cw_vo`, `cw_tac_vo`, `gcw_vo`,
`gcw_tac_vo`) are every one of its battles'. The second battle of an era
enhances only its own music and ambience. The Xbox ADPCM decode runs
over every block at once with numpy (each 36-byte block starts from its
own header), falling back to plain loops without it.

## Settings, the panel, and debugging

Settings → Audio (`src/audio/options.js`; the defaults are in
`src/settings.js`):

- **Sound**: Enhanced (2026 sound stage) or Original (2004), shown as
  ENHANCED and CLASSIC. A first session on a low-end CPU starts on Classic
  (below, "The light mix"); the row says what Classic is on stereo, and on
  such a CPU with Enhanced chosen, that Classic is much lighter there.
- **Output**: Automatic, Headphones (binaural), Stereo speakers, 5.1, 7.1,
  or 7.1.4. Automatic follows the device's channel count: 12 or more is
  7.1.4, 8 to 11 is 7.1, 6 or 7 is 5.1, anything else stereo. Headphones
  are never detected (browsers don't say); choose them.
- **Dynamics**: Home theater (full range), Normal, or Night (narrow range).
- **Mono**, **Clarity (enhanced)** (the clarity stage below; on by
  default, off to hear the enhanced rendering without it), and volumes for
  master, effects, music, voice, ambience and the interface. Voice covers
  the voice-over and every soldier's own voice: his hurts and death, and
  the AI's chatter (in the original those two are on the effects slider's
  bus).
- "Speaker test and audio details" opens the audio panel; so does
  `?audiodebug=1`.

The panel shows the device (channels, rate, latency), what is rendering,
the loudness meter (momentary, short-term and integrated LUFS, true peak,
each channel's peak, the limiter's gain reduction), the room the listener
is in, and the voices playing. Its speaker test plays pink noise on one
channel at a time, named, with the game muted (the LFE gets a 50 Hz tone),
and "around the room" pans a burst once round through the game's own
panner.

`window.__bf.audio` is the `GameAudio` object: `engine` (voices, buses,
`stats`, `culled` among them: one-shots asked for out of earshot),
`engine.renderer` (the graph; its `dsp` is the WebAssembly module, built
from `rust/audio`, shared by the output and optional meter worklets. `engine`
and `master` describe the active Rust graph; `streams` (`queues`: lines
waiting) and the voice counts describe its inputs. There is one audio
implementation. A required module that fails to compile, instantiate or
render produces a visible player error.

## How it fits together

The battle engine is in `src/audio/`; `src/shell/audio.js` and
`loading-audio.js` select the front end's sounds. Gameplay code reports discrete events
through `events.js` (one-line hooks); the rest is read off the game's objects
each frame, as the original's entities poll their own state.

| File | Does |
| --- | --- |
| `game.js` | Loads a battle's sound, maps events to sounds, runs each frame. |
| `data.js`, `hash.js`, `props.js` | The extracted data: objects by name hash, `Inherit` as the library reads it, property defaults, the gates and the dice. |
| `engine.js` | The voice engine: 64 virtual and 32 heard voices, scored and stolen as the library does; buses and ducking; distance, Doppler, the listener. |
| `automation.js` | Params steered once a frame, without re-sending targets that have settled. |
| `streams.js`, `stream-worker.js`, `adpcm.js` | Stream slots, segment picks, the voice-over queue (lines scored as the library scores them, a soldier's line 3D where he said it); segments fetched by byte range and decoded in a worker. |
| `player.js` | The player's unit: weapons' three voices, footsteps (both detectors), foley, voice, jetpack, the droideka. |
| `soldiers.js`, `weapons.js` | Every other soldier's weapons (shots, the fire loop, reloads, overheats) and voice (hurt, death), by the player's own rules, which `weapons.js` and `soldierVoice()` share. |
| `chatter.js` | The Classic AI's chatter: a "vo" record's line from the speaker's class, queued 3D where he said it, or the player's own command at once, 2D. |
| `scene.js` | Rounds in flight, command posts, recharge droids, turrets' guns, mines, recon droids, other soldiers' charges. |
| `vehicles.js` | Every hull's engine, turning on and off, flight, hurt, death, crashes and feet, and every turret's activation, whirs and deactivation (a hull's turret seats and the placed turrets, whoever mans them). |
| `music.js`, `vo.js`, `ambience.js` | The music state machine, voice-over, the map's emitters and stream regions. |
| `spatial.js`, `layouts.js` | The output graph: layouts, panning laws, beds, the LFE, reverb send, master chain. |
| `acoustics.js`, `mixer.js` | Enhanced only: air, occlusion, rooms; the loudness window. |
| `enhanced.js` | The upscaled assets, loaded in the background. |
| `panel.js`, `options.js` | The panel and the settings' choices. |
| `worklets/` | Thin required-module hosts: `output` copies the Rust render quantum to the device; `frontend` copies the menu/loading output; `meter` copies PCM in and loudness readings out. |
| `rust/audio/src/output.rs` | Owns voice and stream playback, placement, convolution, room sends, mono and the complete master phase. |
| `frontend.js`, `rust/audio/src/frontend.rs` | Menu music, whooshes, interface sounds and loading pulses: timed playback, resampling, bus gains and linear fades in Rust. |
| `decode.js` | The temporary, single `decodeAudioData` boundary for compressed asset I/O. |

Each frame: the listener follows the camera; the player and scene modules
start, stop and steer their sounds; voices are scored and the top 32 play;
each voice's direction, filters and sends are batched to the worklets in
one message.

## The original mode

What is the game's, exactly (see `docs/formats.md`, "Sound"):

- Which sound plays for what, and when: every trigger listed there,
  including the two footstep detectors, the weapons' voices (every
  soldier's, the bots' and other players' as the player's own), the
  soldiers' hurt and death voices, command posts, turrets, droids, the
  music's stages, the voice-over's lines and queue, and the AI's chatter
  (3D where it was said, on the commander's stream and queue).
- A one-shot out of earshot where it is asked for (3D, at or past its
  `MuteDistance`) doesn't start, as the library's entity sounds don't: a far
  battle takes no voices from a near one.
- Each sound's properties: defaults, `Inherit`, gain and pitch dice,
  `PlayInterval` and `PlayProbability`, sample picks by weight or in turn.
- Distance gain by the library's curve, plus the OpenAL output's extra
  falloff past half of `MaxDistance`; 64 virtual and 32 heard voices,
  scored (1 − `Bias`) × level + `Bias` × `Priority`; bus gains and the
  voice-over's duck of `main` to 0.5 over 0.5 s; Doppler as 1 + Δ/344, none
  at 172 m/s; the listener at the camera, 2 m ahead for distance; the
  sub-level's space for reverb (Theed's is dry); pitch glides of 1/256 twice
  a frame; 2D sounds always centred (their `Pan` never arrives).
- The output, OpenAL Soft 1.20.1 as the Steam build ships it: its 2D
  ambisonic decoders on stereo (first order, front azimuths widened 1.5
  times), 5.1 (second order) and 7.1 (third), streams' channels at its fixed
  angles (stereo ±30 degrees, a quad's rear pair ±150), and on surround its
  front stabilizer, which the build's `alsoft.ini` turns on and which alone
  feeds the centre. No LFE.
- Streams decode with the original game's ADPCM step rule (shift and add),
  not the multiplying one other decoders use.

What stands in for something:

- **Headphones.** OpenAL Soft turns on its HRTF (the MIT KEMAR set) when
  Windows reports headphones, and pans every source and stream channel
  through it. The port uses the Rust IRCAM HRTF decoder (Chromium's is the
  IRCAM LISTEN composite), brought to the same level as a source ahead on
  speakers. If Windows didn't report your headphones, the original played
  its stereo panning: choose Stereo instead.
- **The limiter.** OpenAL Soft's own output limiter is replaced by the
  master chain's (true peak at −1 dBTP), and the dynamics profile applies
  to both modes. Home theater leaves the mix as it was. The light mix has a
  soft clip instead, and the profile is a level there.
- **Level.** The original mode is raised by the power OpenAL Soft's
  decoders give a source ahead (1.5 dB on stereo, 3.3 on 5.1, 0.9 on 7.1),
  so the two modes match in loudness. Nothing else in the mix changes.
- **7.1.4** is beyond OpenAL Soft 1.20.1: the original plays 7.1, the
  heights silent.
- **Playback resampling** follows Chromium's linear interpolation in Rust for
  every voice and stream. It does not use
  OpenAL Soft's resampler.

Footsteps: the two detectors keep the original game's rules for when a step
is due (a foot rising from its plant; the stride as the fallback), but the
port's animation isn't the game's (blended clips whose planted foot wobbles
a few millimetres a frame), and run literally they gave two or three steps
a footfall. So a foot must lift 5 cm above its last plant before it steps
again, and the stride steps only while the feet haven't planted for 0.75 s.
The original mode keeps the game's steps in the air (its stride
counter runs in every state, on the full speed, so a jetpack flight steps
every 1.75 m); the enhanced mode steps only on the ground.

Leaving the battlefield plays the mission's `SetOutOfBoundsVoiceOver` line
for the player's team ("repleaving" on a Clone Wars Bespin) on the VO
stream, once each time he goes out (src/battlefield.js, docs/formats.md
"Leaving the battlefield"); the page that shows him plays it, in every mode.

Not in the port yet, because the port lacks what triggers them: the spawn
screen's sounds, Galactic Conquest's bonus lines, and `LowHealthSound`
(only the heroes have it, and they are AI-only in the 2004 game).
`ClothingRustleSound` is played as the original plays it, though no class
on Theed sets one. Other soldiers' feet and foley aren't played (the
player's are).

Online, other players' and the server's bots' shots, hurts and deaths play
(from their rounds' launches and their replicated health), but not their
reloads or overheats (their weapons' states aren't sent), and not the
bots' chatter: the AI's "vo" records stay on the server.

### The light mix (stereo)

On stereo speakers (and Automatic on a stereo device), Classic uses the
light mix in `rust/audio/src/output.rs`. It keeps the same sound choices,
distance curve, Doppler, pitch glides and voice ranking.

- Rust mixes each voice through OpenAL Soft's first-order stereo gains.
  Both gains ease over 15 ms, the first value applies immediately, and
  changes of a thousandth or less are held.
- Rust places stream channels at OpenAL Soft's angles: stereo hard left
  and right, mono ahead, and a quad bed's rear pair spread across both ears.
- The master applies the original stereo level (+1.5 dB), optional mono,
  the dynamics profile's level (Home theater −10.2 dB, Normal −6.1,
  Night −5.7), the 4097-entry soft-clip curve and master volume.
- This mode has no reverb or compressor. It uses the same single Rust
  output worklet as every other mode.

Against the full original output on stereo it drops the sub-level's reverb
(Theed, the Dune Sea and Endor's are dry; on the other maps it is the
reverb a PC without EAX hardware never had), the dynamics' compression
(Night no longer narrows the range there), the true-peak limiter, and its
1.5 ms of lookahead. The rest renders the same: the same 16 moving voices
through both, aligned for that lookahead, differ by 0.01 dB a channel, the
residual 47 dB under the signal (the old mixer moved a voice's panning
within one block, the light mix over 15 ms).

On surround and headphones the original keeps OpenAL Soft's output as
above. Settings → Audio → Classic, Output Stereo picks the light mix on any
device.

A first session (no settings stored) on a low-end CPU starts on Classic:
four threads or fewer and, as Chromium reports it, 4 GB of memory or less
(`src/cpu-info.js` `lowEndCpu`, `Settings.adoptHardwareDefaults`), which is
the A8 laptop above and a 2011 quad-core with 4 GB; current PCs keep
Enhanced. A choice the player makes stays theirs, and settings already
stored aren't changed: there the SOUND row says that Classic is much
lighter on that CPU.

## The enhanced mode

### Output

The browser outputs discrete PCM channels, in the order Chromium uses:
stereo L R; 5.1 L R C LFE SL SR; 7.1 FL FR FC LFE BL BR SL SR; 7.1.4 the
7.1 and then TFL TFR TBL TBR (Chromium 153 and later). The speaker test
checks the order on your setup. There is no Dolby or DTS bitstream: a
receiver shows multichannel PCM, never "Dolby Digital" or "Atmos" from the
game itself.

Getting the channels to the browser:

- **Windows.** Set the output device's speaker setup (Sound control panel →
  Configure) to 5.1 or 7.1; HDMI to a receiver then carries multichannel
  PCM. With Windows Sonic or Dolby Atmos for Headphones on, Windows offers
  7.1 and renders it for headphones itself: either turn it off and choose
  Headphones here (the dummy-head decoder), or keep it and choose 7.1, not
  both. With Dolby Atmos for home theater on, Windows encodes its whole mix
  for the receiver, and the game's channels are a bed in it, not objects.
  (From the documentation; not tried on hardware.)
- **macOS** (experimental, not supported yet). Audio MIDI Setup → the
  device → Configure Speakers.
- **Linux.** Chromium reports the channel count of the PulseAudio (or
  pipewire-pulse) default sample spec, which is usually stereo even on a
  surround card. Set the card's profile to a surround one and the default
  position, for example `pulse.properties = { pulse.default.position = [ FL
  FR FC LFE RL RR SL SR ] }` in a `pipewire-pulse.conf.d` file. (From
  PipeWire's documentation; not tried here.)

### Speakers

- **Panning**: on stereo, equal power by the sine of the angle off the
  front (front and back alike, so nothing behind collapses to the middle);
  on 5.1 and up, pairwise amplitude panning (VBAP, Pulkki 1997) over the
  layout's ring, power-normalized, and on 7.1.4 the energy splits between
  the ear-level and height rings by elevation. The centre speaker is part
  of the ring, so anything ahead anchors on it rather than on a phantom
  image (OpenAL Soft's front stabilizer does this for the original; here
  the panning does it directly).
- **Spread**: within 1.5 m a source widens toward every speaker (up to 60%),
  so the player's own sounds don't snap between speakers.
- **Voice-over** plays from the centre. **Music** plays the enhancer's 5.1
  or 7.1 upmix when there is one (its centre carries the middle of the
  stereo, 6 dB down, out of the dialogue's way; the surrounds the
  decorrelated ambience), else stereo on the fronts with a light live upmix
  of the side signal to the surrounds. **Ambience**: a quad bed's rear pair
  on the surrounds (on 7.1, between the sides and the backs).
- **LFE**: explosions send half their level to the LFE, low-passed at 120 Hz
  (24 dB/octave). Nothing else does; the main channels keep their own lows
  for the receiver's bass management.

### Headphones

Every 3D voice and the ambience bed's four channels are encoded into
third-order ambisonics (16 channels) in the spatial mixer, and the mix is
decoded to the ears through a measured dummy head: SADIE II's Neumann KU
100 (subject D1, 8802 directions at 48 kHz, University of York), by
magnitude least squares (Schörkhuber, Zaunschirm and Höldrich 2018;
spaudiopy's implementation). That is 16 filters per ear, 256 taps, run by
16 ConvolverNodes however many voices play. 2D sounds and music stay plain
stereo.

`tools/make_binaural.py` keeps both ears' filters (the decoder's filters
for the two ears aren't mirror images channel by channel), scales them so a
source ahead plays at −3 dB in each ear between 200 Hz and 4 kHz (a centred
sound's level on stereo speakers), and writes the decoder's and the head's
own levels for a few directions. Decoded against measured, in dB (left /
right ear):

| Source | 1 kHz decoded | 1 kHz head | 4 kHz decoded | 4 kHz head |
| --- | --- | --- | --- | --- |
| ahead | −3.3 / −2.9 | −2.7 / −2.3 | −2.1 / −1.1 | −1.8 / −0.7 |
| left | 1.9 / −4.1 | 2.3 / −5.0 | −0.5 / −14.0 | 0.2 / −16.0 |
| 45° left | 0.6 / −11.5 | 0.8 / −11.5 | 4.0 / −9.8 | 3.7 / −12.0 |
| behind | −2.7 / −3.0 | −2.6 / −2.8 | −4.4 / −5.0 | −3.7 / −4.4 |
| 60° up | −4.9 / −5.0 | −4.9 / −3.4 | −2.9 / −2.4 | −3.2 / −2.6 |

Third order follows the head to within about 2.5 dB up to 4 kHz (the file
also has 500 Hz and 2 kHz), except deep in the far ear's shadow, which it
fills in by up to 6.5 dB at 4 kHz. The decoder chain
reproduces the decoder's levels to within 0.3 dB at 22.05, 44.1, 48 and 96
kHz (decoding resamples the filters to the context's rate, which scales
their gain; the loader undoes that).

Without the filters, each 3D voice uses Rust's IRCAM HRTF decoder, at
about twice the cost with 32 voices and 2.6 times with 64 (below).

### The listener

The listener faces where the camera faces, so on-screen left is left. For
distance it sits 2 m ahead of the camera, the original's rule. In third
person that point lies between the camera and the player, so the player's
own sounds play close and in front, and a distant source's level is as the
original sets it.

### Distance, air, occlusion, Doppler

- Distance gain is the original's curve, without the extra falloff the
  Steam build's OpenAL output adds past half of `MaxDistance` (a side effect
  of its reference distance, not part of the 2004 game's own outputs).
- **Air**: a low-pass at 20 kHz out to 20 m, then falling, to 9 kHz at
  100 m and 4 kHz at 300 m (never under 2.5 kHz): about half of what
  ISO 9613-1 gives at 20 °C and 50% humidity, so a battle across the plaza
  keeps its edge.
- **Occlusion**: three rays from the listener to each heard voice, six rays
  a frame shared out in turn (each voice is looked at again every few
  frames), through the map's collision (the same BVH as the game's), aimed
  a metre above the voice's origin (a soldier's origin is at
  his feet, where the ground's bumps would block nearly everything) and
  half a metre to either side. The share blocked eases the voice toward
  −4.4 dB and a low-pass near 2.8 kHz, muffled rather than buried; voices
  within 4 m are never occluded. Its reverb send isn't occluded, so a
  firefight behind a building still fills the street. (A single ray to the
  feet, −6 dB and 1.1 kHz, had half of Theed's battle sounding as if
  through a wall: the median corner of the voices heard was 1.2 kHz; it is
  now 11 kHz.)
- **Doppler** is the original's.

### Rooms

A fan of 19 rays from the listener, cast two a frame (the whole fan about
six times a second, with no frame paying for all of it), measures the space
(walls within 60 m, how far, a roof or not) and sets the reverb. Walls give
early reflections; a roof gives the long, wet tail: under Theed's arcades a
dense room of about a second, in the plaza (ringed by buildings but open
above) reflections and a fainter, shorter tail. The send is high-passed at
180 Hz, so rooms add space without boom. The reverb is a feedback delay network (8 lines, a Householder
matrix, damping per line for `DecayHFRatio`, input diffusers, early
reflections) with each speaker reading its own mix of the lines, so the
tail is decorrelated across the room. Each voice's send is the room's,
scaled by its sound's `ReverbGain`.

### The mix

The game's buses and the voice-over duck stay as they are. On top, a
loudness window in the manner of "HDR audio": the loudest sound playing
sets the top, and quieter kinds make room under it. A blast close by pulls
the ambience down by up to 6 dB and foley by up to 3 dB while it rings,
then lets them back at 8 dB a second. The voice limit (32 heard of 64) and
its ranking are the original's.

### Clarity

The game's mix, measured over a battle, has its most energy at 125 to 250
Hz and almost none above 11 kHz (most samples are 22 kHz, the music and
ambience 4-bit ADPCM): it sounds dull next to a 2026 mix, however clean the
rendering. The enhanced mode's master chain opens with a clarity stage
(on by default; Settings → Audio → Clarity):

- **EQ**: −2 dB around 220 Hz (where many sounds at once pile up), +1.5 dB
  presence around 3.2 kHz, +3 dB air above 8.5 kHz.
- **Attacks** (`rust/audio/src/clarity.rs`): two envelopes of the whole mix's
  power, fast (1 ms up, 40 ms down) and slow (25 ms up, 250 ms down), linked
  across the channels so the image holds. Where the fast one stands over the
  slow one an attack is under way, and the gain rises by a quarter of the
  difference past the first dB, up to +4 dB, easing back over 60 ms. Shots
  and footsteps land harder; tails, beds and music pads stay where they
  were, so the range between them grows.
- **A top octave**: per channel, the 2.5 to 7 kHz band full-wave rectified
  (its even harmonics, an octave up and more), high-passed at 7 kHz and
  mixed back in at −12 dB. It rises and falls with each sound, in time with
  its attacks.
- The stage's output is trimmed 0.5 dB to keep the A/B with the original
  level. The LFE passes untouched.

Over the same 45 s battle (Enhanced, with clarity against without,
measured where the mix enters the dynamics): spectral centroid 1856 → 2196
Hz; energy above 8 kHz −22.3 → −19.6 dB of the total, the 16 kHz octave
−30.9 → −27.3 dB; the 250 Hz octave −7.4 → −8.8 dB; peak-to-loudness ratio
17.9 → 19.7 dB; loudness range 15.5 → 16.5 LU.

### Master chain and loudness

The clarity stage, the dynamics (one of three profiles), then a true-peak
limiter (4× oversampled peak detection, 1.5 ms lookahead, linked channels)
at −1 dBTP, then the master volume.

| Profile | Compression | Target | Measured enhanced / original | Loudness range |
| --- | --- | --- | --- | --- |
| Home theater | none | −24 LUFS | −23.4 / −23.8 | 13 / 12 LU |
| Normal | 2:1 over −18 dB, 25 ms attack | −20 LUFS | −19.6 / −19.7 | 10 / 8 LU |
| Night | 5:1 over −30 dB | −18 LUFS | −18.8 / −18.5 | 5 / 4 LU |

The targets are ASWG-R001's (−24 ±2 LKFS for the home, −18 for portable
devices; true peak at most −1 dBTP). Measured with
`tests/audio-loudness.mjs` on the same 60 s of Theed in each mode (a seeded
battle: walking, turning, firing in bursts among the AI's fighting),
integrated loudness gated per BS.1770-4, loudness range per EBU Tech 3342.
True peak stayed at or under −1 dBTP throughout. The meter in the panel is
the same BS.1770 meter (K-weighting, 400 ms blocks, absolute and relative
gates).

## Runtime ownership

One AudioContext supplies the clock and device. The page resumes it on the
first gesture, suspends it while hidden, selects assets and sends timed
commands. One zero-input AudioWorklet owns a required Rust module. Every
render quantum calls `output_process` once and copies the finished PCM to
the device. Its twelve discrete output lanes allow layout changes without
replacing the live host; unused lanes are zero and the device truncates them.

Rust owns PCM normalization and loading-time gain compensation as well as
sample cursors, playback-rate interpolation, automation, gain,
filters, panning, stream routing and upmix delays, both headphone decoders,
reverb, EQ, clarity, mono, dynamics, limiting and volume. Classic surround
uses OpenAL Soft's decoder and front stabilizer. Classic headphones and
Enhanced headphones without the optional dummy-head filters use the IRCAM
HRTF database. Its kernel preparation, interaural delays and crossfades run
in Rust. The dummy-head path uses 32 partitioned ear convolutions.
The browser FFT uses WebAssembly SIMD for adjacent complex butterflies.
The front end keeps an all-mono mix in one lane until the final stereo copy.

Mono keeps the frozen routing, including its existing 7.1/7.1.4 behavior:
the first channel is copied to every output. Stereo and 5.1 use their
recorded downmix coefficients. Changing this is a separate audible change.

The meter is an optional second thin host for Rust's BS.1770 meter.
Speaker-panel signals are generated by Rust and played through Rust lanes;
the speaker test targets a device channel after the muted game mix.
There are no browser source, gain, filter, panner, convolution, delay,
shaper, splitter or merger DSP nodes in the game, menus or loading screen.
The front end uses its own two-channel Rust host on its context, retaining
its music and shellfx gain stages and linear fades. Music is fetched and
decoded one track at a time, rather than played by an HTML audio element;
this keeps playback and resampling inside Rust but holds that track's
decoded PCM in memory until it ends or is replaced.

Sources, filters, delay lines, FFT buffers and render scratch are reserved
before their first block. PCM and command messages may allocate between
blocks. Stream segments use three reusable sample slots per lane; finished
lanes release their PCM. Meter integrated history reserves another hour
between blocks, requested with one minute of capacity remaining.

### Temporary host codec I/O

`src/audio/decode.js: decodeAudioAsset(context, bytes)` is the only
`decodeAudioData` call. It remains for this branch; the next decoder work
moves FLAC first, then Opus, into Rust. Xbox IMA ADPCM already decodes in
Rust in the stream loader worker. AudioBuffers hold decoded PCM only.

Host decoding currently returns **context-rate PCM**, and rejects a result
at any other rate. This affects menu tracks, enhanced sample and segment frame counts,
durations, loop/offset timing and the playback-rate scale. Headphone HRIRs
are decoded at that rate too; their gain is compensated by the source-rate
to context-rate ratio. A replacement decoder must preserve these contracts
or explicitly own the same conversion before playback. The runtime's linear
playback resampler is separate from this asset conversion.

The IRCAM resource generator also records a signed-16 host detail: negative
samples multiply by float32 `1/32768`, nonnegative samples by float32
`1/32767`. Double-precision division changes some HRTF kernel preparation
results. This observation covers that FLAC resource; FLAC assets with other
bit depths and Opus need their own bit-exact codec fixtures. Regenerate it
with `node tools/audio-hrtf.mjs path/to/Composite.flac --check`.

## Recorded render gates

`tests/fixtures/audio` contains synthetic reference renders frozen before
the old graph was removed. Its manifest records the source revision and
Chromium version. No game assets are included. The graph fixture exercises
both quality modes, stereo, 5.1, 7.1, 7.1.4 and headphones, each with mono
on/off; headphones cover both IRCAM HRTF and a synthetic dummy-head decoder.
Additional stream-only cases schedule three overlapping segments at 22.05,
44.1 and 48 kHz with offsets, fractional playback rates, gain automation and
stop times. They cover placed voice-over and 6/8-channel bed routing.
Twelve moving voices and quad ambience, stereo music and mono voice-over
beds exercise routing, room changes and volume automation. Strict cases
cover two-voice stereo and full voice lifetimes, including one-shot end,
fade stop and starting a loop partway through after entering earshot.
Menu/loading goldens at 44.1 and 48 kHz cover stereo music, mono effects,
bus dips, pitched pulses joined late, scheduled end fades and screen exits
that replace a pulse fade mid-curve.

Strict cases compare SHA-256 of the float32 bytes. Other cases require every
sample's difference from the frozen native render to stay below −96 dBFS.
Component goldens cover 3,289,466 samples and readings exactly. Separate
recorded HRTF impulses cover 44.1 and 48 kHz, azimuth and elevation. Rust
allocation tests count allocations from the first block, including stream,
convolution, front-end playback and meter report paths.

`tests/audio-bench.mjs` measures the complete live engine over generated
voices and beds using a running Chromium debug endpoint. It no longer
builds isolated browser/worklet DSP graphs. Its header gives the endpoint,
mode, warm-up and measurement options. Use calibrated local slots when
comparing revisions; a reference run uses its own frozen build.

Offline rendering proves samples, not real-time cost: it never reaches the
same optimizing tiers as a live context. CPU measurements must use a live
context after warm-up, with the frozen dist and candidate running the same
synthetic scene in alternating order. Count all audio threads and verify
nonzero output and the requested layout before measuring. Hardware listening
still needs to check device channel order, headphone placement, mode changes
and glitches during a battle.

## Upscaling (`tools/enhance_audio.py`)

Offline, local, repeatable (pinned packages, no randomness). Each sample
and stream segment goes through: decode exactly as the game does; de-click
(voice, music and ambience only) and a gentle de-noise (spectral gating
under the quietest frames' floor, at most 6 dB, only where the floor is
above −60 dBFS); resampling to 48 kHz with soxr at very high quality (loops
as loops); bandwidth extension for sources under 40 kHz (the top octave
transposed up an octave in the STFT domain, laid in above the old band edge
3 dB under the spectrum's slope; nothing added below the edge); a light EQ
by kind; the level set back so the K-weighted energy equals the original's
(the game's own gains keep doing the mixing); loops kept seamless and cut-off
one-shots given a 10 ms fade. Samples are written as 24-bit FLAC, streams as
Opus (96 to 576 kb/s by channel count, discrete channel mapping above two
channels, which Chromium decodes in order). Music also gets 5.1 and 7.1
upmixes by primary/ambient extraction (Avendano and Jot 2004); the stereo
stays for stereo and headphones.

Guardrails: nothing new is composed. Every item is compared with its
original at 48 kHz: the 10 ms loudness envelopes must correlate at 0.98 or
better (or, for a steady sound, differ by at most 0.5 dB in the median
frame), the spectrum below the old band edge must stay within 1.5 dB, and
what was added above it must stay under its budget. An item that fails
plays its original. On Theed 505 of 543 samples pass and 38 fall back
(mostly short foley transients, whose envelope moves 1 to 3 dB once the top
octave is filled in, and a few voice lines), and 402 of 403 stream segments
pass.

Machine-learning bandwidth extension was evaluated and not used: no model
was both licensed for this and practical on a CPU. UniverSR (MIT code, CC
BY 4.0 weights) took about 500 times a sound's length per mono channel here;
AP-BWE (MIT) is for speech only; AudioSR needs an old NumPy and Python and
6.2 GB; FlashSR has no license; Apollo is CC BY-SA and for MP3 restoration.
`tools/models/` (gitignored) is where weights would go.

## Licenses

| Component | Used for | License |
| --- | --- | --- |
| OpenAL Soft 1.20.1 | decoder coefficients (`src/audio/layouts.js`) and its band splitter and front stabilizer, translated to Rust (`rust/audio/src/stabilizer.rs`), for the original mode | LGPL-2.0-or-later; see `THIRD-PARTY-NOTICES.md` |
| SADIE II database, subject D1 (University of York) | the headphone decoder's HRIRs | Apache-2.0 |
| spaudiopy 0.2.0 | MagLS decoder design | MIT |
| NumPy 2.5.3, SciPy 1.18.1 | DSP in the tools | BSD-3-Clause |
| soundfile 0.14.0 (libsndfile) | WAV and FLAC I/O | BSD-3-Clause (libsndfile LGPL-2.1) |
| soxr 1.1.0 (libsoxr) | resampling | LGPL-2.1-or-later |
| h5py 3.16.0 | reading the SOFA file | BSD-3-Clause |
| matplotlib 3.11.2, resampy 0.4.3, sounddevice 0.5.6 | imported by spaudiopy | Matplotlib License (PSF-based), ISC, MIT |
| FFmpeg with libopus | Opus encoding (the tools run it as a program; desktop builds redistribute a static FFmpeg build) | LGPL-2.1-or-later or GPL by build; Opus BSD-3-Clause; see `THIRD-PARTY-NOTICES.md` |

The game's sound is the user's own copy and stays out of git.

## Tests

No game assets are needed for the recorded gates:

```sh
node --test tests/audio-*.test.js
cargo test --manifest-path rust/Cargo.toml -p audio
node tools/build-rust.mjs audio --check
```

The Node render host supplies PCM storage and context scheduling only. It
runs the real page command handoff, output worklet and Rust module against
the recorded fixtures, so it runs in CI. The component golden gate is also
available directly as `node tests/audio-dsp-parity.mjs`.

For the actual browser host, start Vite and run Chromium offline:

```sh
AUDIO_TEST_URL=http://127.0.0.1:18449 node tests/audio-engine-gate.mjs
AUDIO_TEST_URL=http://127.0.0.1:18449 node tests/audio.mjs
```

Set `PLAYWRIGHT_MODULE` if Playwright is installed outside the project.
`OUTPUT` and `QUALITY` select a subset. Both entrypoints use
`tests/audio-checks.js` and the same goldens. A failed module or silent
render fails the gate. `tests/audio-loudness.mjs` uses extracted Theed assets
to measure live final-output Rust meter readings for each dynamics profile.
It no longer records a browser DSP reference or rebuilds the mix in nodes.

## Listening checklist

Things only a person with the hardware can check, in rough order:

1. **Speaker order** (5.1, 7.1 or 7.1.4): in the audio panel, play each
   speaker and note whether the named one sounds. "around the room" should
   circle once, smoothly, counter-clockwise from the front.
2. **Original against enhanced** on the same output: switch in Settings →
   Audio while standing in Theed's plaza and while firing. The level should
   stay about the same; the enhanced should sound wider, clearer at the
   top, and placed.
3. **Headphones**: choose Headphones. A clone firing to your left should be
   outside your head, left; turning the camera should move it smoothly. The
   music and interface sounds stay inside the head (stereo).
4. **Rooms and occlusion** (enhanced): walk under the arcades and back out
   to the plaza (the reverb tightens and wets, then opens out); put a
   building between you and a firefight (it dulls and drops).
5. **LFE** (enhanced, 5.1 or 7.1): a thermal detonator close by should reach
   the subwoofer; gunfire should not.
6. **Night profile**: quiet sounds carry and blasts don't jump out.
7. **Clarity**: toggle Settings → Audio → Clarity (enhanced) mid-firefight.
   On, shots should crack and land harder, with more air on top; nothing
   should hiss, sound harsh, or pump.
8. **Upscaled sound**: footsteps, blaster fire and voice-over should keep
   their character, just fuller at the top; anything that sounds smeared,
   metallic, or louder than before is worth a note (the panel lists the
   voices playing; the manifest lists each file with its checks).
9. **The light mix** (Classic, Output Stereo): on Theed it should sound as
   the original did on stereo before it (the same levels and placing); on a
   map whose original space has a reverb (Kamino, Bespin) it is dry. Moving
   sources should pan smoothly, and the loudest moments shouldn't crackle.
10. **Menus and loading**: listen to the first track starting, rapid menu
    changes during a music dip, track endings and the loading pulses. Leave
    the loading screen during a pulse's end fade; it should end cleanly.

## Open questions

- Which of the library's five mix configurations a player's profile uses
  isn't known; the port follows the OpenAL one, which the Steam build's
  output implies.
- The listener sits at the controlled unit rather than the camera when a
  flag on the player says so; which flag that is isn't known.
- `RunSpeed` is never set, so every footstep uses the run sounds; whether
  that was intended can't be known, only that it is what plays.
- A multichannel stream placed in the world is set by distance only; the
  OpenAL Soft source confirms its channels keep their fixed angles, but
  how the library itself handles it isn't known. A soldier's chatter line (a
  mono 3D stream) is panned as his other sounds are.
- Where a queued voice-over line's tie goes (ahead of or behind an equal
  score) isn't known; the port puts it behind.
