# Audio

The port's sound has two modes, switched live in Settings → Audio:

- **Original (2004)** (CLASSIC in the menus) plays what the Steam build
  plays, when and how it plays it: the same sounds from the same archives,
  picked, gated, mixed, and placed by the game's sound library's rules,
  through what the build's OpenAL Soft 1.20.1 output does with them.
  `docs/formats.md` ("Sound") has the rules. On every output, stereo
  speakers included, it plays the full original chain: OpenAL Soft's
  decoder for the layout, the reverb of the listener's space, the dynamics
  and the limiter. Both modes use Rust for every playback and DSP operation.
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

Settings → Audio (`rust/audio-control`; the defaults are in
`rust/bfe-interface` (`settings`)):

- **Sound**: Enhanced (2026 sound stage) or Original (2004), shown as
  ENHANCED and CLASSIC; Enhanced by default. AUDIO DETAILS says what the mode in
  force plays on the output in force; for Classic on stereo, OPENAL SOFT
  STEREO DECODER, THE ORIGINAL REVERB.
- **Output**: Automatic, Headphones (binaural), Stereo speakers, 5.1, 7.1,
  or 7.1.4. Automatic follows the device's channel count: 12 or more is
  7.1.4, 8 to 11 is 7.1, 6 or 7 is 5.1, anything else stereo. Headphones
  are never detected; choose them. A layout the device will not open plays
  as the widest one it does (else stereo), and the audio details say so.
- **Dynamics**: Home theater (full range), Normal, or Night (narrow range).
- **Mono**, **Clarity (enhanced)** (the clarity stage below; on by
  default, off to hear the enhanced rendering without it), and volumes for
  master, effects, music, voice, ambience and the interface. Voice covers
  the voice-over and every soldier's own voice: his hurts and death, and
  the AI's chatter (in the original those two are on the effects slider's
  bus).
- "Speaker test and audio details" opens the audio panel (AUDIO DETAILS).

The panel shows the device (channels, rate, latency), what is rendering,
the loudness meter (momentary, short-term and integrated LUFS, true peak,
each channel's peak, the limiter's gain reduction), and the voices playing. Its speaker test plays pink noise on one
channel at a time, named, with the game muted (the LFE gets a 50 Hz tone),
and "around the room" pans a burst once round through the game's own
panner.

A module failure produces a visible audio error.

## How it fits together

`rust/audio-control` owns the whole battle sound-control phase: it reads
the battle's facts each presented frame (listener, units, weapons, posts,
vehicles, events), borrows the current collision world for its room and
occlusion rays, and decides what plays. `rust/audio` renders it, and
`rust/bfe-client` (`audio.rs`, `online/sound.rs`, `online/sound_output.rs`,
`online/stream_assets.rs`) hosts both on the sound device.

| Owner | Does |
| --- | --- |
| `rust/audio-control (phase.rs)` | Runs event selection, weapons, player, soldiers, scene, vehicles, acoustics and directors in their recorded order. |
| `engine.rs`, `layers.rs` | Gates, sample cycles, layered and parameterized sounds, voice ranking and stealing, bus fades and ducks, distance, Doppler and timed render commands. |
| `player.rs`, `scene.rs`, `vehicles.rs`, `feet.rs` | Unit foley and state sounds, mines, posts, recon and ordnance, engines, flight, turrets, crashes and walker schedules. |
| `director.rs`, `streams.rs` | Mission music, VO, chatter, static and region ambience, stream slots and queues, asset variants, decoded-cache budget and segment scheduling. |
| `acoustics.rs` | Room and occlusion ray budgets, air, LFE, reverb sends and the loudness window. |
| `rust/xtask` (`audio_schema.rs`) | Generates the matching input, resource and output layouts. |
| `rust/audio (output.rs)` | Voice and stream playback, placement, convolution, room sends, mono and the complete master phase. |
| `rust/audio (frontend.rs)` | Menu and loading PCM playback, resampling, bus gains and fades. |
| `rust/audio-decode` | FLAC, Ogg Opus and PCM/float WAV decoding, and asset-rate conversion. |
| `rust/bfe-client` (`audio.rs`) | The one `cpal` output stream, shared by the front end and the battle; device channels and order; the speaker test |
| `rust/bfe-client` (`online/stream_assets.rs`) | Streamed segments read off the window and device threads; Opus segments decoded ahead of playback |

