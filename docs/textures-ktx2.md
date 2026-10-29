# KTX2 textures

The extractor writes every texture as a PNG. `tools/ktx2.py` adds a KTX2 copy
beside each one: Basis UASTC with Zstandard supercompression, carrying a full
mip chain. The runtime can transcode it on load to BC7, ASTC 4x4, BC1/BC3 or
ETC, whichever the GPU takes. That is a quarter of the memory and bandwidth of
RGBA8, and no `gl.generateMipmap` at load. The KTX2s are optional. A texture
without one (no encoder, skipped, failed) is loaded from its PNG as before.

Everything the tool writes goes next to the PNGs, in the gitignored
`public/maps`, `public/sides` and `public/common` trees. Never commit it.

## Running it

It runs automatically at the end of an import:

- `tools/extract.py <map> --side <side>` encodes `public/maps/<map>` (its
  `textures`, `vehicles/textures` and `environment/textures`),
  `public/sides/<side>` and `public/common`. It runs after the three exports
  and before audio.
- `tools/zero_vehicle.py` encodes the vehicle pack it wrote. With
  `--all-extracted`, it encodes every `maps/*/vehicles`.
- `tools/zero_environment.py` encodes the `environment/textures` it wrote.

In the extractors a missing encoder, or any error, only prints a warning. The
extraction never fails over compression. If you call an export function
directly from Python (for example `extract_side`, as in `docs/ai-heroes.md`),
run the CLI on that pack afterwards.

```sh
python3 tools/ktx2.py                       # every pack: public/maps/*, public/sides/*, public/common
python3 tools/ktx2.py public/maps/nab2      # a pack root: every `textures` directory under it
python3 tools/ktx2.py public/sides/rep/textures --jobs 4
python3 tools/ktx2.py --force public/common # re-encode even unchanged PNGs
python3 tools/ktx2.py --check public/maps/nab2/textures/nab2_bldg_gate.png
```

