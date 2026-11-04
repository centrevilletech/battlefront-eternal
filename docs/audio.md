# Audio

The port's sound has two modes, switched live in Settings → Audio:

- **Original (2004)** (CLASSIC in the menus) plays what the Steam build
  plays, when and how it plays it: the same sounds from the same archives,
  picked, gated, mixed, and placed by the game's sound library's rules,
  through what the build's OpenAL Soft 1.20.1 output does with them.
  `docs/formats.md` ("Sound") has the rules. On stereo speakers it plays
  through the light mix (below): the same mix without the reverb, summed by
  the browser's own nodes, so a slow PC's audio thread can afford it.
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
the headphone filters it uses the browser's HRTF on headphones.

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
from `rust/audio`, that the worklets run, or null with `?audio=js`, which
runs their JavaScript instead: the same samples to the bit, for A/B; its
`engine` is whether the voices play in the module's voice engine, as they
do unless `?voices=native`, and `master` whether the chain after them does, unless `?master=native`: see "The voice engine"), `streams` (`queues`: the lines waiting),
`music`, `vo`, `player`, `soldiers`, `chatter`, `scene`, `ambience`,
`acoustics`, `data`, and `log` (sounds asked for that don't exist).

## How it fits together

Everything is in `src/audio/`. Gameplay code reports discrete events
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
| `worklets/` | `spatial-mixer` (every 3D voice's panning), `stabilizer` (OpenAL Soft's), `clarity` (the enhanced mode's attacks and top octave), `reverb` (an FDN), `limiter` (compressor and true-peak limiter), `meter` (BS.1770). |

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
  through it. The port uses the browser's HRTF panner (Chromium's is the
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
- **Resampling** is the browser's, not OpenAL Soft's.

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

On stereo speakers (and Automatic on a stereo device, which is every laptop)
the original plays through the light mix (`Renderer.buildLight` in
`src/audio/spatial.js`). A 2014 laptop with an A8-6410 (four 2 GHz cores
sharing 15 W with the GPU) spent 31 to 43% of a core on the audio thread in
a battle in the original's full output, at real-time priority, taking it
from the page and the world worker that already had none to spare. The
light mix keeps every decision the game makes (which sound, when, how loud,
the distance curve and the Steam build's falloff, Doppler, the pitch
glides, the 32 heard voices of 64 and how they are ranked and stolen) and
sums the result the way a PC of 2004 did, in the browser's own nodes:

- Each voice is its buffer source and two gains, one into each speaker,
  carrying its level times OpenAL Soft's first-order stereo law for its
  direction (front azimuths widened 1.5 times, as before): three nodes, as
  many as before, with no gain node of its own, no filter, no send and no
  mixer input. Both gains ease as the engine eases a level (15 ms), the
  first at once, and a change of a thousandth or less isn't sent.
- Streams play at OpenAL Soft's angles for their channels, in as few nodes
  as that takes: a stereo pair lands hard left and right, so a music stream
  goes straight into the mix; a mono line ahead through one gain into both
  speakers (0.5957 each); a quad bed's front pair straight to its speakers
  and its rear pair through a gain to each (0.667 on its own side, 0.167 on
  the other).
- The master: the mix at the original's stereo level (+1.5 dB), the mono
  fold if chosen, the dynamics profile as a level (Home theater −10.2 dB,
  Normal −6.1, Night −5.7), a soft clip (unity to −1 dBFS, then a knee that
  reaches full scale 6 dB over it), the master volume. With no compressor
  the levels are what put a battle at the enhanced mode's loudness:
  `tests/audio-loudness.mjs` on 60 s of Theed gave Home −22.9 LUFS, Normal
  −18.8, Night −18.4 against the enhanced mode's −21.8, −18.7 and −18.2,
  true peak at most −1.3 dBTP (the soft clip only guards), and a loudness
  range of 9.3 LU in every profile.
- No worklet, no reverb, no limiter. A context that never loads a worklet
  renders on the device's own audio thread, without a second real-time
  thread to hand each block across; the worklets load if the player later
  picks a graph that needs them (Enhanced, surround, headphones, or the
  audio panel's meter), the light mix playing until they are in.

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
fills in by up to 6.5 dB at 4 kHz. The browser's chain
reproduces the decoder's levels to within 0.3 dB at 22.05, 44.1, 48 and 96
kHz (decoding resamples the filters to the context's rate, which scales
their gain; the loader undoes that).

Without the filters, each 3D voice gets the browser's HRTF panner, at
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
- **Attacks** (`worklets/clarity.js`): two envelopes of the whole mix's
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

## Browser practice

- One AudioContext, created at load and resumed on the first click, key
  or touch; suspended while the page is hidden and resumed when it shows.
- The DSP runs in AudioWorklets (panning, stabilizer, reverb, dynamics,
  meter) and native nodes (filters, convolvers); the main thread only
  decides, and sends each frame's voice directions in one message. The
  light mix runs no worklet: its panning is two native gains a voice, set
  from the main thread.
- Per-battle decoding: only the sub-level's samples load (24 MB of PCM, or
  the enhanced FLACs in the background, six at a time). Streams are
  fetched by byte range per segment, decoded in a worker, and kept for at
  most 600 s of sound.

## Cost

`tests/audio-bench.mjs` renders offline in headless Chromium and reads the
render thread's own CPU time: the whole renderer per mode with 0, 32 and 64
voices that move every frame (as the engine moves them), each worklet
alone, and the engine's main-thread update. On this machine (8 cores,
shared with other work, so cells move by up to a fifth run to run), as a
percentage of one core, best of three, the version before the changes below
against this one, measured back to back:

| Rendering | 0 voices | 32 voices | 64 voices |
| --- | --- | --- | --- |
| Original, stereo | 10.0 → 4.2% | 38.3 → 29.2% | 59.2 → 47.5% |
| Original, 5.1 | 24.8 → 8.8% | 43.8 → 39.5% | 84.4 → 61.9% |
| Original, headphones (HRTF) | 12.7 → 3.5% | 113 → 128% | 230 → 233% |
| Enhanced, stereo | 14.6 → 7.2% | 59.4 → 35.7% | 112 → 52.4% |
| Enhanced, 5.1 | 31.4 → 11.9% | 80.3 → 35.9% | 125 → 77.5% |
| Enhanced, 7.1.4 | 32.3 → 18.7% | 80.4 → 44.0% | 112 → 82.5% |
| Enhanced, headphones (dummy-head decoder) | 26.8 → 15.8% | 65.9 → 57.9% | 117 → 77.2% |
| Enhanced, headphones (browser HRTF fallback) | 16.8 → 6.8% | 119 → 112% | 265 → 200% |

The game hears at most 32 voices. In the game itself (Theed, a scripted
battle, about 5 voices heard of some 50), the audio thread went from about
37% of a core to 25% in the enhanced mode, and from 24-31% to 18-20% in the
original. The engine's main-thread update with 48 voices went from about
240 to 140-160 µs; the acoustics' rays from 0.44 to 0.33 ms a frame.

What made the difference, all without changing the output (the original
mode renders bit for bit the same on speakers, and within rounding, -130
dBFS, on headphones; the enhanced mode within -105 dBFS, the spread
Chromium shows between two identical runs):

- The limiter checks true peaks only in blocks that could reach the
  ceiling, keeps its window's minimum in a running queue, and skips the
  compressor's logarithms under the knee: 2 channels 7.9 → 1.6%, 12
  channels 25 → 3.4%.
- The reverb runs a block at a time (its delay lines are all longer than a
  block) and not at all while silence goes in and it holds only zeros
  (Theed's original room, always): 12 channels 5.3 → 1.8%.
- The meter and the stabilizer work channel by channel, without
  allocations.
- A param's target is no longer sent again every frame once it has
  settled; while the browser has an automation event pending it works the
  param out sample by sample, and some nodes take a slower path for it
  (`src/audio/automation.js`).
- The enhanced mode's per-voice low-pass moves its corner once a block
  (2.7 ms) rather than every sample: several times cheaper while it glides,
  a waveform difference under -49 dB.
- The mixer has 64 inputs, not 96 (each costs a little every block, used or
  not), pans two channels a pass, and skips silent inputs; the engine ranks
  its voices without new lists every frame and caches bus gains and each
  voice's distance curve.

What still costs most: each voice's own native nodes (the buffer source,
its gain, its filter; on headphones without the decoder, an HRTF panner of
about 3.5% each), the headphone decoder's 16 convolvers (about 8% of a
core), and on 7.1.4 the meter while the audio panel shows it. A JavaScript
FFT convolver wouldn't beat the browser's convolvers; a WebAssembly one
might, and was left for later.

