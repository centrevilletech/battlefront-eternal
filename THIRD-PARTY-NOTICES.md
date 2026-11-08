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

<!-- BEGIN CARGO METADATA CRATES -->
## Rust crates (`cargo metadata`)

The following third-party Rust crates are resolved in `rust/Cargo.lock`
(`cargo metadata --locked --format-version 1 --manifest-path rust/Cargo.toml`):

| Crate | Version | License | Authors | Repository |
| --- | --- | --- | --- | --- |
| `adler2` | `2.0.1` | 0BSD OR MIT OR Apache-2.0 | Jonas Schievink &lt;jonasschievink@gmail.com&gt;, oyvindln &lt;oyvindln@users.noreply.github.com&gt; | <https://github.com/oyvindln/adler2> |
| `aead` | `0.5.2` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/traits> |
| `aes` | `0.8.4` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/block-ciphers> |
| `aes-gcm` | `0.10.3` | Apache-2.0 OR MIT | RustCrypto Developers | <https://github.com/RustCrypto/AEADs> |
| `ahash` | `0.8.12` | MIT OR Apache-2.0 | Tom Kaitchuck &lt;Tom.Kaitchuck@gmail.com&gt; | <https://github.com/tkaitchuck/ahash> |
| `aho-corasick` | `1.1.5` | Unlicense OR MIT | Andrew Gallant &lt;jamslam@gmail.com&gt; | <https://github.com/BurntSushi/aho-corasick> |
| `alsa` | `0.9.1` | Apache-2.0/MIT | David Henningsson &lt;coding@diwic.se&gt; | <https://github.com/diwic/alsa-rs> |
| `alsa-sys` | `0.3.1` | MIT | Pierre Krieger &lt;pierre.krieger1708@gmail.com&gt;, David Henningsson &lt;coding@diwic.se&gt; | <https://github.com/diwic/alsa-sys> |
| `android-activity` | `0.6.1` | MIT OR Apache-2.0 | The crate authors | <https://github.com/rust-mobile/android-activity> |
| `android-properties` | `0.2.2` | MIT | Mikhail Lappo &lt;mikhail.lappo@esrlabs.com&gt; | <https://github.com/miklelappo/android-properties> |
| `as-raw-xcb-connection` | `1.0.1` | MIT OR Apache-2.0 | The crate authors | <https://github.com/psychon/as-raw-xcb-connection> |
| `atomic-waker` | `1.1.2` | Apache-2.0 OR MIT | Stjepan Glavina &lt;stjepang@gmail.com&gt;, Contributors to futures-rs | <https://github.com/smol-rs/atomic-waker> |
| `autocfg` | `1.5.1` | Apache-2.0 OR MIT | Josh Stone &lt;cuviper@gmail.com&gt; | <https://github.com/cuviper/autocfg> |
| `base64` | `0.22.1` | MIT OR Apache-2.0 | Marshall Pierce &lt;marshall@mpierce.org&gt; | <https://github.com/marshallpierce/rust-base64> |
| `base64` | `0.23.1` | MIT OR Apache-2.0 | Marshall Pierce &lt;marshall@mpierce.org&gt; | <https://github.com/marshallpierce/rust-base64> |
| `base64ct` | `1.8.3` | Apache-2.0 OR MIT | RustCrypto Developers | <https://github.com/RustCrypto/formats> |
| `basisu_wasm_libcxx` | `0.1.0` | MIT OR Apache-2.0 | The crate authors | <https://github.com/beicause/basisu_c_sys> |
| `bindgen` | `0.72.1` | BSD-3-Clause | Jyun-Yan You &lt;jyyou.tw@gmail.com&gt;, Emilio Cobos Álvarez &lt;emilio@crisal.io&gt;, Nick Fitzgerald &lt;fitzgen@gmail.com&gt;, The Servo project developers | <https://github.com/rust-lang/rust-bindgen> |
| `bitflags` | `1.3.2` | MIT/Apache-2.0 | The Rust Project Developers | <https://github.com/bitflags/bitflags> |
| `bitflags` | `2.13.2` | MIT OR Apache-2.0 | The Rust Project Developers | <https://github.com/bitflags/bitflags> |
| `block-buffer` | `0.10.4` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/utils> |
| `block2` | `0.5.1` | MIT | Steven Sheldon, Mads Marquart &lt;mads@marquart.dk&gt; | <https://github.com/madsmtm/objc2> |
| `bumpalo` | `3.20.3` | MIT OR Apache-2.0 | Nick Fitzgerald &lt;fitzgen@gmail.com&gt; | <https://github.com/fitzgen/bumpalo> |
| `bytemuck` | `1.25.2` | Zlib OR Apache-2.0 OR MIT | Lokathor &lt;zefria@gmail.com&gt; | <https://github.com/Lokathor/bytemuck> |
| `byteorder-lite` | `0.1.0` | Unlicense OR MIT | The crate authors | <https://github.com/image-rs/byteorder-lite> |
| `bytes` | `1.12.1` | MIT | Carl Lerche &lt;me@carllerche.com&gt;, Sean McArthur &lt;sean@seanmonstar.com&gt; | <https://github.com/tokio-rs/bytes> |
| `calloop` | `0.13.0` | MIT | Elinor Berger &lt;elinor@safaradeg.net&gt; | <https://github.com/Smithay/calloop> |
| `cc` | `1.11.0` | MIT OR Apache-2.0 | The crate authors | <https://github.com/rust-lang/cc-rs> |
| `cesu8` | `1.1.0` | Apache-2.0/MIT | Eric Kidd &lt;git@randomhacks.net&gt; | <https://github.com/emk/cesu8-rs> |
| `cexpr` | `0.6.0` | Apache-2.0/MIT | Jethro Beekman &lt;jethro@jbeekman.nl&gt; | <https://github.com/jethrogb/rust-cexpr> |
| `cfg-if` | `1.0.5` | MIT OR Apache-2.0 | Alex Crichton &lt;alex@alexcrichton.com&gt; | <https://github.com/rust-lang/cfg-if> |
| `cfg_aliases` | `0.2.2` | MIT | Zicklag &lt;zicklag@katharostech.com&gt; | <https://github.com/katharostech/cfg_aliases> |
| `cgl` | `0.3.2` | MIT / Apache-2.0 | The Servo Project Developers | <https://github.com/servo/cgl-rs> |
| `cipher` | `0.4.4` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/traits> |
| `clang-sys` | `1.9.1` | Apache-2.0 | Kyle Mayes &lt;kyle@mayeses.com&gt; | <https://github.com/KyleMayes/clang-sys> |
| `claxon` | `0.4.3` | Apache-2.0 | Ruud van Asseldonk &lt;dev@veniogames.com&gt; | <https://github.com/ruuda/claxon> |
| `combine` | `4.6.9` | MIT | Markus Westerlind &lt;marwes91@gmail.com&gt; | <https://github.com/Marwes/combine> |
| `concurrent-queue` | `2.5.0` | Apache-2.0 OR MIT | Stjepan Glavina &lt;stjepang@gmail.com&gt;, Taiki Endo &lt;te316e89@gmail.com&gt;, John Nunley &lt;dev@notgull.net&gt; | <https://github.com/smol-rs/concurrent-queue> |
| `const-oid` | `0.9.6` | Apache-2.0 OR MIT | RustCrypto Developers | <https://github.com/RustCrypto/formats/tree/master/const-oid> |
| `convert_case` | `0.6.0` | MIT | Rutrum &lt;dave@rutrum.net&gt; | <https://github.com/rutrum/convert-case> |
| `core-foundation` | `0.9.4` | MIT OR Apache-2.0 | The Servo Project Developers | <https://github.com/servo/core-foundation-rs> |
| `core-foundation-sys` | `0.8.7` | MIT OR Apache-2.0 | The Servo Project Developers | <https://github.com/servo/core-foundation-rs> |
| `core-graphics` | `0.23.2` | MIT OR Apache-2.0 | The Servo Project Developers | <https://github.com/servo/core-foundation-rs> |
| `core-graphics-types` | `0.1.3` | MIT OR Apache-2.0 | The Servo Project Developers | <https://github.com/servo/core-foundation-rs> |
| `coreaudio-rs` | `0.11.3` | MIT/Apache-2.0 | mitchmindtree &lt;mitchell.nordine@gmail.com&gt;, yupferris &lt;jake@fusetools.com&gt; | <https://github.com/RustAudio/coreaudio-rs.git> |
| `coreaudio-sys` | `0.2.18` | MIT | Mitchell Nordine &lt;mitchell.nordine@gmail.com&gt; | <https://github.com/RustAudio/coreaudio-sys.git> |
| `cpal` | `0.15.3` | Apache-2.0 | The crate authors | <https://github.com/rustaudio/cpal> |
| `cpufeatures` | `0.2.17` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/utils> |
| `crc32fast` | `1.5.4` | MIT OR Apache-2.0 | Sam Rijs &lt;srijs@airpost.net&gt;, Alex Crichton &lt;alex@alexcrichton.com&gt; | <https://github.com/srijs/rust-crc32fast> |
| `crossbeam-utils` | `0.8.23` | MIT OR Apache-2.0 | The crate authors | <https://github.com/crossbeam-rs/crossbeam> |
| `crypto-common` | `0.1.7` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/traits> |
| `ctor` | `0.2.9` | Apache-2.0 OR MIT | Matt Mastracci &lt;matthew@mastracci.com&gt; | <https://github.com/mmastrac/rust-ctor> |
| `ctr` | `0.9.2` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/block-modes> |
| `cursor-icon` | `1.2.0` | MIT OR Apache-2.0 OR Zlib | Kirill Chibisov &lt;contact@kchibisov.com&gt; | <https://github.com/rust-windowing/cursor-icon> |
| `curve25519-dalek` | `4.1.3` | BSD-3-Clause | Isis Lovecruft &lt;isis@patternsinthevoid.net&gt;, Henry de Valence &lt;hdevalence@hdevalence.ca&gt; | <https://github.com/dalek-cryptography/curve25519-dalek/tree/main/curve25519-dalek> |
| `curve25519-dalek-derive` | `0.1.1` | MIT/Apache-2.0 | The crate authors | <https://github.com/dalek-cryptography/curve25519-dalek> |
| `dasp_sample` | `0.11.0` | MIT OR Apache-2.0 | mitchmindtree &lt;mitchell.nordine@gmail.com&gt; | <https://github.com/rustaudio/sample.git> |
| `data-encoding` | `2.11.1` | MIT | The crate authors | <https://github.com/ia0/data-encoding> |
| `der` | `0.7.10` | Apache-2.0 OR MIT | RustCrypto Developers | <https://github.com/RustCrypto/formats/tree/master/der> |
| `digest` | `0.10.7` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/traits> |
| `dispatch` | `0.2.0` | MIT | Steven Sheldon | <http://github.com/SSheldon/rust-dispatch> |
| `dispatch2` | `0.3.1` | Zlib OR Apache-2.0 OR MIT | Mads Marquart &lt;mads@marquart.dk&gt;, Mary &lt;mary@mary.zone&gt; | <https://github.com/madsmtm/objc2> |
| `dlib` | `0.5.3` | MIT | Elinor Berger &lt;elinor@safaradeg.net&gt; | <https://github.com/elinorbgr/dlib> |
| `dpi` | `0.1.2` | Apache-2.0 AND MIT | The crate authors | <https://github.com/rust-windowing/winit> |
| `ed25519` | `2.2.3` | Apache-2.0 OR MIT | RustCrypto Developers | <https://github.com/RustCrypto/signatures/tree/master/ed25519> |
| `ed25519-dalek` | `2.2.0` | BSD-3-Clause | isis lovecruft &lt;isis@patternsinthevoid.net&gt;, Tony Arcieri &lt;bascule@gmail.com&gt;, Michael Rosenberg &lt;michael@mrosenberg.pub&gt; | <https://github.com/dalek-cryptography/curve25519-dalek/tree/main/ed25519-dalek> |
| `either` | `1.19.0` | MIT OR Apache-2.0 | The crate authors | <https://github.com/rayon-rs/either> |
| `equivalent` | `1.0.3` | Apache-2.0 OR MIT | The crate authors | <https://github.com/indexmap-rs/equivalent> |
| `errno` | `0.3.14` | MIT OR Apache-2.0 | Chris Wong &lt;lambda.fairy@gmail.com&gt;, Dan Gohman &lt;dev@sunfishcode.online&gt; | <https://github.com/lambda-fairy/rust-errno> |
| `fallible-iterator` | `0.3.0` | MIT/Apache-2.0 | Steven Fackler &lt;sfackler@gmail.com&gt; | <https://github.com/sfackler/rust-fallible-iterator> |
| `fallible-streaming-iterator` | `0.1.9` | MIT/Apache-2.0 | Steven Fackler &lt;sfackler@gmail.com&gt; | <https://github.com/sfackler/fallible-streaming-iterator> |
| `fdeflate` | `0.3.7` | MIT OR Apache-2.0 | The image-rs Developers | <https://github.com/image-rs/fdeflate> |
| `fiat-crypto` | `0.2.9` | MIT OR Apache-2.0 OR BSD-1-Clause | Fiat Crypto library authors &lt;jgross@mit.edu&gt; | <https://github.com/mit-plv/fiat-crypto> |
| `find-msvc-tools` | `0.1.15` | MIT OR Apache-2.0 | The crate authors | <https://github.com/rust-lang/cc-rs> |
| `flate2` | `1.1.10` | MIT OR Apache-2.0 | Alex Crichton &lt;alex@alexcrichton.com&gt;, Josh Triplett &lt;josh@joshtriplett.org&gt; | <https://github.com/rust-lang/flate2-rs> |
| `foreign-types` | `0.5.0` | MIT/Apache-2.0 | Steven Fackler &lt;sfackler@gmail.com&gt; | <https://github.com/sfackler/foreign-types> |
| `foreign-types-macros` | `0.2.4` | MIT/Apache-2.0 | Steven Fackler &lt;sfackler@gmail.com&gt; | <https://github.com/sfackler/foreign-types> |
| `foreign-types-shared` | `0.3.1` | MIT/Apache-2.0 | Steven Fackler &lt;sfackler@gmail.com&gt; | <https://github.com/sfackler/foreign-types> |
| `futures-core` | `0.3.34` | MIT OR Apache-2.0 | The crate authors | <https://github.com/rust-lang/futures-rs> |
| `futures-task` | `0.3.34` | MIT OR Apache-2.0 | The crate authors | <https://github.com/rust-lang/futures-rs> |
| `futures-util` | `0.3.34` | MIT OR Apache-2.0 | The crate authors | <https://github.com/rust-lang/futures-rs> |
| `generic-array` | `0.14.7` | MIT | Bartłomiej Kamiński &lt;fizyk20@gmail.com&gt;, Aaron Trent &lt;novacrazy@gmail.com&gt; | <https://github.com/fizyk20/generic-array.git> |
| `gethostname` | `1.1.0` | Apache-2.0 | Sebastian Wiesner &lt;sebastian@swsnr.de&gt; | <https://codeberg.org/swsnr/gethostname.rs.git> |
| `getrandom` | `0.2.17` | MIT OR Apache-2.0 | The Rand Project Developers | <https://github.com/rust-random/getrandom> |
| `getrandom` | `0.3.4` | MIT OR Apache-2.0 | The Rand Project Developers | <https://github.com/rust-random/getrandom> |
| `getrandom` | `0.4.3` | MIT OR Apache-2.0 | The Rand Project Developers | <https://github.com/rust-random/getrandom> |
| `ghash` | `0.5.1` | Apache-2.0 OR MIT | RustCrypto Developers | <https://github.com/RustCrypto/universal-hashes> |
| `gl_generator` | `0.14.0` | Apache-2.0 | Brendan Zabarauskas &lt;bjzaba@yahoo.com.au&gt;, Corey Richardson, Arseny Kapoulkine | <https://github.com/brendanzab/gl-rs/> |
| `glob` | `0.3.4` | MIT OR Apache-2.0 | The Rust Project Developers | <https://github.com/rust-lang/glob> |
| `glow` | `0.18.0` | MIT OR Apache-2.0 OR Zlib | Joshua Groves &lt;josh@joshgroves.com&gt;, Dzmitry Malyshau &lt;kvarkus@gmail.com&gt; | <https://github.com/grovesNL/glow> |
| `glutin` | `0.32.3` | Apache-2.0 | Kirill Chibisov &lt;contact@kchibisov.com&gt; | <https://github.com/rust-windowing/glutin> |
| `glutin-winit` | `0.5.0` | MIT | Kirill Chibisov &lt;contact@kchibisov.com&gt; | <https://github.com/rust-windowing/glutin> |
| `glutin_egl_sys` | `0.7.1` | Apache-2.0 | Kirill Chibisov &lt;contact@kchibisov.com&gt; | <https://github.com/rust-windowing/glutin> |
| `glutin_glx_sys` | `0.6.1` | Apache-2.0 | Kirill Chibisov &lt;contact@kchibisov.com&gt; | <https://github.com/rust-windowing/glutin> |
| `glutin_wgl_sys` | `0.6.1` | Apache-2.0 | Kirill Chibisov &lt;contact@kchibisov.com&gt; | <https://github.com/rust-windowing/glutin> |
| `hashbrown` | `0.14.5` | MIT OR Apache-2.0 | Amanieu d'Antras &lt;amanieu@gmail.com&gt; | <https://github.com/rust-lang/hashbrown> |
| `hashbrown` | `0.17.1` | MIT OR Apache-2.0 | The crate authors | <https://github.com/rust-lang/hashbrown> |
| `hashlink` | `0.9.1` | MIT OR Apache-2.0 | kyren &lt;kerriganw@gmail.com&gt; | <https://github.com/kyren/hashlink> |
| `hermit-abi` | `0.5.3` | MIT OR Apache-2.0 | The crate authors | <https://github.com/hermit-os/hermit-rs> |
| `hkdf` | `0.12.4` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/KDFs/> |
| `hmac` | `0.12.1` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/MACs> |
| `http` | `1.5.0` | MIT OR Apache-2.0 | Alex Crichton &lt;alex@alexcrichton.com&gt;, Carl Lerche &lt;me@carllerche.com&gt;, Sean McArthur &lt;sean@seanmonstar.com&gt; | <https://github.com/hyperium/http> |
| `httparse` | `1.10.1` | MIT OR Apache-2.0 | Sean McArthur &lt;sean@seanmonstar.com&gt; | <https://github.com/seanmonstar/httparse> |
| `image-webp` | `0.2.4` | MIT OR Apache-2.0 | The crate authors | <https://github.com/image-rs/image-webp> |
| `indexmap` | `2.14.2` | Apache-2.0 OR MIT | The crate authors | <https://github.com/indexmap-rs/indexmap> |
| `inout` | `0.1.4` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/utils> |
| `itertools` | `0.13.0` | MIT OR Apache-2.0 | bluss | <https://github.com/rust-itertools/itertools> |
| `itoa` | `1.0.18` | MIT OR Apache-2.0 | David Tolnay &lt;dtolnay@gmail.com&gt; | <https://github.com/dtolnay/itoa> |
| `jni` | `0.21.1` | MIT/Apache-2.0 | Josh Chase &lt;josh@prevoty.com&gt; | <https://github.com/jni-rs/jni-rs> |
| `jni` | `0.22.4` | MIT OR Apache-2.0 | jni team | <https://github.com/jni-rs/jni-rs> |
| `jni-macros` | `0.22.4` | MIT OR Apache-2.0 | The crate authors | <https://github.com/jni-rs/jni-rs> |
| `jni-sys` | `0.3.1` | MIT OR Apache-2.0 | Steven Fackler &lt;sfackler@gmail.com&gt; | <https://github.com/jni-rs/jni-sys> |
| `jni-sys` | `0.4.1` | MIT OR Apache-2.0 | Steven Fackler &lt;sfackler@gmail.com&gt;, Robert Bragg &lt;robert@sixbynine.org&gt; | <https://github.com/jni-rs/jni-sys> |
| `jni-sys-macros` | `0.4.1` | MIT OR Apache-2.0 | Robert Bragg &lt;robert@sixbynine.org&gt; | <https://github.com/jni-rs/jni-sys> |
| `jobserver` | `0.1.35` | MIT OR Apache-2.0 | Alex Crichton &lt;alex@alexcrichton.com&gt; | <https://github.com/rust-lang/jobserver-rs> |
| `js-sys` | `0.3.107` | MIT OR Apache-2.0 | The wasm-bindgen Developers | <https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/js-sys> |
| `khronos_api` | `3.1.0` | Apache-2.0 | Brendan Zabarauskas &lt;bjzaba@yahoo.com.au&gt;, Corey Richardson, Arseny Kapoulkine, Pierre Krieger &lt;pierre.krieger1708@gmail.com&gt; | <https://github.com/brendanzab/gl-rs/> |
| `libc` | `0.2.191` | MIT OR Apache-2.0 | The crate authors | <https://github.com/rust-lang/libc> |
| `libloading` | `0.8.9` | ISC | Simonas Kazlauskas &lt;libloading@kazlauskas.me&gt; | <https://github.com/nagisa/rust_libloading/> |
| `libredox` | `0.1.25` | MIT | 4lDO2 &lt;4lDO2@protonmail.com&gt; | <https://gitlab.redox-os.org/redox-os/libredox.git> |
| `libsqlite3-sys` | `0.30.1` | MIT | The rusqlite developers | <https://github.com/rusqlite/rusqlite> |
| `linux-raw-sys` | `0.12.1` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Dan Gohman &lt;dev@sunfishcode.online&gt; | <https://github.com/sunfishcode/linux-raw-sys> |
| `linux-raw-sys` | `0.4.15` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Dan Gohman &lt;dev@sunfishcode.online&gt; | <https://github.com/sunfishcode/linux-raw-sys> |
| `log` | `0.4.35` | MIT OR Apache-2.0 | The Rust Project Developers | <https://github.com/rust-lang/log> |
| `mach2` | `0.4.3` | BSD-2-Clause OR MIT OR Apache-2.0 | The crate authors | <https://github.com/JohnTitor/mach2> |
| `memchr` | `2.8.3` | Unlicense OR MIT | Andrew Gallant &lt;jamslam@gmail.com&gt;, bluss | <https://github.com/BurntSushi/memchr> |
| `minimal-lexical` | `0.2.1` | MIT/Apache-2.0 | Alex Huszagh &lt;ahuszagh@gmail.com&gt; | <https://github.com/Alexhuszagh/minimal-lexical> |
| `miniz_oxide` | `0.8.9` | MIT OR Zlib OR Apache-2.0 | Frommi &lt;daniil.liferenko@gmail.com&gt;, oyvindln &lt;oyvindln@users.noreply.github.com&gt;, Rich Geldreich richgel99@gmail.com | <https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide> |
| `miniz_oxide` | `0.9.1` | MIT OR Zlib OR Apache-2.0 | Frommi &lt;daniil.liferenko@gmail.com&gt;, oyvindln &lt;oyvindln@users.noreply.github.com&gt;, Rich Geldreich richgel99@gmail.com | <https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide> |
| `napi` | `2.16.17` | MIT | Nathan Sobo &lt;nathan@github.com&gt;, Yinan Long &lt;lynweklm@gmail.com&gt; | <https://github.com/napi-rs/napi-rs> |
| `napi-build` | `2.3.1` | MIT | LongYinan &lt;lynweklm@gmail.com&gt; | <https://github.com/napi-rs/napi-rs> |
| `napi-derive` | `2.16.13` | MIT | LongYinan &lt;lynweklm@gmail.com&gt;, Forehalo &lt;forehalo@gmail.com&gt; | <https://github.com/napi-rs/napi-rs> |
| `napi-derive-backend` | `1.0.75` | MIT | The crate authors | <https://github.com/napi-rs/napi-rs> |
| `napi-sys` | `2.4.0` | MIT | LongYinan &lt;lynweklm@gmail.com&gt; | <https://github.com/napi-rs/napi-rs> |
| `ndk` | `0.8.0` | MIT OR Apache-2.0 | The Rust Mobile contributors | <https://github.com/rust-mobile/ndk> |
| `ndk` | `0.9.0` | MIT OR Apache-2.0 | The Rust Mobile contributors | <https://github.com/rust-mobile/ndk> |
| `ndk-context` | `0.1.1` | MIT OR Apache-2.0 | The Rust Windowing contributors | <https://github.com/rust-windowing/android-ndk-rs> |
| `ndk-sys` | `0.5.0+25.2.9519653` | MIT OR Apache-2.0 | The Rust Windowing contributors | <https://github.com/rust-mobile/ndk> |
| `ndk-sys` | `0.6.0+11769913` | MIT OR Apache-2.0 | The Rust Windowing contributors | <https://github.com/rust-mobile/ndk> |
| `nom` | `7.1.3` | MIT | contact@geoffroycouprie.com | <https://github.com/Geal/nom> |
| `num-derive` | `0.4.2` | MIT OR Apache-2.0 | The Rust Project Developers | <https://github.com/rust-num/num-derive> |
| `num-traits` | `0.2.19` | MIT OR Apache-2.0 | The Rust Project Developers | <https://github.com/rust-num/num-traits> |
| `num_enum` | `0.7.6` | BSD-3-Clause OR MIT OR Apache-2.0 | Daniel Wagner-Hall &lt;dawagner@gmail.com&gt;, Daniel Henry-Mantilla &lt;daniel.henry.mantilla@gmail.com&gt;, Vincent Esche &lt;regexident@gmail.com&gt; | <https://github.com/illicitonion/num_enum> |
| `num_enum_derive` | `0.7.6` | BSD-3-Clause OR MIT OR Apache-2.0 | Daniel Wagner-Hall &lt;dawagner@gmail.com&gt;, Daniel Henry-Mantilla &lt;daniel.henry.mantilla@gmail.com&gt;, Vincent Esche &lt;regexident@gmail.com&gt; | <https://github.com/illicitonion/num_enum> |
| `objc-sys` | `0.3.5` | MIT | Mads Marquart &lt;mads@marquart.dk&gt; | <https://github.com/madsmtm/objc2> |
| `objc2` | `0.5.3` | MIT | Steven Sheldon, Mads Marquart &lt;mads@marquart.dk&gt; | <https://github.com/madsmtm/objc2> |
| `objc2` | `0.6.5` | MIT | Mads Marquart &lt;mads@marquart.dk&gt; | <https://github.com/madsmtm/objc2> |
| `objc2-app-kit` | `0.2.2` | MIT | The crate authors | <https://github.com/madsmtm/objc2> |
| `objc2-app-kit` | `0.3.2` | Zlib OR Apache-2.0 OR MIT | The crate authors | <https://github.com/madsmtm/objc2> |
| `objc2-cloud-kit` | `0.2.2` | MIT | The crate authors | <https://github.com/madsmtm/objc2> |
| `objc2-contacts` | `0.2.2` | MIT | The crate authors | <https://github.com/madsmtm/objc2> |
| `objc2-core-data` | `0.2.2` | MIT | The crate authors | <https://github.com/madsmtm/objc2> |
| `objc2-core-foundation` | `0.3.2` | Zlib OR Apache-2.0 OR MIT | The crate authors | <https://github.com/madsmtm/objc2> |
| `objc2-core-image` | `0.2.2` | MIT | The crate authors | <https://github.com/madsmtm/objc2> |
| `objc2-core-location` | `0.2.2` | MIT | The crate authors | <https://github.com/madsmtm/objc2> |
| `objc2-encode` | `4.1.0` | MIT | Mads Marquart &lt;mads@marquart.dk&gt; | <https://github.com/madsmtm/objc2> |
| `objc2-foundation` | `0.2.2` | MIT | The crate authors | <https://github.com/madsmtm/objc2> |
| `objc2-foundation` | `0.3.2` | MIT | The crate authors | <https://github.com/madsmtm/objc2> |
| `objc2-link-presentation` | `0.2.2` | MIT | The crate authors | <https://github.com/madsmtm/objc2> |
| `objc2-metal` | `0.2.2` | MIT | The crate authors | <https://github.com/madsmtm/objc2> |
| `objc2-quartz-core` | `0.2.2` | MIT | The crate authors | <https://github.com/madsmtm/objc2> |
| `objc2-symbols` | `0.2.2` | MIT | The crate authors | <https://github.com/madsmtm/objc2> |
| `objc2-ui-kit` | `0.2.2` | MIT | The crate authors | <https://github.com/madsmtm/objc2> |
| `objc2-uniform-type-identifiers` | `0.2.2` | MIT | The crate authors | <https://github.com/madsmtm/objc2> |
| `objc2-user-notifications` | `0.2.2` | MIT | The crate authors | <https://github.com/madsmtm/objc2> |
| `oboe` | `0.6.1` | Apache-2.0 | K. &lt;kayo@illumium.org&gt; | <https://github.com/katyo/oboe-rs> |
| `oboe-sys` | `0.6.1` | Apache-2.0 | K. &lt;kayo@illumium.org&gt; | <https://github.com/katyo/oboe-rs> |
| `once_cell` | `1.21.4` | MIT OR Apache-2.0 | Aleksey Kladov &lt;aleksey.kladov@gmail.com&gt; | <https://github.com/matklad/once_cell> |
| `opaque-debug` | `0.3.1` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/utils> |
| `opus-pure` | `0.2.2` | BSD-3-Clause | The crate authors | <https://github.com/stephenberry/opus-pure> |
| `orbclient` | `0.3.55` | MIT | Jeremy Soller &lt;jackpot51@gmail.com&gt; | <https://gitlab.redox-os.org/redox-os/orbclient> |
| `percent-encoding` | `2.3.2` | MIT OR Apache-2.0 | The rust-url developers | <https://github.com/servo/rust-url/> |
| `pin-project` | `1.1.13` | Apache-2.0 OR MIT | The crate authors | <https://github.com/taiki-e/pin-project> |
| `pin-project-internal` | `1.1.13` | Apache-2.0 OR MIT | The crate authors | <https://github.com/taiki-e/pin-project> |
| `pin-project-lite` | `0.2.17` | Apache-2.0 OR MIT | The crate authors | <https://github.com/taiki-e/pin-project-lite> |
| `pkcs8` | `0.10.2` | Apache-2.0 OR MIT | RustCrypto Developers | <https://github.com/RustCrypto/formats/tree/master/pkcs8> |
| `pkg-config` | `0.3.34` | MIT OR Apache-2.0 | Alex Crichton &lt;alex@alexcrichton.com&gt; | <https://github.com/rust-lang/pkg-config-rs> |
| `plain` | `0.2.3` | MIT/Apache-2.0 | jzr | <https://github.com/randomites/plain> |
| `png` | `0.18.1` | MIT OR Apache-2.0 | The image-rs Developers | <https://github.com/image-rs/image-png> |
| `polling` | `3.11.0` | Apache-2.0 OR MIT | Stjepan Glavina &lt;stjepang@gmail.com&gt;, John Nunley &lt;dev@notgull.net&gt; | <https://github.com/smol-rs/polling> |
| `polyval` | `0.6.2` | Apache-2.0 OR MIT | RustCrypto Developers | <https://github.com/RustCrypto/universal-hashes> |
| `ppv-lite86` | `0.2.21` | MIT OR Apache-2.0 | The CryptoCorrosion Contributors | <https://github.com/cryptocorrosion/cryptocorrosion> |
| `proc-macro-crate` | `3.5.0` | MIT OR Apache-2.0 | Bastian Köcher &lt;git@kchr.de&gt; | <https://github.com/bkchr/proc-macro-crate> |
| `proc-macro2` | `1.0.107` | MIT OR Apache-2.0 | David Tolnay &lt;dtolnay@gmail.com&gt;, Alex Crichton &lt;alex@alexcrichton.com&gt; | <https://github.com/dtolnay/proc-macro2> |
| `quick-error` | `2.0.1` | MIT/Apache-2.0 | Paul Colomiets &lt;paul@colomiets.name&gt;, Colin Kiegel &lt;kiegel@gmx.de&gt; | <http://github.com/tailhook/quick-error> |
| `quote` | `1.0.47` | MIT OR Apache-2.0 | David Tolnay &lt;dtolnay@gmail.com&gt; | <https://github.com/dtolnay/quote> |
| `r-efi` | `5.3.0` | MIT OR Apache-2.0 OR LGPL-2.1-or-later | The crate authors | <https://github.com/r-efi/r-efi> |
| `r-efi` | `6.0.0` | MIT OR Apache-2.0 OR LGPL-2.1-or-later | The crate authors | <https://github.com/r-efi/r-efi> |
| `rand` | `0.9.5` | MIT OR Apache-2.0 | The Rand Project Developers, The Rust Project Developers | <https://github.com/rust-random/rand> |
| `rand_chacha` | `0.9.0` | MIT OR Apache-2.0 | The Rand Project Developers, The Rust Project Developers, The CryptoCorrosion Contributors | <https://github.com/rust-random/rand> |
| `rand_core` | `0.6.4` | MIT OR Apache-2.0 | The Rand Project Developers, The Rust Project Developers | <https://github.com/rust-random/rand> |
| `rand_core` | `0.9.5` | MIT OR Apache-2.0 | The Rand Project Developers, The Rust Project Developers | <https://github.com/rust-random/rand> |
| `raw-window-handle` | `0.6.2` | MIT OR Apache-2.0 OR Zlib | Osspial &lt;osspial@gmail.com&gt; | <https://github.com/rust-windowing/raw-window-handle> |
| `redox_syscall` | `0.4.1` | MIT | Jeremy Soller &lt;jackpot51@gmail.com&gt; | <https://gitlab.redox-os.org/redox-os/syscall> |
| `redox_syscall` | `0.9.5` | MIT | Jeremy Soller &lt;jackpot51@gmail.com&gt; | <https://gitlab.redox-os.org/redox-os/kernel> |
| `regex` | `1.13.1` | MIT OR Apache-2.0 | The Rust Project Developers, Andrew Gallant &lt;jamslam@gmail.com&gt; | <https://github.com/rust-lang/regex> |
| `regex-automata` | `0.4.18` | MIT OR Apache-2.0 | The Rust Project Developers, Andrew Gallant &lt;jamslam@gmail.com&gt; | <https://github.com/rust-lang/regex> |
| `regex-syntax` | `0.8.11` | MIT OR Apache-2.0 | The Rust Project Developers, Andrew Gallant &lt;jamslam@gmail.com&gt; | <https://github.com/rust-lang/regex> |
| `ring` | `0.17.14` | Apache-2.0 AND ISC | The crate authors | <https://github.com/briansmith/ring> |
| `rusqlite` | `0.32.1` | MIT | The rusqlite developers | <https://github.com/rusqlite/rusqlite> |
| `rustc-hash` | `2.1.3` | Apache-2.0 OR MIT | The Rust Project Developers | <https://github.com/rust-lang/rustc-hash> |
| `rustc_version` | `0.4.1` | MIT OR Apache-2.0 | The crate authors | <https://github.com/djc/rustc-version-rs> |
| `rustix` | `0.38.44` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Dan Gohman &lt;dev@sunfishcode.online&gt;, Jakub Konka &lt;kubkon@jakubkonka.com&gt; | <https://github.com/bytecodealliance/rustix> |
| `rustix` | `1.1.6` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Dan Gohman &lt;dev@sunfishcode.online&gt;, Jakub Konka &lt;kubkon@jakubkonka.com&gt; | <https://github.com/bytecodealliance/rustix> |
| `rustls` | `0.23.46` | Apache-2.0 OR ISC OR MIT | The crate authors | <https://github.com/rustls/rustls> |
| `rustls-pki-types` | `1.15.1` | MIT OR Apache-2.0 | The crate authors | <https://github.com/rustls/pki-types> |
| `rustls-webpki` | `0.103.16` | ISC | The crate authors | <https://github.com/rustls/webpki> |
| `rustversion` | `1.0.23` | MIT OR Apache-2.0 | David Tolnay &lt;dtolnay@gmail.com&gt; | <https://github.com/dtolnay/rustversion> |
| `same-file` | `1.0.6` | Unlicense/MIT | Andrew Gallant &lt;jamslam@gmail.com&gt; | <https://github.com/BurntSushi/same-file> |
| `semver` | `1.0.28` | MIT OR Apache-2.0 | David Tolnay &lt;dtolnay@gmail.com&gt; | <https://github.com/dtolnay/semver> |
| `serde` | `1.0.229` | MIT OR Apache-2.0 | Erick Tryzelaar &lt;erick.tryzelaar@gmail.com&gt;, David Tolnay &lt;dtolnay@gmail.com&gt; | <https://github.com/serde-rs/serde> |
| `serde_core` | `1.0.229` | MIT OR Apache-2.0 | Erick Tryzelaar &lt;erick.tryzelaar@gmail.com&gt;, David Tolnay &lt;dtolnay@gmail.com&gt; | <https://github.com/serde-rs/serde> |
| `serde_derive` | `1.0.229` | MIT OR Apache-2.0 | Erick Tryzelaar &lt;erick.tryzelaar@gmail.com&gt;, David Tolnay &lt;dtolnay@gmail.com&gt; | <https://github.com/serde-rs/serde> |
| `serde_json` | `1.0.154` | MIT OR Apache-2.0 | The crate authors | <https://github.com/serde-rs/json> |
| `sha1` | `0.10.7` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/hashes> |
| `sha2` | `0.10.9` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/hashes> |
| `shlex` | `1.3.0` | MIT OR Apache-2.0 | comex &lt;comexk@gmail.com&gt;, Fenhl &lt;fenhl@fenhl.net&gt;, Adrian Taylor &lt;adetaylor@chromium.org&gt;, Alex Touchet &lt;alextouchet@outlook.com&gt;, Daniel Parks &lt;dp+git@oxidized.org&gt;, Garrett Berg &lt;googberg@gmail.com&gt; | <https://github.com/comex/rust-shlex> |
| `shlex` | `2.0.1` | MIT OR Apache-2.0 | comex &lt;comexk@gmail.com&gt;, Fenhl &lt;fenhl@fenhl.net&gt;, Adrian Taylor &lt;adetaylor@chromium.org&gt;, Alex Touchet &lt;alextouchet@outlook.com&gt;, Daniel Parks &lt;dp+git@oxidized.org&gt;, Garrett Berg &lt;googberg@gmail.com&gt; | <https://github.com/comex/rust-shlex> |
| `signature` | `2.2.0` | Apache-2.0 OR MIT | RustCrypto Developers | <https://github.com/RustCrypto/traits/tree/master/signature> |
| `simd-adler32` | `0.3.10` | MIT | Marvin Countryman &lt;me@maar.vin&gt; | <https://github.com/mcountryman/simd-adler32> |
| `simd_cesu8` | `1.2.0` | Apache-2.0 OR MIT | Sean C. Roach &lt;me@seancroach.dev&gt; | <https://github.com/seancroach/simd_cesu8> |
| `simdutf8` | `0.1.5` | MIT OR Apache-2.0 | Hans Kratz &lt;hans@appfour.com&gt; | <https://github.com/rusticstuff/simdutf8> |
| `slab` | `0.4.12` | MIT | Carl Lerche &lt;me@carllerche.com&gt; | <https://github.com/tokio-rs/slab> |
| `slotmap` | `1.1.1` | Zlib | Orson Peters &lt;orsonpeters@gmail.com&gt; | <https://github.com/orlp/slotmap> |
| `smallvec` | `1.16.5` | MIT OR Apache-2.0 | The Servo Project Developers | <https://github.com/servo/rust-smallvec> |
| `smol_str` | `0.2.2` | MIT OR Apache-2.0 | Aleksey Kladov &lt;aleksey.kladov@gmail.com&gt; | <https://github.com/rust-analyzer/smol_str> |
| `spki` | `0.7.3` | Apache-2.0 OR MIT | RustCrypto Developers | <https://github.com/RustCrypto/formats/tree/master/spki> |
| `subtle` | `2.6.1` | BSD-3-Clause | Isis Lovecruft &lt;isis@patternsinthevoid.net&gt;, Henry de Valence &lt;hdevalence@hdevalence.ca&gt; | <https://github.com/dalek-cryptography/subtle> |
| `syn` | `2.0.119` | MIT OR Apache-2.0 | David Tolnay &lt;dtolnay@gmail.com&gt; | <https://github.com/dtolnay/syn> |
| `syn` | `3.0.9` | MIT OR Apache-2.0 | David Tolnay &lt;dtolnay@gmail.com&gt; | <https://github.com/dtolnay/syn> |
| `thiserror` | `1.0.69` | MIT OR Apache-2.0 | David Tolnay &lt;dtolnay@gmail.com&gt; | <https://github.com/dtolnay/thiserror> |
| `thiserror` | `2.0.21` | MIT OR Apache-2.0 | David Tolnay &lt;dtolnay@gmail.com&gt; | <https://github.com/dtolnay/thiserror> |
| `thiserror-impl` | `1.0.69` | MIT OR Apache-2.0 | David Tolnay &lt;dtolnay@gmail.com&gt; | <https://github.com/dtolnay/thiserror> |
| `thiserror-impl` | `2.0.21` | MIT OR Apache-2.0 | David Tolnay &lt;dtolnay@gmail.com&gt; | <https://github.com/dtolnay/thiserror> |
| `tokio` | `1.54.1` | MIT | Tokio Contributors &lt;team@tokio.rs&gt; | <https://github.com/tokio-rs/tokio> |
| `toml_datetime` | `1.1.3+spec-1.1.0` | MIT OR Apache-2.0 | The crate authors | <https://github.com/toml-rs/toml> |
| `toml_edit` | `0.25.19+spec-1.1.0` | MIT OR Apache-2.0 | The crate authors | <https://github.com/toml-rs/toml> |
| `toml_parser` | `1.1.5+spec-1.1.0` | MIT OR Apache-2.0 | The crate authors | <https://github.com/toml-rs/toml> |
| `tracing` | `0.1.44` | MIT | Eliza Weisman &lt;eliza@buoyant.io&gt;, Tokio Contributors &lt;team@tokio.rs&gt; | <https://github.com/tokio-rs/tracing> |
| `tracing-core` | `0.1.36` | MIT | Tokio Contributors &lt;team@tokio.rs&gt; | <https://github.com/tokio-rs/tracing> |
| `tungstenite` | `0.26.2` | MIT OR Apache-2.0 | Alexey Galakhov, Daniel Abramov | <https://github.com/snapview/tungstenite-rs> |
| `typenum` | `1.20.1` | MIT OR Apache-2.0 | The crate authors | <https://github.com/paholg/typenum> |
| `unicode-ident` | `1.0.27` | (MIT OR Apache-2.0) AND Unicode-3.0 | David Tolnay &lt;dtolnay@gmail.com&gt; | <https://github.com/dtolnay/unicode-ident> |
| `unicode-segmentation` | `1.13.3` | MIT OR Apache-2.0 | kwantam &lt;kwantam@gmail.com&gt;, Manish Goregaokar &lt;manishsmail@gmail.com&gt; | <https://github.com/unicode-rs/unicode-segmentation> |
| `universal-hash` | `0.5.1` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/traits> |
| `untrusted` | `0.9.0` | ISC | Brian Smith &lt;brian@briansmith.org&gt; | <https://github.com/briansmith/untrusted> |
| `ureq` | `3.5.1` | MIT OR Apache-2.0 | Martin Algesten &lt;martin@algesten.se&gt;, Jacob Hoffman-Andrews &lt;ureq@hoffman-andrews.com&gt; | <https://github.com/algesten/ureq> |
| `ureq-proto` | `0.7.1` | MIT OR Apache-2.0 | Martin Algesten &lt;martin@algesten.se&gt; | <https://github.com/algesten/ureq-proto> |
| `utf-8` | `0.7.6` | MIT OR Apache-2.0 | Simon Sapin &lt;simon.sapin@exyr.org&gt; | <https://github.com/SimonSapin/rust-utf8> |
| `utf8-zero` | `0.8.1` | MIT OR Apache-2.0 | Simon Sapin &lt;simon.sapin@exyr.org&gt;, Martin Algesten &lt;martin@algesten.se&gt; | <https://github.com/algesten/utf8-zero> |
| `vcpkg` | `0.2.15` | MIT/Apache-2.0 | Jim McGrath &lt;jimmc2@gmail.com&gt; | <https://github.com/mcgoo/vcpkg-rs> |
| `version_check` | `0.9.5` | MIT/Apache-2.0 | Sergio Benitez &lt;sb@sergio.bz&gt; | <https://github.com/SergioBenitez/version_check> |
| `walkdir` | `2.5.0` | Unlicense/MIT | Andrew Gallant &lt;jamslam@gmail.com&gt; | <https://github.com/BurntSushi/walkdir> |
| `wasi` | `0.11.1+wasi-snapshot-preview1` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | The Cranelift Project Developers | <https://github.com/bytecodealliance/wasi> |
| `wasip2` | `1.0.4+wasi-0.2.12` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | The crate authors | <https://github.com/bytecodealliance/wasi-rs> |
| `wasm-bindgen` | `0.2.130` | MIT OR Apache-2.0 | The wasm-bindgen Developers | <https://github.com/wasm-bindgen/wasm-bindgen> |
| `wasm-bindgen-futures` | `0.4.80` | MIT OR Apache-2.0 | The wasm-bindgen Developers | <https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/futures> |
| `wasm-bindgen-macro` | `0.2.130` | MIT OR Apache-2.0 | The wasm-bindgen Developers | <https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/macro> |
| `wasm-bindgen-macro-support` | `0.2.130` | MIT OR Apache-2.0 | The wasm-bindgen Developers | <https://github.com/wasm-bindgen/wasm-bindgen/tree/main/crates/macro-support> |
| `wasm-bindgen-shared` | `0.2.130` | MIT OR Apache-2.0 | The wasm-bindgen Developers | <https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/shared> |
| `web-sys` | `0.3.107` | MIT OR Apache-2.0 | The wasm-bindgen Developers | <https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/web-sys> |
| `web-time` | `1.1.0` | MIT OR Apache-2.0 | The crate authors | <https://github.com/daxpedda/web-time> |
| `webpki-roots` | `0.26.11` | CDLA-Permissive-2.0 | The crate authors | <https://github.com/rustls/webpki-roots> |
| `webpki-roots` | `1.0.11` | CDLA-Permissive-2.0 | The crate authors | <https://github.com/rustls/webpki-roots> |
| `winapi-util` | `0.1.11` | Unlicense OR MIT | Andrew Gallant &lt;jamslam@gmail.com&gt; | <https://github.com/BurntSushi/winapi-util> |
| `windows` | `0.54.0` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `windows-core` | `0.54.0` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `windows-link` | `0.2.1` | MIT OR Apache-2.0 | The crate authors | <https://github.com/microsoft/windows-rs> |
| `windows-result` | `0.1.2` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `windows-sys` | `0.45.0` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `windows-sys` | `0.52.0` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `windows-sys` | `0.61.2` | MIT OR Apache-2.0 | The crate authors | <https://github.com/microsoft/windows-rs> |
| `windows-targets` | `0.42.2` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `windows-targets` | `0.52.6` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `windows_aarch64_gnullvm` | `0.42.2` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `windows_aarch64_gnullvm` | `0.52.6` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `windows_aarch64_msvc` | `0.42.2` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `windows_aarch64_msvc` | `0.52.6` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `windows_i686_gnu` | `0.42.2` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `windows_i686_gnu` | `0.52.6` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `windows_i686_gnullvm` | `0.52.6` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `windows_i686_msvc` | `0.42.2` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `windows_i686_msvc` | `0.52.6` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `windows_x86_64_gnu` | `0.42.2` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `windows_x86_64_gnu` | `0.52.6` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `windows_x86_64_gnullvm` | `0.42.2` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `windows_x86_64_gnullvm` | `0.52.6` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `windows_x86_64_msvc` | `0.42.2` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `windows_x86_64_msvc` | `0.52.6` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `winit` | `0.30.13` | Apache-2.0 | The winit contributors, Pierre Krieger &lt;pierre.krieger1708@gmail.com&gt; | <https://github.com/rust-windowing/winit> |
| `winnow` | `1.0.5` | MIT | The crate authors | <https://github.com/winnow-rs/winnow> |
| `wit-bindgen` | `0.57.1` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Alex Crichton &lt;alex@alexcrichton.com&gt; | <https://github.com/bytecodealliance/wit-bindgen> |
| `x11-dl` | `2.21.0` | MIT | daggerbot &lt;daggerbot@gmail.com&gt;, Erle Pereira &lt;erle@erlepereira.com&gt;, AltF02 &lt;contact@altf2.dev&gt; | <https://github.com/AltF02/x11-rs.git> |
| `x11rb` | `0.13.2` | MIT OR Apache-2.0 | Uli Schlachter &lt;psychon@znc.in&gt;, Eduardo Sánchez Muñoz &lt;eduardosm-dev@e64.io&gt;, notgull &lt;jtnunley01@gmail.com&gt; | <https://github.com/psychon/x11rb> |
| `x11rb-protocol` | `0.13.2` | MIT OR Apache-2.0 | Uli Schlachter &lt;psychon@znc.in&gt;, Eduardo Sánchez Muñoz &lt;eduardosm-dev@e64.io&gt;, notgull &lt;jtnunley01@gmail.com&gt; | <https://github.com/psychon/x11rb> |
| `x25519-dalek` | `2.0.1` | BSD-3-Clause | Isis Lovecruft &lt;isis@patternsinthevoid.net&gt;, DebugSteven &lt;debugsteven@gmail.com&gt;, Henry de Valence &lt;hdevalence@hdevalence.ca&gt; | <https://github.com/dalek-cryptography/curve25519-dalek/tree/main/x25519-dalek> |
| `xkbcommon-dl` | `0.4.2` | MIT | Francesca Frangipane &lt;francesca@comfysoft.net&gt; | <https://github.com/rust-windowing/xkbcommon-dl> |
| `xkeysym` | `0.2.1` | MIT OR Apache-2.0 OR Zlib | John Nunley &lt;jtnunley01@gmail.com&gt; | <https://github.com/notgull/xkeysym> |
| `xml-rs` | `0.8.29` | MIT | Vladimir Matveev &lt;vmatveev@citrine.cc&gt; | <https://github.com/kornelski/xml-rs> |
| `zerocopy` | `0.8.63` | BSD-2-Clause OR Apache-2.0 OR MIT | The crate authors | <https://github.com/google/zerocopy> |
| `zerocopy-derive` | `0.8.63` | BSD-2-Clause OR Apache-2.0 OR MIT | The crate authors | <https://github.com/google/zerocopy> |
| `zeroize` | `1.9.1` | Apache-2.0 OR MIT | The RustCrypto Project Developers | <https://github.com/RustCrypto/utils> |
| `zeroize_derive` | `1.5.0` | Apache-2.0 OR MIT | The RustCrypto Project Developers | <https://github.com/RustCrypto/utils> |
| `zlib-rs` | `0.6.8` | Zlib | The crate authors | <https://github.com/trifectatechfoundation/zlib-rs> |
| `zmij` | `1.0.23` | MIT | David Tolnay &lt;dtolnay@gmail.com&gt; | <https://github.com/dtolnay/zmij> |
<!-- END CARGO METADATA CRATES -->

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