The frame block contains listener pose, device time and configuration, game
clocks, poses and velocities, raw entity, weapon and post facts and discrete
events. Stable rows are kept on both sides; only changed rows cross again.
Stream decode completion is a separate boundary, so a ready segment starts at
device time without waiting for a new gameplay frame. It never reruns
gameplay policy.

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
  listener's space for reverb (the space of the earliest `soundspace` region
  holding him, kept while he stays inside it, dry outside every region;
  Theed has none); pitch glides of 1/256 twice
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
  through it. The port uses an IRCAM HRTF decoder (the IRCAM LISTEN
  composite, as Chromium's audio code prepares it), brought to the same level as a source ahead on
  speakers. If Windows didn't report your headphones, the original played
  its stereo panning: choose Stereo instead.
- **The limiter.** OpenAL Soft's own output limiter is replaced by the
  master chain's (true peak at −1 dBTP), and the dynamics profile applies
  to both modes. Home theater leaves the mix as it was. The light mix
  (below, kept as the cheap path and not used now) has a soft clip instead,
  and the profile is a level there.
- **Level.** The original mode is raised by the power OpenAL Soft's
  decoders give a source ahead (3.3 dB on 5.1, 0.9 on 7.1), so the two
  modes match in loudness. Stereo's 3.2 dB is measured instead: recorded
  battles on Home theater, where nothing compresses, as loud as Enhanced,
  which keeps Normal and Night within half a LU as well. Nothing else in
  the mix changes.
- **7.1.4** is beyond OpenAL Soft 1.20.1: the original plays 7.1, the
  heights silent.
- **Playback resampling** is a linear interpolation (translated from
  Chromium's audio code, see `THIRD-PARTY-NOTICES.md`) for every voice and
  stream. It does not use OpenAL Soft's resampler.

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
stream, once each time he goes out (docs/formats.md "Leaving the
battlefield"); the client that shows him plays it, in every mode.

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

Classic on stereo speakers (and Automatic on a stereo device) plays the
full original chain, as every other layout does: OpenAL Soft's stereo
decoder, the reverb of the listener's space (Theed and the Dune Sea have
none; Jabba's Palace, Kamino, Bespin and Endor's bunker do), the dynamics
and the true-peak limiter. AUDIO DETAILS shows it as OPENAL SOFT STEREO
DECODER, THE ORIGINAL REVERB.

The light mix stays in `rust/audio (output.rs)` as the cheap path
(`Output::light`); no setting or device picks it now. It keeps the same
sound choices, distance curve, Doppler, pitch glides and voice ranking.

- Rust mixes each voice through OpenAL Soft's first-order stereo gains.
  Both gains ease over 15 ms, the initial value applies immediately, and
  changes of a thousandth or less are held.
- Rust places stream channels at OpenAL Soft's angles: stereo hard left
  and right, mono ahead, and a quad bed's rear pair spread across both ears.
- The master applies the original stereo level (+1.5 dB), optional mono,
  the dynamics profile's level (Home theater −8.8 dB, Normal −5.3,
  Night −3.8), the 4097-entry soft-clip curve and master volume.
- It has no reverb, compressor or limiter.

Against the full chain on stereo it drops the reverb, the dynamics'
compression (Night no longer narrows the range there), the true-peak
limiter, and its 1.5 ms of lookahead. The rest renders the same: the same
16 moving voices through both, aligned for that lookahead, differ by
0.01 dB a channel, the residual 47 dB under the signal.

Why the full chain on every PC: on the A8 laptop (four cores, Radeon R5) in a
full bot battle at the Minimum preset, the full chain's worst audio block
per frame is 0.8 ms at the 99th percentile, against 0.65 ms for the light
mix, of a 2.67 ms block, and the frame rate is the same (59 frames a
second with either, three battles each). The chain holds its budget there,
so nothing switches to the light mix.

`rust/bfe-interface` (`settings`) has the rule for starting a first
session on Classic on a low-end CPU (four threads or fewer and 4 GB of
memory or less), but the Rust client does not read the CPU and memory yet,
so every first session starts on Enhanced. A choice the player makes stays
theirs.

## The enhanced mode

### Output

The client writes discrete PCM channels to the sound device through one
`cpal` stream at 48 kHz, in these layouts: stereo L R; 5.1 L R C LFE SL SR;
7.1 FL FR FC LFE BL BR SL SR; 7.1.4 the 7.1 and then TFL TFR TBL TBR.
Windows and macOS take that order as it is; on Linux the client reorders
them for ALSA, which puts the rear pair before the centre and LFE, and for
7.1.4 uses the PipeWire plugin's order (`rust/bfe-client` `audio.rs`). The
speaker test checks the order on your setup. There is no Dolby or DTS
bitstream: a receiver shows multichannel PCM, never "Dolby Digital" or
"Atmos" from the game itself.

Automatic follows the device's own channel count. A battle whose layout
wants more channels opens the device again at that width; a width the device
will not open is struck off, and the layout plays as the widest one it does
(else stereo).

Getting the channels to the device:

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
- **Linux.** The default device's channel count is usually the PulseAudio
  (or pipewire-pulse) default sample spec's, which is stereo even on a
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

Without the filters, each 3D voice uses Rust's IRCAM HRTF decoder (the
original mode on headphones does too, as OpenAL Soft's HRTF pans every
source). Its cost follows the voices actually sounding: a voice silent for
longer than its response (waiting for its next sound, or held under the
heard limit of 32) runs no convolution, since everything it would read is
zero, so the per-voice work is bounded by the heard voices and the beds'
lanes. On a full Theed battle that per-voice path costs about a third of
what it did before those two changes and the transform's precomputed stage
roots (identical samples), around three times the stereo mix. Headphones are
never chosen automatically; only the OUTPUT setting reaches this path.

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
- **Attacks** (`rust/audio (clarity.rs)`): two envelopes of the whole mix's
  power, fast (1 ms up, 40 ms down) and slow (25 ms up, 250 ms down), linked
  across the channels so the image holds. Where the fast one stands over the
  slow one an attack is under way, and the gain rises by a quarter of the
  difference past the initial dB, up to +4 dB, easing back over 60 ms. Shots
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
`cargo test --locked --workspace` on the same 60 s of Theed in each mode (a seeded
battle: walking, turning, firing in bursts among the AI's fighting),
integrated loudness gated per BS.1770-4, loudness range per EBU Tech 3342.
True peak stayed at or under −1 dBTP throughout. The meter in the panel is
the same BS.1770 meter (K-weighting, 400 ms blocks, absolute and relative
gates).

## Runtime ownership

One `cpal` output stream lives as long as the window (`rust/bfe-client`
`audio.rs`); the front end's mixer and the battle's renderer take turns on
it. The engine renders in 128-frame blocks; a device buffer that is not a
multiple of that keeps the unused samples, so a short callback never
advances or loses DSP time. The callback owns its renderer and its PCM: the
window never reaches into DSP state while the device thread renders, and
diagnostics read atomics only.

Rust owns PCM normalization and loading-time gain compensation as well as
sample cursors, playback-rate interpolation, automation, gain, filters,
panning, stream routing and upmix delays, both headphone decoders, reverb,
EQ, clarity, mono, dynamics, limiting and volume. Classic surround uses
OpenAL Soft's decoder and front stabilizer. Classic headphones and Enhanced
headphones without the optional dummy-head filters use the IRCAM HRTF
database. Its kernel preparation, interaural delays and crossfades run in
Rust. The dummy-head path uses 32 partitioned ear convolutions. The front
end keeps an all-mono mix in one lane until the final stereo copy.

Mono keeps the frozen routing, including its existing 7.1/7.1.4 behavior:
the initial channel is copied to every output. Stereo and 5.1 use their
recorded downmix coefficients. Changing this is a separate audible change.

When nothing can be heard (the master volume rests at 0, or there is no sound
device, `--no-audio`), nothing is mixed: each block, every voice, bed and
placement moves on as if it played, so the sound picks up in place, but no
sample is read, placed or mixed and the master chain does not run. Streamed
segments are not decoded meanwhile: an ADPCM feed moves on to the block it
has reached (each block starts afresh), and an upscaled Opus segment drops
whole packets undecoded and decodes again from 80 ms before the frame it
has reached when the sound returns. Coming back starts a fresh mix
configuration (no reverb tail or limiter state from before the silence),
and the volume glides up from 0 as from any other level. With no sound
device the battle's sound still runs, on a thread of its own.

The meter is Rust's BS.1770 meter. Speaker-panel signals are generated by
Rust; the speaker test plays in the output layout's channels and, while it
plays, is all the device hears. The front end's music is decoded one track
at a time into a bounded ring, so no whole decoded track is kept.

Sources, filters, delay lines, FFT buffers and render scratch are reserved
before their first block. Stream segments use three reusable sample slots
per lane; finished lanes release their PCM. Meter integrated history
reserves another hour between blocks, requested with one minute of capacity
remaining.

### Asset decoding and rate conversion

`rust/audio-decode` owns FLAC, Ogg Opus and PCM/float WAV decoding, off the
device thread. Claxon decodes FLAC integers; opus-pure decodes Opus packets at
48 kHz. Rust parses Ogg pages, checks CRCs, joins continued packets, applies
pre-skip, output gain and the final granule trim, and reads multistream
mappings from the header. The tools' family-255 streams preserve discrete
lane order; family 1 keeps the standard Vorbis channel mapping. The optional
SADIE decoder uses sixteen-channel IEEE-float WAV. Xbox IMA ADPCM segments
stay compressed and are decoded a block at a time as they play; an upscaled
(Opus) segment is decoded on a thread of its own, a few windows ahead of
playback, so the device only copies it (`rust/bfe-client`
`online/stream_assets.rs`).

A native-rate request returns source-rate PCM. Callers request device-rate
conversion explicitly to preserve the previous asset-loading behavior.
Rust owns the 32-tap sinc conversion and truncates the output frame count
exactly as the frozen reference does. Track lengths, scheduled segment
ends, loop offsets and playback-rate scale therefore still use the same
frame counts and rates. This conversion is separate from the linear
playback resampler. HRIR conversion also multiplies by the WAV source rate
divided by the requested output rate. Generated HRIR metadata and WAV rates agree.

Seekable assets are read in at most 64 KiB ranges. An Opus tail range
provides its final granule and exact duration before playback.
Compressed input, page/packet reassembly and decoder PCM are bounded.
Long menu music and enhanced beds transfer 4,096-frame planar windows
directly to a 32,768-frame Rust ring, with credits from the consumer. An
8,192-frame pre-roll precedes the scheduled start. Attached playback
producers are scheduled before background effect decoding, so clip
preloading cannot consume their next decode turn. Stops cancel the producer;
replacing a segment releases its previous decoder. Short FLAC effects may
retain their PCM in the existing asset cache. The original ADPCM segment
cache keeps its existing device-memory limit.

There is no codec fallback. A decode or stream underrun failure reaches
the player's audio error display.

The synthetic codec corpus records native and converted PCM at 22.05,
44.1 and 48 kHz. FLAC S16/PCM_24 and float HRIR WAV compare bit for bit at
source rate; converted output and Opus stay below -96 dBFS. S16 conversion
keeps the host's asymmetric float32 reciprocals: negative samples multiply
by `1/32768`, nonnegative samples by `1/32767`. Container chunk boundaries
must not change PCM (`cargo test --locked -p audio-decode`).

## Recorded render gates

`tests/fixtures/audio` contains synthetic reference renders, recorded once
and frozen; its manifest records where they came from. No game assets are
included. The graph fixture exercises
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
allocation tests count allocations from the initial block, including stream,
convolution, front-end playback and meter report paths.

`cargo test --locked --workspace` additionally drives the battle's complete sound
controller with synthetic game facts. Thirteen recorded scenes cover Classic and
Enhanced stereo, mono, 5.1, 7.1, HRTF and synthetic binaural output. They exercise
weapon state changes, moving sources, voice stealing and rejection, command
posts, stations, timed mines, hull and turret sounds, speech, room probes and
pause fades, layered-emitter curves and droideka reload/heat state. PCM differences must remain below -96 dBFS; per-frame voice counts,
steals, rejections and random-draw state must match exactly. Synthetic asset
completions are drained before offline resume, and release timers follow the
scripted context clock, so host scheduling cannot change the reference.

Native control allocation tests exercise the complete first frame and
retained frames, including asynchronous asset completion. The phase
regressions are in `rust/audio-control/tests/`, with frozen scripted
decisions and PCM.

Hardware listening
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
channels, decoded in order by `rust/audio-decode`). Music also gets 5.1 and 7.1
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
No model weights are shipped or required by the current audio tools.

## Licenses

| Component | Used for | License |
| --- | --- | --- |
| OpenAL Soft 1.20.1 | decoder coefficients (`rust/audio-control`) and its band splitter and front stabilizer, translated to Rust (`rust/audio (stabilizer.rs)`), for the original mode | LGPL-2.0-or-later; see `THIRD-PARTY-NOTICES.md` |
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
cd rust
cargo test --locked -p audio -p audio-decode -p audio-control
```

They run the Rust renderer and controller against the recorded fixtures,
in CI. A failed or silent render fails the gate.

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
9. **Classic on stereo** (Classic, Output Stereo): on Theed and the Dune
   Sea it is dry (their maps have no space); in Jabba's Palace, Kamino's
   bunker and Bespin each shot has the room's tail, and outdoors on Endor
   and in Kamino's rain it is dry again. Its loudness should match Enhanced
   on the same dynamics profile. Moving sources should pan smoothly, and
   the loudest moments shouldn't crackle.
10. **Menus and loading**: listen to the initial track starting, rapid menu
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