### The light mix

The original on stereo before the light mix (its full output: the spatial
mixer, the reverb and the limiter in worklets) against the light mix, on a
16-core Xeon E5-2690 v4 in a virtual machine shared with other work (single
runs move by up to half, so these are medians of runs alternating between
the two versions), as a percentage of one core:

| `tests/audio-bench.mjs`'s scene, offline at 48 kHz | Full output | Light mix |
| --- | --- | --- |
| A stereo music bed, no voices | 5.7% | 1.2% |
| and 8 voices moving every frame | 12.5% | 6.6% |
| and 16 | 18.4% | 11.6% |
| and 32 | 22.1% | 19.7% |

| A battle (the Classic AI, a scripted player, 20 s at 44.1 kHz) | Voices heard, streams | Full output | Light mix |
| --- | --- | --- | --- |
| Theed | 13, 2.5 | 24.5% | 16.7% |
| Kamino (the full output runs its reverb) | 4, 3.2 | 22.5% | 11.6% |

In the battles that is every audio thread of the page: the full output's
worklet thread and the device's (which hands each block across, about 2%),
the light mix's device thread alone. The engine's update on the main thread
(48 voices, 12 heard) went from about 196 to 115 µs: the light mix sends no
mixer batch and no reverb sends.

What is left is the voices themselves: the browser resamples a voice by
interpolating whenever it plays at any rate but its sample's own (the
game's samples are mostly 22.05 kHz), about 13 to 14% of a core for 32
voices at 48 kHz against 0.9% at their own rate, and in real time every
node costs something each block. Two ways further, measured and not taken:

- A context at 22.05 kHz, the rate of most of the game's samples: the
  light mix measured 10% of a core on Theed against 17%. But the music and
  ambience (48 and 44.1 kHz) have about −30 dB of their energy above 11
  kHz, which it would cut, and Theed's 52 samples at 44.1 kHz −29 dB (one
  −16 dB), which the browser's interpolation would fold back down unless
  they were first resampled with a low-pass; and a context's rate holds for
  its battle, so Enhanced chosen mid-battle would play at it too.
- Every voice resampled, levelled and panned in one AudioWorklet: 3.5 to
  3.9% of a core for 32 voices against 9 to 14% for the browser's buffer
  sources, the same linear interpolation. It moves the playing of every
  voice (starts, loops, offsets, fades, ends) into the worklet, a larger
  change, now under way: the enhanced mode's voices play that way behind
  a switch, and the light mix is next (see "The voice engine").

## The voice engine

The enhanced mode's voices on speakers play in the spatial mixer's
WebAssembly (`rust/audio/src/voices.rs`) instead of each through its own
browser nodes (a buffer source, a gain, a low-pass and a reverb send), and
the light mix's (Classic on stereo, the low-end default) play in one
worklet of their own, `voice-engine`, instead of a buffer source and two
speaker gains each. `?voices=native` in the address keeps the browser's
nodes, a developer's A/B switch: the two sound the same (below), and the
engine costs the audio thread about half as much. On headphones without
the dummy-head decoder the voices keep the browser's HRTF panners. A voice
that plays through a node of its own (a stream's line said somewhere, the
audio panel's test) keeps the browser's nodes beside the engine.

Unless `?master=native`, the enhanced mode's master chain follows the voices
into the same module (`rust/audio/src/master.rs`): the reverb and its
high-pass, the LFE's filters, the clarity EQ and stage, the limiter and
the volume, in place of three worklets and the browser's nodes between
them. One worklet's output is then the finished sound, and what still comes
through the browser's nodes (the stream beds, a voice that keeps them)
reaches it through two more inputs, the mixer needing only those and eight
for such voices, not one for each of its 64 slots. The binaural decoder's
convolvers and the mono fold keep the nodes. The stream beds stay in the
browser's nodes too, a source and two gains and, on stereo, as few routes
as they take: with the chain in the module three beds cost the audio thread
under a point of a core, too little to pay for moving decoded streams into
it. Not built yet, in this order: Classic on surround, and headphones.

The page still decides everything (ranking, distance, Doppler, levels,
air, sends). Instead of AudioParam calls it sends one message a frame of
commands, each carrying the context time its AudioParam call would have
had. The module plays each voice as Chromium's nodes compute it: the
buffer source's linear interpolation at a double-precision read position,
from its start frame to its stop frame (the start time's fraction of a
frame moving the read position on, a loop wrapping with its fraction, a
one-shot's last frame extrapolated from its last two); the AudioParam
timeline's
arithmetic for setValueAtTime, setTargetAtTime and cancelScheduledValues
(`rust/audio/src/timeline.rs`, including the four-values-at-a-time fill
Chromium's x86 build uses for setTargetAtTime); and the low-pass's
coefficients from fdlibm, as the browser's are. Each sample is posted to
the worklet once, and nothing allocates while it renders.

### Exactness

The engine must sound exactly as the browser's own nodes do, and is held
to this gate:

1. Each voice's chain is bit-identical to Chromium's nodes wherever
   Chromium reproduces its own output: one or two voices into an input,
   across automation, starts, stops, loops and playback rates.
2. With three or more voices into one input, the engine sums them in slot
   order. In the light mix its render may differ from a native render by
   no more than two native renders of the same scene differ from each
   other (the largest difference over several pairs of pages). In the
   enhanced mode every sample must stay within -96 dBFS of every native
   render, under the smallest step of 16-bit sound: below for why.

The second rule is there because Chromium's own output is not
reproducible in that case. A node input adds up its connections in the
order of a hash set of their outputs' addresses
(`AudioSummingJunction::outputs_` is a `HashSet<AudioNodeOutput*>`,
copied to `rendering_outputs_` in its order, which
`AudioNodeInput::SumAllConnections` follows). Floating-point addition is
not associative, so the order shows in the last bits, and it changes from
one graph to the next: one graph of three sources into one GainNode,
rendered eight times in one page, summed (a+b)+c five times and (a+c)+b
three times. Two pages rendering the same scene (Chromium offline on
Linux, each frame's messages applied before its block renders):

| Scene | Native against native | Engine against native |
| --- | --- | --- |
| Enhanced, reverb silent, 12 to 32 voices | identical | identical |
| Enhanced, reverb on, 2 voices | identical | identical |
| Enhanced, reverb on, 3 voices | -118.3 dBFS at most, -135.2 rms | -118.3 dBFS at most, -135.1 rms |
| Enhanced, reverb on, 24 voices | -109.5 dBFS at most, -130.5 rms | -111.4 dBFS at most, -130.8 rms |
| Light mix, 1 or 2 voices | identical | identical |
| Light mix, 3 voices | -132.5 dBFS at most | -132.5 dBFS at most |
| Light mix, 24 voices | -126.4 dBFS at most, -146.3 rms | -126.4 dBFS at most, -146.1 rms |

That spread is far below hearing and below 16-bit resolution (-96 dBFS).
The engine sums in a fixed order, so its own output is the same every
time.

The master chain in the module meets the same rules against the browser's
nodes and worklets, the voices in the engine on both sides. Every sum in
the chain has two terms on a channel, so without beds the two are bit for
bit the same: stereo, 5.1, 7.1 and 7.1.4, 3 to 32 voices, every dynamics
profile, clarity off, 44.1 kHz, and the room and the volume moving. A third
term into the mix (several beds) goes in Chromium's own order, and the
difference is then -110 dBFS at most. One thing Chromium does not reproduce
here: with a voice sounding in the very first block a graph renders, its
LFE filters take that block's input in about half its renders and miss it
in the rest, a difference gone within 0.4 s. The module takes it every
time, and the checks start their voices a few frames in.

The reference is Chromium on Linux on x86. Elsewhere the browser's nodes
compute some things differently (on macOS its biquads run through vDSP;
off x86 setTargetAtTime fills one value at a time), so there the engine
and the nodes can differ at the same rounding level.

`tests/audio-checks.js` holds the engine to the first rule, bit for bit:
twelve voices with the reverb silent, two with it on, and a voice's whole
life in both modes (a loop as the listener moves, a one-shot that ends, a
loop stopped with a fade, a loop that comes into earshot partway through
its sample and starts there).

They hold the master chain in the module to it too: stereo and 5.1, the
room and the volume moving, bit for bit, and beds with a voice that keeps
its nodes within -96 dBFS.

`tests/audio-engine-gate.mjs` measures the second, over several seeds (a
scene of 24 voices, each seed its own noise and listener's path), each
rendered with the browser's nodes in six pages and once with the engine.

- The light mix takes the rule as written: the engine's largest
  difference from any native render within the largest between any two
  of them. It lands exactly on the natives' own largest every time: 8, 24
  and 32 voices, -132.5, -126.4 and -126.4 dBFS; at 24 voices twelve
  seeds of twelve.
- In the enhanced mode the reverb spreads the rounding over its tail, so
  the largest difference is an extreme value that moves from seed to seed,
  and an extreme taken over more pairs runs larger: six natives make
  fifteen pairs among themselves, the engine against them only six, and
  judged by its six against their fifteen it came out above, by 0.1 to
  1.7 dB, in three runs of seven. An engine no different from one more
  native render would meet the natives' own largest only about half the
  time, on any comparison of extremes. So the rule here is a limit: sound
  plays on each player's own machine and feeds nothing back into the
  world, and a difference this far down is under the smallest step of
  16-bit sound, in bits Chromium does not reproduce itself. Over four
  runs of twelve seeds, with and without a battle's three beds and with
  the master chain in the module or not, every difference is below -107
  dBFS, 11 dB under the limit, and the natives' own pairs reach -107.5.
  The gate still reports the comparison like with like (six native pairs
  drawn at random from the seed against the engine's six, its largest
  within theirs, its median within 0.5 dB of theirs), and it moves with
  Chromium's own orders: from run to run the largest within the natives'
  in 4 to 8 seeds of the twelve, the medians in 4 to 7, both in 2 to 5.

### What it saves

The enhanced mode, offline in Chromium (32 voices moving every frame, 48
kHz, the render thread's own CPU): 339 ms a second of sound down to 138
on stereo, 313 to 198 on 5.1. The engine's update on the main thread with
32 voices went from 385 to 194 µs a frame. On the 2014 low-power laptop
the game must run well on (AMD A8-6410, Enhanced on stereo, battles on
Bespin Platforms and Hoth, native and engine runs in alternating order),
the AudioWorklet thread went from 72 to 74% of a core to 34% on Bespin
and from 68 to 37% on Hoth, the page's audio phase from 3.3-3.5 to
2.6-2.7 ms a frame, and the frame rate rose from 25 to 29-30 fps on
Bespin and from 24 to 26.5 on Hoth. Most of what is left on that thread is the worklets
themselves (the mixer with the engine is about half of that), then the
master chain's gains and filters and the stream beds' nodes.

The master chain in the module, on a live context (offline renders run too
fast for the module's code to reach its optimizing tier), 32 voices moving
and three beds on a desktop: the audio threads took 26.8% of a core with
the chain in nodes and 23.0 with it in the module. On the slow-PC emulator
(Enhanced on stereo, alternating runs), the AudioWorklet thread went from
28-30% to 23% on Bespin Platforms and from 27-28 to 24-26% on Theed, and
with the chain switched back and forth inside one battle the frame rate
stayed within its noise, 1 to 2 fps up if anything. The rest of that
thread is mostly the voices' own work, then the reverb, clarity and
limiter's, spread evenly enough that no one stage stands out.

The light mix, offline (the same scene): 110 ms a second of sound down to
58 with 32 voices, 46 to 34 with 8; with a single voice the worklet costs
more than the browser's three nodes (11 against 23 ms), the two meeting
at about four voices. On the laptop (Classic on stereo, the same battles
and order), the light mix's thread, the device's own, took 38% of a core;
with the engine the worklet thread takes 16 to 19% and the device's,
which only hands its blocks over then, 1.7%. The page's audio phase went
from 2.1-2.3 to 1.8-2.0 ms a frame and the frame rate rose by 1 to 2 fps
(28.4-29.9 to 30.5-30.8 on Bespin, 26.8 to 27.9 on Hoth).

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
| OpenAL Soft 1.20.1 | decoder coefficients (`src/audio/layouts.js`) and its band splitter and front stabilizer, translated to JavaScript (`src/audio/worklets/stabilizer.js`), for the original mode | LGPL-2.0-or-later; see `THIRD-PARTY-NOTICES.md` |
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

```
node --test tests/audio-units.test.js            # 20: hashing, ADPCM, distance, picks, graphs, panning, Doppler, music, voice-over, footsteps
node --test tests/audio-soldiers.test.js         # 9: other soldiers' weapons and voices, out of earshot, the chatter's queue and lines, online hurts
node --test tests/audio-light.test.js            # 14: the light mix's graph, panning law and soft clip; Enhanced's graph after a trip through it; worklets loaded only when needed; voices moving between graphs; the graphs with the voice engine and the master chain in the module
python3 -m unittest tests/test_zero_sound.py      # 5: the archive reader on made-up chunks
PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs AUDIO_TEST_URL=http://127.0.0.1:18449 node tests/audio.mjs
                                                  # 80 offline checks in Chromium (no game assets)
PLAYWRIGHT_MODULE=... AUDIO_TEST_URL=http://127.0.0.1:18449 node tests/audio-engine-gate.mjs
                                                  # the voice engine's gate, its second rule (VOICES, PAGES)
PLAYWRIGHT_MODULE=... GAME_URL=http://127.0.0.1:18449 node tests/audio-loudness.mjs
                                                  # the loudness table above (needs the extracted map)
PLAYWRIGHT_MODULE=... AUDIO_TEST_URL=http://127.0.0.1:18449 node tests/audio-bench.mjs
                                                  # the cost table above (BENCH_REPS, BENCH_SECONDS, BENCH_ONLY)
```

The browser runs are safest against a Vite server without hot reload: an
edit elsewhere reloads the page under a running test.

`tests/audio.mjs` runs `tests/audio-checks.js` in the browser on
OfflineAudioContexts with generated signals: every layout's panning at unit
power, the LFE route and its filter, voice-over on the centre, the limiter
on EBU Tech 3341's cases and a true peak between samples, the meter, the
reverb's decay time and decorrelation, the voice engine's ranking, limit,
bus gains and duck, OpenAL Soft's decoders against hand-worked gains, the
front stabilizer (its delay, a side signal untouched, a low centre sound a
third of the way to the centre), a 2D sound through the whole original
chain on 5.1, the light mix (voices where OpenAL Soft's stereo law puts
them, with no worklet; streams at its angles; overs clipped softly; an
engine voice's level on its speaker gains), the clarity stage (a top octave in proportion, nothing added
under its band, an attack lifted while a steady bed stays put, the LFE
untouched), the voice engine against the browser's own nodes (bit for
bit: twelve voices moving with the reverb silent, two with it on, a voice's
whole life in the enhanced mode and the light mix, and a voice that keeps
its nodes beside it), the master chain in the module against the browser's
nodes, and the headphone decoder (the encoder's harmonics against
spaudiopy's, spread keeping the energy, each ear's level per direction
against the decoder's own, a 2D sound left as stereo). The decoder checks
are skipped when `tools/make_binaural.py` hasn't been run.

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
