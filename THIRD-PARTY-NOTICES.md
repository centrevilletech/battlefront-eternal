# Third-party notices

Battlefront Eternal is licensed under the GNU General Public License,
version 3 only ([LICENSE](LICENSE)). It includes, derives from or bundles the
third-party work below, each under its own license.

No original game files are included in this repository or in the desktop
packages. Battlefront Eternal prepares its data from the player's own copy of
the game, on the player's machine.

## In this repository

| Component | Where | Copyright | License |
| --- | --- | --- | --- |
| [Saira](https://github.com/Omnibus-Type/Saira) typeface | `electron/ui/fonts/saira-latin.woff2`; also used to draw `electron/assets/steam/` | Copyright 2020 The Saira Project Authors | SIL Open Font License 1.1 (full text in `electron/ui/fonts/OFL.txt`) |
| [OpenAL Soft](https://github.com/kcat/openal-soft) 1.20.1 | `src/audio/worklets/stabilizer.js` translates its band splitter and front stabilizer; `src/audio/layouts.js` carries its speaker decoder coefficients | Copyright the OpenAL Soft authors | LGPL-2.0-or-later. The derived code is distributed under GPL-3.0-only, as section 3 of that license allows. |
| [libebur128](https://github.com/jiixyj/libebur128) | The K-weighting filter design in `src/audio/worklets/meter.js` and `tools/enhance_audio.py` | Copyright (c) 2011 Jan Kokemüller | MIT |
| AMD FidelityFX Super Resolution 1 | The RCAS sharpening pass in `src/shell/video-sharpen.js` follows its design | Copyright (c) Advanced Micro Devices, Inc. | MIT |

## Libraries bundled in builds

| Component | Copyright | License |
| --- | --- | --- |
| [three.js](https://threejs.org), including addons such as CSM, KTX2Loader and FXAAShader (FXAA algorithm by NVIDIA; C# implementation by Jasper Flick; GLSL port by Dave Hoskins) | Copyright 2010-2024 three.js authors | MIT |
| [Basis Universal](https://github.com/BinomialLLC/basis_universal) transcoder (shipped with three.js) | Copyright Binomial LLC | Apache-2.0 |
| [three-mesh-bvh](https://github.com/gkjohnson/three-mesh-bvh) | Copyright (c) 2018 Garrett Johnson | MIT |
| [ws](https://github.com/websockets/ws) | Copyright (c) 2011 Einar Otto Stangvik; Copyright (c) 2013 Arnout Kazemier and contributors | MIT |
| [yauzl](https://github.com/thejoshwolfe/yauzl), [yazl](https://github.com/thejoshwolfe/yazl) | Copyright (c) 2014 Josh Wolfe | MIT |
| [Electron](https://www.electronjs.org) and Chromium (desktop packages) | Copyright the Electron and Chromium authors | MIT; Chromium's notices ship with Electron as `LICENSES.chromium.html` |

## Tools bundled in desktop packages

The desktop packages include these programs, fetched by
`build/fetch-toolchain.mjs`, to prepare game data on the player's machine:

| Component | License | Source |
| --- | --- | --- |
| FFmpeg (GPL-enabled static builds by BtbN for Windows and Linux, and by Martin Riedl for macOS) | GNU GPL | <https://ffmpeg.org>, <https://github.com/BtbN/FFmpeg-Builds>, <https://ffmpeg.martin-riedl.de> |
| KTX-Software (Khronos Group) | Apache-2.0 | <https://github.com/KhronosGroup/KTX-Software> |
| CPython (python-build-standalone) | Python Software Foundation License 2.0, plus the licenses of the libraries it bundles | <https://github.com/astral-sh/python-build-standalone> |
| Pillow | MIT-CMU (HPND) | <https://github.com/python-pillow/Pillow> |
| NumPy, SciPy | BSD-3-Clause | <https://numpy.org>, <https://scipy.org> |
| python-soundfile, with libsndfile | BSD-3-Clause; libsndfile LGPL-2.1-or-later | <https://github.com/bastibe/python-soundfile>, <https://github.com/libsndfile/libsndfile> |
| python-soxr, with libsoxr | LGPL-2.1-or-later | <https://github.com/dofuuz/python-soxr> |

## Downloaded on the player's machine

`tools/make_binaural.py` downloads the SADIE II database's D1 head-related
impulse responses (University of York, Apache-2.0) and uses
[spaudiopy](https://github.com/chris-hld/spaudiopy) (MIT) to build the
headphone decoder. Neither is stored in this repository.

## Acknowledgments

[swbf-unmunge](https://github.com/PrismaticFlower/swbf-unmunge) by
PrismaticFlower (MIT) was a valuable reference: some property-name spellings
were checked against its name list, and its PATH decoder helped cross-check
our own reader. No code from it is included.

## MIT License

The components above marked MIT are provided under the following terms, with
the copyright notices listed above:

> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to
> deal in the Software without restriction, including without limitation the
> rights to use, copy, modify, merge, publish, distribute, sublicense, and/or
> sell copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in
> all copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
> FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS
> IN THE SOFTWARE.
