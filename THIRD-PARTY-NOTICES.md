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
| Chromium audio platform | `rust/audio/src/hrtf.rs` translates HRTF kernel preparation, spatial convolution and delay; `rust/audio-decode/src/resample.rs` translates streaming asset-rate conversion; `rust/audio/data/irc-composite.f32` is its decoded composite response resource | Copyright the Chromium Authors; Copyright (C) 2010 Google Inc. | BSD-3-Clause and the WebKit two-clause license (below) |
| [OpenAL Soft](https://github.com/kcat/openal-soft) 1.20.1 | `rust/audio/src/stabilizer.rs` translates its band splitter and front stabilizer; `rust/audio/src/stereo.rs` retains its stereo panning law; `src/audio/layouts.js` carries its speaker decoder coefficients | Copyright the OpenAL Soft authors | LGPL-2.0-or-later. The derived code is distributed under GPL-3.0-only, as section 3 of that license allows. |
| [libebur128](https://github.com/jiixyj/libebur128) | The K-weighting filter design in `rust/audio/src/meter.rs` and `tools/enhance_audio.py` | Copyright (c) 2011 Jan Kokemüller | MIT |
| AMD FidelityFX Super Resolution 1 | The RCAS sharpening pass in `src/shell/video-sharpen.js` follows its design | Copyright (c) Advanced Micro Devices, Inc. | MIT |
| fdlibm, as ported to C++ in [V8](https://v8.dev) 13.6 (`src/base/ieee754.cc`), and V8's `math::pow` (`src/numbers/ieee754.cc`) | `rust/v8math/src/trig.rs`, `explog.rs`, `inv.rs` and the first part of `pow.rs` translate them to Rust | Copyright (C) 1993 by Sun Microsystems, Inc.; Copyright 2011 and 2016 the V8 project authors | fdlibm: "Permission to use, copy, modify, and distribute this software is freely granted, provided that this notice is preserved." V8: BSD-3-Clause (below) |
| The double-precision `pow` of Arm's [optimized-routines](https://github.com/ARM-software/optimized-routines) (`math/pow.c` and its tables), which glibc ships as its `pow` | `rust/v8math/src/pow.rs` and `pow_data.rs` translate it to Rust | Copyright (c) 2018-2026, Arm Limited | MIT (offered as MIT OR Apache-2.0 WITH LLVM-exception; used here under MIT) |
| [three.js](https://threejs.org) r170 math collections | `rust/three` transliterates its `src/math` classes and `Object3D`'s transform semantics to Rust, operation by operation, for the simulation port | Copyright 2010-2024 three.js authors | MIT |
| TimSort, as V8 runs it for `Array.prototype.sort` | `rust/jsrt/src/sort.rs` transliterates V8 13.6's `array-sort.tq`, itself a port of CPython's listsort | Copyright (c) 2001-2018 Python Software Foundation (the algorithm text V8's file carries); V8's port copyright the V8 project authors | Python Software Foundation License 2.0; BSD-3-Clause (V8) |

## Libraries bundled in builds

| Component | Copyright | License |
| --- | --- | --- |
| [Claxon](https://github.com/ruuda/claxon) 0.4.3, pure Rust FLAC decoder | Copyright 2014-2019 Ruud van Asseldonk and contributors | Apache-2.0 (full text in `rust/audio-decode/licenses/Claxon.txt`, installed as `legal/audio-codecs/Claxon.txt`) |
| [opus-pure](https://github.com/stephenberry/opus-pure) 0.2.2, pure Rust Opus decoder | Copyright 2026 Stephen Berry; Copyright 2026 restsend.com; Copyright 2001-2023 Xiph.Org and contributors | BSD-3-Clause (full notices in `rust/audio-decode/licenses/Opus.txt`, installed as `legal/audio-codecs/Opus.txt`) |
| [three.js](https://threejs.org), including addons such as CSM and FXAAShader (FXAA algorithm by NVIDIA; C# implementation by Jasper Flick; GLSL port by Dave Hoskins) | Copyright 2010-2024 three.js authors | MIT |
| [Basis Universal](https://github.com/BinomialLLC/basis_universal) 2.50 transcoder, vendored through basisu_c_sys 0.9.1 | Copyright Binomial LLC | Apache-2.0; `legal/image-codecs/Basis-Universal.txt` |
| Zstandard 1.5.7 decompressor | Copyright Meta Platforms, Inc. and affiliates | BSD-3-Clause; `legal/image-codecs/Zstandard.txt` |
| basisu_wasm_libcxx 0.1.0 and its bare-WebAssembly support | Copyright Gyungmin Myung and contributors | MIT; `legal/image-codecs/Basisu-Wasm-Libcxx.txt` |
| musl libc, built by basisu_wasm_libcxx | Copyright 2005-2020 Rich Felker et al.; additional authors listed in its notices | MIT with retained sub-notices; `legal/image-codecs/Musl.txt` |
| Emscripten LLVM libc++, libc++abi and compiler-rt, built by basisu_wasm_libcxx | Copyright LLVM contributors and the University of Illinois | Apache-2.0 WITH LLVM-exception and retained legacy notices; `legal/image-codecs/Libcxx.txt`, `Libcxxabi.txt`, `Compiler-rt.txt` |
| Signal shim in basisu_wasm_libcxx | Copyright Gyungmin Myung | Blue Oak Model License 1.0.0; `legal/image-codecs/Blue-Oak-Signal.txt` |
| nanoprintf 0.5.2 | Copyright 2019- Charles Nicholson | 0BSD (selected from 0BSD OR Unlicense); `legal/image-codecs/Nanoprintf.txt` |
| png 0.18.1 | Copyright 2015 nwin and image-rs contributors | MIT (selected from MIT OR Apache-2.0); `legal/image-codecs/Png.txt` |
| image-webp 0.2.4 | image-rs contributors | MIT (selected from MIT OR Apache-2.0); `legal/image-codecs/Image-Webp.txt` |
| Decoder dependencies: adler2, bitflags, byteorder-lite, cfg-if, crc32fast, fdeflate, flate2, miniz_oxide, quick-error and simd-adler32 | Their respective contributors, as listed in the shipped texts | MIT, Apache-2.0, BSD-3-Clause and Zlib as applicable; complete texts under `legal/image-codecs/` |
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

## BSD-3-Clause License (V8)

The V8 code above is provided under the following terms:

> Copyright 2014, the V8 project authors. All rights reserved.
> Redistribution and use in source and binary forms, with or without
> modification, are permitted provided that the following conditions are
> met:
>
> * Redistributions of source code must retain the above copyright
>   notice, this list of conditions and the following disclaimer.
> * Redistributions in binary form must reproduce the above
>   copyright notice, this list of conditions and the following
>   disclaimer in the documentation and/or other materials provided
>   with the distribution.
> * Neither the name of Google Inc. nor the names of its
>   contributors may be used to endorse or promote products derived
>   from this software without specific prior written permission.
>
> THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
> "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
> LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
> A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
> OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
> SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
> LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
> DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
> THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
> (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
> OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

## BSD-3-Clause License (Chromium audio)

```
// Copyright 2015 The Chromium Authors
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are
// met:
//
//    * Redistributions of source code must retain the above copyright
// notice, this list of conditions and the following disclaimer.
//    * Redistributions in binary form must reproduce the above
// copyright notice, this list of conditions and the following disclaimer
// in the documentation and/or other materials provided with the
// distribution.
//    * Neither the name of Google LLC nor the names of its
// contributors may be used to endorse or promote products derived from
// this software without specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
// "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
// LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
// A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
// OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
// SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
// LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
// DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
// THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
// (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
// OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

## WebKit two-clause license (HRTF implementation)

```text
Copyright (C) 2010, Google Inc. All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions
are met:
1.  Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer.
2.  Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in the
   documentation and/or other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY APPLE INC. AND ITS CONTRIBUTORS ``AS IS'' AND
ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
ARE DISCLAIMED. IN NO EVENT SHALL APPLE INC. OR ITS CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH
DAMAGE.
```