Repeat runs are cheap. Only PNGs whose bytes changed are encoded again (see
[Manifest](#manifest)), so a full no-op pass over all 1174 textures takes
about a second. `KTX2_ENCODER=off` turns compression off for a run. With no
encoder, the CLI also warns and exits 0.

## Installing the encoder

The encoder is `ktx` from KhronosGroup's KTX-Software, 4.4 or later (4.4
renamed `--assign-oetf` to `--assign-tf`). The Linux release is
self-contained (`$ORIGIN` rpath to its own `libktx`). Unpack it under
`~/.cache/bf-tools`, never in the repo:

```sh
mkdir -p ~/.cache/bf-tools/bin && cd ~/.cache/bf-tools
curl -LO https://github.com/KhronosGroup/KTX-Software/releases/download/v4.4.2/KTX-Software-4.4.2-Linux-x86_64.tar.bz2
sha1sum KTX-Software-4.4.2-Linux-x86_64.tar.bz2   # c6b08c817f8c8dd299deccae4f2fbb8d55e9acd2
tar xjf KTX-Software-4.4.2-Linux-x86_64.tar.bz2
ln -sfn ../KTX-Software-4.4.2-Linux-x86_64/bin/ktx bin/ktx
bin/ktx --version                                  # ktx version: v4.4.2
```

The tool looks for the encoder in this order:

1. `$KTX2_ENCODER`: the `ktx` binary, or an unpacked KTX-Software directory.
   When this is set, nothing else is tried.
2. `ktx` on `PATH`.
3. `~/.cache/bf-tools/bin/ktx`.
4. The newest `~/.cache/bf-tools/KTX-Software-*/bin/ktx`.

On other platforms, use the release's installer, which puts `ktx` on `PATH`.
`basisu` is not supported. `ktx` wraps the same Basis Universal encoder, adds
the mip filters and DFD control used here, and Khronos ships it as a binary.

## The encoding

One `ktx create` per PNG, with one process per core:

```sh
ktx create --format R8G8B8A8_UNORM --assign-tf linear \
    --generate-mipmap --mipmap-filter box \
    --encode uastc --uastc-quality 2 --zstd 18 --threads 1 foo.png foo.ktx2
```

- **UASTC, not ETC1S/BasisLZ.** UASTC is 8 bpp, and its blocks are close to a
  subset of BC7 and ASTC 4x4, so transcoding to those is nearly a repack.
  ETC1S gives far smaller files at ETC1 quality. With default settings it was
  4× smaller on `nab2_bldg_gate` (32.2 dB against UASTC's 38.5 dB) and 7×
  smaller on the normal map `rhn_bldg_coffin_bump` (17.9 dB against 27.9 dB).
  These are `ktx create --compare-psnr` figures, as are the next ones.
- **Quality level 2.** On the hardest textures (see below), levels 3 and 4
  gained at most 0.22 dB. Level 3 took up to 2.5× as long, and level 4 up to
  37×.
- **Zstandard 18.** Lossless. Overall it takes the 113.8 MB of UASTC blocks
  down to 76.0 MB, a third smaller. The median texture of 128x128 or more ends
  at 0.69 of its raw size. RDO (`--uastc-rdo`) would buy smaller files with
  quality, so it is off.
- **Mips on raw values, box filter.** The engine samples textures raw and
  lights and blends in gamma space, so the data is tagged linear
  (`R8G8B8A8_UNORM` plus `--assign-tf linear`). With an sRGB tag, `ktx`
  decodes to linear light before resampling. With the linear tag it filters
  the stored values directly, as `gl.generateMipmap` did. Verified on an
  uncompressed encode of a real texture: level 1 matched a 2x2 raw average
  (rounded) on every texel. The sRGB-tagged encode matched the linear-light
  average instead, differing from the raw one by up to 21. `--assign-tf` also
  stops `ktx` from treating an untagged PNG as sRGB and converting it. Its
  warning calls that "a visual lossy color conversion".
- **Linear DFD.** The transfer function is `KHR_DF_TRANSFER_LINEAR` with BT.709
  primaries. three r170's KTX2Loader maps that to `LinearSRGBColorSpace`: a
  non-sRGB upload format, sampled raw. An sRGB tag would map to
  `SRGBColorSpace` and an sRGB format, which decodes on sampling. The runtime
  overrides colour space anyway.
- **Perceptual metrics.** `ktx` can't set perceptual metrics separately: libktx
  sets basisu's `m_perceptual` from the transfer function, and that one switch
  also controls the mip linearization and the DFD. That costs nothing here.
  Only the ETC1S path reads `m_perceptual`. The UASTC encoder measures plain
  RGBA error (`perceptual = false` in `basisu_uastc_enc.cpp`). An sRGB-tagged
  and a linear-tagged encode of the same PNG gave bit-identical level data.
  Since the stored values are gamma-encoded, error on them already tracks
  perceived error much better than error in linear light would.
- **No flip.** There is no `--convert-texcoord-origin`, and no
  `KTXorientation` key is written. The data starts with the PNG's top row,
  the top-left origin the KTX2 spec prefers, which is what the runtime's
  `flipY = false` expects. Compressed uploads can't be flipped by the GPU
  anyway.
- **Alpha.** Input is always RGBA. Basis drops the alpha channel when every
  texel is opaque, and the DFD says which happened: channel `UASTC_RGB` or
  `UASTC_RGBA`. Transcoders use this to pick BC1/ETC1 (4 bpp) over BC3/ETC2
  RGBA where BC7 and ASTC are missing. 685 textures keep alpha; 489 are
  opaque.
- **Multiples of 4 only.** WebGL's S3TC, RGTC and BPTC extensions reject a
  level 0 whose width or height is not a multiple of 4. A PNG like that, or
  smaller than 4x4, is not encoded; it is listed under `skipped` with the
  reason, and the runtime keeps the PNG. Later levels need `size << level` to
  be a multiple of 4, which every mip of a multiple-of-4 base satisfies, so
  non-power-of-two sizes such as 12 or 20 are fine. Today every extracted
  texture is a power of two from 4x4 to 1024x256, and none is skipped.
- **One thread per `ktx`.** Parallelism comes from running one file per core,
  largest first. The output doesn't depend on the thread count, apart from the
  `KTXwriterScParams` metadata.

## Manifest

Every processed `textures` directory gets a `ktx2.json`, which the runtime
reads:

```json
{
  "version": 1,
  "encoder": "ktx create v4.4.2 --format R8G8B8A8_UNORM --assign-tf linear --generate-mipmap --mipmap-filter box --encode uastc --uastc-quality 2 --zstd 18",
  "files": {
    "foo.png": {"ktx2": "foo.ktx2", "width": 256, "height": 256, "sha1": "<sha1 of the PNG bytes>", "bytes": 81234}
  },
  "skipped": {
    "bar.png": "6x8: both sides must be multiples of 4 (so at least 4) for WebGL to take a block-compressed level 0"
  }
}
```

- `files` is keyed by the PNG's basename exactly as on disk, case kept. nab2
  has both `sky_naboo2.png` and `SKY_naboo2.png`. `ktx2` is the file beside
  it, `width`/`height` are level 0, `sha1` hashes the PNG the KTX2 was made
  from, and `bytes` is the KTX2's size.
- A texture whose PNG is in `files` has a KTX2. For anything else, load the
  PNG.
- `skipped` maps a PNG to the reason it has no KTX2. The reason is the size
  rule, "not a PNG", "encode failed: ..." (retried on the next run), or two
  PNGs that would share a `.ktx2` name.
- A PNG is encoded again when:
  - its sha1 changed;
  - the manifest's `encoder` string differs (another `ktx` version or other
    flags);
  - its KTX2 is missing, or its size differs from `bytes`;
  - `--force` is given.
- KTX2s that no manifest entry owns are deleted. That covers a PNG that is
  gone, a PNG that is now skipped, and a failed re-encode.
- Encodes go to `.<name>.ktx2.<pid>.tmp` and are renamed into place. The
  manifest is rewritten (atomically) only when it changes. A later run deletes
  temporaries left by an interrupted one.

## Checking

`--check` (`python3 tools/ktx2.py --check [paths or files]`) decodes each
KTX2's levels 0 and 1 with `ktx extract --transcode rgba8`. It compares them
with its own pure-Python decode of the PNG, independent of `ktx`'s loader, so
a flip would show. It verifies:

- the full mip chain;
- UASTC + Zstandard + linear transfer;
- an alpha channel exactly when the PNG has a non-opaque texel;
- that level 0 is closer to the PNG than to the PNG upside down;
- that level 1 follows a raw box filter rather than a linear-light one. It
  votes on the values where the two filters differ by 4 or more.

It prints PSNR and the maximum error per file.

Full run, all 1174 textures (1m38s on 8 cores): no problems.

- **Orientation and alpha:** correct on every file.
- **Mip filter:** in all 731 textures with at least 100 telling values,
  level 1 is nearer the raw filter (median 87%, minimum 53%). An sRGB-tagged
  control scored 38% and was flagged.
- **Colour quality:** RGB PSNR median 49.7 dB. Outside the `*_bump`
  textures, the minimum is 30.1 dB and the 5th percentile 39.9 dB.
- **The 16 water du/dv maps** (`water_bumpmap_*`) score 33.7 to 34.0 dB.
- **Normal maps:** the other 77 `*_bump` textures are tangent-space normal
  maps (RGB = XYZ, A = gloss; see `src/materials.js`). They are the weak
  class: median 30.5 dB, and every one of the 36 files under 30 dB, down to
  23.8 dB on `geo_bldg_war_room_bump`. They are noisy texel to texel, which
  4x4 blocks can't follow, and levels 3 and 4 don't help. If lit bump surfaces
  show block noise, either keep those as PNG (a skip rule on `_bump`) or
  encode them with `--normal-mode`. `--normal-mode` stores X+Y and needs the
  shader to rebuild Z, which is a runtime change.

## Measured

Encoding all 1174 PNGs from scratch takes 149 s on 8 cores; an unchanged
rerun takes 1 s. GPU memory below counts full mip chains: RGBA8 is what the
PNG path uploads, and BC7 (or ASTC 4x4) is 16 bytes per 4x4 block on every
level. The PNGs are written with zlib level 1, so they are larger than they
need to be on disk.

| Pack | Textures | Alpha | PNG | KTX2 | RGBA8 + mips | BC7 + mips |
|---|---:|---:|---:|---:|---:|---:|
| common | 73 | 71 | 0.4 MB | 0.4 MB | 4.8 MB | 1.2 MB |
| maps/bes1/vehicles | 49 | 42 | 1.8 MB | 1.4 MB | 10.2 MB | 2.6 MB |
| maps/bes2 | 75 | 15 | 9.9 MB | 7.1 MB | 51.6 MB | 12.9 MB |
| maps/bes2/environment | 1 | 1 | 0.0 MB | 0.0 MB | 0.1 MB | 0.0 MB |
| maps/bes2/vehicles | 31 | 27 | 0.6 MB | 0.5 MB | 3.3 MB | 0.8 MB |
| maps/end1/vehicles | 30 | 27 | 0.8 MB | 0.7 MB | 4.3 MB | 1.1 MB |
| maps/geo1 | 50 | 16 | 6.2 MB | 4.2 MB | 23.2 MB | 5.8 MB |
| maps/geo1/vehicles | 50 | 42 | 1.5 MB | 1.2 MB | 9.3 MB | 2.3 MB |
| maps/hot1 | 58 | 16 | 3.9 MB | 3.0 MB | 19.2 MB | 4.8 MB |
| maps/hot1/vehicles | 38 | 33 | 1.3 MB | 1.0 MB | 5.7 MB | 1.4 MB |
| maps/kam1 | 53 | 28 | 5.4 MB | 3.3 MB | 21.3 MB | 5.3 MB |
| maps/kam1/vehicles | 1 | 1 | 0.0 MB | 0.0 MB | 0.0 MB | 0.0 MB |
| maps/nab1/vehicles | 43 | 36 | 1.0 MB | 0.9 MB | 5.4 MB | 1.4 MB |
| maps/nab2 | 145 | 53 | 14.0 MB | 9.6 MB | 61.4 MB | 15.4 MB |
| maps/nab2/environment | 6 | 6 | 0.1 MB | 0.0 MB | 0.3 MB | 0.1 MB |
| maps/nab2/vehicles | 47 | 40 | 1.3 MB | 1.0 MB | 6.7 MB | 1.7 MB |
| maps/rhn2 | 106 | 11 | 36.7 MB | 25.0 MB | 125.9 MB | 31.5 MB |
| maps/rhn2/vehicles | 1 | 1 | 0.0 MB | 0.0 MB | 0.0 MB | 0.0 MB |
| maps/tat1/vehicles | 59 | 49 | 2.3 MB | 1.8 MB | 12.3 MB | 3.1 MB |
| sides/all | 71 | 45 | 8.0 MB | 4.7 MB | 28.2 MB | 7.0 MB |
| sides/cis | 63 | 53 | 7.0 MB | 3.7 MB | 20.0 MB | 5.0 MB |
| sides/imp | 66 | 37 | 5.3 MB | 3.5 MB | 22.3 MB | 5.6 MB |
| sides/rep | 58 | 35 | 4.4 MB | 2.9 MB | 19.4 MB | 4.8 MB |
| **Total** | **1174** | **685** | **112.1 MB** | **76.0 MB** | **454.9 MB** | **113.8 MB** |

(MB = 10^6 bytes.)

A Theed battle loads nab2 with its vehicles and environment, plus rep,
cis and common. That is 392 textures: 27.2 MB of PNG against 17.7 MB of KTX2
to download, and 112.5 MB of RGBA8 against 28.1 MB of BC7 on the GPU. On a
device that has neither BPTC nor ASTC and falls back to BC1/BC3 or ETC, opaque
textures drop to 4 bpp.

## Runtime

`src/textures.js` (`TextureLibrary`) loads every world texture. `game.js`
creates it with the renderer and the loading manager, then awaits
`prepare()`. That reads the `ktx2.json` of each folder the map, side, common,
vehicle and other-side tables point into. Each `load(url)` then works like
this:

- **PNG listed in a manifest:** returns a texture at once, as
  `TextureLoader` would, so materials can hold it from the start.
  - Three's `KTX2Loader` fetches the KTX2 and transcodes it in workers to
    what the GPU samples directly: BC7 on desktop, ASTC or ETC2 on mobile,
    or BC1/BC3 failing those.
  - The result fills the waiting texture: its mip chain (no
    `generateMipmaps`), `flipY = false`, and `NoColorSpace` so values are
    sampled raw, as with the PNGs.
  - The loading manager counts the transcode, so `ready` waits for it.
  - If the KTX2 fails, the PNG fills the texture instead and a warning is
    logged.
- **Normal maps (`*_bump.png`) stay PNG.** They measured worst (see
  above), and their block noise shows in the lighting.
- **CPU readers** call `texturePixels()` (`src/texture-pixels.js`) instead of
  drawing `texture.image` into a canvas: lamp detection and a laser's hue.
  A compressed texture names its PNG in `userData.png`, which is fetched on
  first request. `textureHue()` returns `undefined` until then, and `null`
  for a grey texture.
- **The Settings option "Compressed textures"** (`textureCompression`,
  default on) is read when a map loads.

Checked on Theed (SwiftShader):

- 165 of 172 world textures load compressed; the other 7 are normal maps.
- The frame matches the PNG run at 47.7 dB PSNR, which is visually identical.
- Lamp detection finds the same 107 lights once the PNG fetches complete.

The transcoder (`three/addons/libs/basis/basis_transcoder.{js,wasm}`) is
imported with Vite's `?url`. A loading-manager URL modifier points
`KTX2Loader`'s fixed file names at those URLs, so dev and production builds
both find it.

