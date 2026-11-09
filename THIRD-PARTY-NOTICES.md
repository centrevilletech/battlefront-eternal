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
| [Saira](https://github.com/Omnibus-Type/Saira) typeface | `rust/bfe-client/assets/fonts/saira-latin.ttf`; also used to draw `build/native/assets/steam/` | Copyright 2020 The Saira Project Authors | SIL Open Font License 1.1 (full text in `licenses/OFL-Saira.txt`) |
| Chromium audio platform | `rust/audio/src/hrtf.rs` translates HRTF kernel preparation, spatial convolution and delay; `rust/audio-decode/src/resample.rs` translates streaming asset-rate conversion; `rust/audio/data/irc-composite.f32` is its decoded composite response resource | Copyright the Chromium Authors; Copyright (C) 2010 Google Inc. | BSD-3-Clause and the WebKit two-clause license (below) |
| [OpenAL Soft](https://github.com/kcat/openal-soft) 1.20.1 | `rust/audio/src/stabilizer.rs` translates its band splitter and front stabilizer; `rust/audio/src/stereo.rs` retains its stereo panning law; `rust/audio/src/layouts.rs` carries its speaker decoder coefficients | Copyright the OpenAL Soft authors | LGPL-2.0-or-later. The derived code is distributed under GPL-3.0-only, as section 3 of that license allows. |
| [libebur128](https://github.com/jiixyj/libebur128) | The K-weighting filter design in `rust/audio/src/meter.rs` and `tools/enhance_audio.py` | Copyright (c) 2011 Jan Kokemüller | MIT |
| AMD FidelityFX Super Resolution 1 | The RCAS sharpening pass in `rust/bfe-render/src/post.rs` follows its design | Copyright (c) Advanced Micro Devices, Inc. | MIT |
| fdlibm, as translated to C++ in [V8](https://v8.dev) 13.6 (`base/ieee754.cc`), and V8's `math::pow` (`numbers/ieee754.cc`) | `rust/v8math/src/trig.rs`, `explog.rs`, `inv.rs` and the first part of `pow.rs` translate them to Rust | Copyright (C) 1993 by Sun Microsystems, Inc.; Copyright 2011 and 2016 the V8 project authors | fdlibm: "Permission to use, copy, modify, and distribute this software is freely granted, provided that this notice is preserved." V8: BSD-3-Clause (below) |
| The double-precision `pow` of Arm's [optimized-routines](https://github.com/ARM-software/optimized-routines) (`math/pow.c` and its tables), which glibc ships as its `pow` | `rust/v8math/src/pow.rs` and `pow_data.rs` translate it to Rust | Copyright (c) 2018-2026, Arm Limited | MIT (offered as MIT OR Apache-2.0 WITH LLVM-exception; used here under MIT) |
| [three.js](https://threejs.org) r170 math collections | `rust/three` transliterates its math classes and `Object3D`'s transform semantics to Rust, operation by operation, for the simulation | Copyright 2010-2024 three.js authors | MIT |
| TimSort, as V8 runs it for `Array.prototype.sort` | `rust/jsrt/src/sort.rs` transliterates V8 13.6's `array-sort.tq`, itself a translation of CPython's listsort | Copyright (c) 2001-2018 Python Software Foundation (the algorithm text V8's file carries); V8's implementation copyright the V8 project authors | Python Software Foundation License 2.0; BSD-3-Clause (V8) |

## Libraries bundled in builds

| Component | Copyright | License |
| --- | --- | --- |
| [Claxon](https://github.com/ruuda/claxon) 0.4.3, pure Rust FLAC decoder | Copyright 2014-2019 Ruud van Asseldonk and contributors | Apache-2.0 (full text in `rust/audio-decode/licenses/Claxon.txt`, installed as `legal/audio-codecs/Claxon.txt`) |
| [opus-pure](https://github.com/stephenberry/opus-pure) 0.2.2, pure Rust Opus decoder | Copyright 2026 Stephen Berry; Copyright 2026 restsend.com; Copyright 2001-2023 Xiph.Org and contributors | BSD-3-Clause (full notices in `rust/audio-decode/licenses/Opus.txt`, installed as `legal/audio-codecs/Opus.txt`) |
| [Basis Universal](https://github.com/BinomialLLC/basis_universal) 2.50 transcoder, vendored through `rust/basis-transcode` | Copyright Binomial LLC | Apache-2.0; `licenses/Basis-Universal.txt` |
| Zstandard 1.5.7 decompressor | Copyright Meta Platforms, Inc. and affiliates | BSD-3-Clause; `licenses/Zstandard.txt` |
| nanoprintf 0.5.2 | Copyright 2019- Charles Nicholson | 0BSD (selected from 0BSD OR Unlicense); `licenses/Nanoprintf.txt` |
| png 0.18.1 | Copyright 2015 nwin and image-rs contributors | MIT (selected from MIT OR Apache-2.0); `licenses/Png.txt` |
| image-webp 0.2.4 | image-rs contributors | MIT (selected from MIT OR Apache-2.0); `licenses/Image-Webp.txt` |
| Decoder dependencies: adler2, bitflags, byteorder-lite, cfg-if, crc32fast, fdeflate, flate2, miniz_oxide, quick-error and simd-adler32 | Their respective contributors, as listed in the shipped texts | MIT, Apache-2.0, BSD-3-Clause and Zlib as applicable; complete texts under `licenses/` |

<!-- BEGIN CARGO METADATA CRATES -->
## Rust crates (`cargo metadata`)

The following third-party Rust crates are resolved in `rust/Cargo.lock`
across the shipped native targets (`x86_64-unknown-linux-gnu`,
`aarch64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`):

| Crate | Version | License | Authors | Repository |
| --- | --- | --- | --- | --- |
| `adler2` | `2.0.1` | 0BSD OR MIT OR Apache-2.0 | Jonas Schievink &lt;jonasschievink@gmail.com&gt;, oyvindln &lt;oyvindln@users.noreply.github.com&gt; | <https://github.com/oyvindln/adler2> |
| `aead` | `0.5.2` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/traits> |
| `aes` | `0.8.4` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/block-ciphers> |
| `aes-gcm` | `0.10.3` | Apache-2.0 OR MIT | RustCrypto Developers | <https://github.com/RustCrypto/AEADs> |
| `ahash` | `0.8.12` | MIT OR Apache-2.0 | Tom Kaitchuck &lt;Tom.Kaitchuck@gmail.com&gt; | <https://github.com/tkaitchuck/ahash> |
| `alsa` | `0.9.1` | Apache-2.0/MIT | David Henningsson &lt;coding@diwic.se&gt; | <https://github.com/diwic/alsa-rs> |
| `alsa-sys` | `0.3.1` | MIT | Pierre Krieger &lt;pierre.krieger1708@gmail.com&gt;, David Henningsson &lt;coding@diwic.se&gt; | <https://github.com/diwic/alsa-sys> |
| `arboard` | `3.7.0` | MIT OR Apache-2.0 | The crate authors | <https://github.com/1Password/arboard> |
| `as-raw-xcb-connection` | `1.0.1` | MIT OR Apache-2.0 | The crate authors | <https://github.com/psychon/as-raw-xcb-connection> |
| `base64` | `0.22.1` | MIT OR Apache-2.0 | Marshall Pierce &lt;marshall@mpierce.org&gt; | <https://github.com/marshallpierce/rust-base64> |
| `base64` | `0.23.1` | MIT OR Apache-2.0 | Marshall Pierce &lt;marshall@mpierce.org&gt; | <https://github.com/marshallpierce/rust-base64> |
| `base64ct` | `1.8.3` | Apache-2.0 OR MIT | RustCrypto Developers | <https://github.com/RustCrypto/formats> |
| `bitflags` | `2.13.2` | MIT OR Apache-2.0 | The Rust Project Developers | <https://github.com/bitflags/bitflags> |
| `block-buffer` | `0.10.4` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/utils> |
| `bytemuck` | `1.25.2` | Zlib OR Apache-2.0 OR MIT | Lokathor &lt;zefria@gmail.com&gt; | <https://github.com/Lokathor/bytemuck> |
| `byteorder-lite` | `0.1.0` | Unlicense OR MIT | The crate authors | <https://github.com/image-rs/byteorder-lite> |
| `bytes` | `1.12.1` | MIT | Carl Lerche &lt;me@carllerche.com&gt;, Sean McArthur &lt;sean@seanmonstar.com&gt; | <https://github.com/tokio-rs/bytes> |
| `calloop` | `0.13.0` | MIT | Elinor Berger &lt;elinor@safaradeg.net&gt; | <https://github.com/Smithay/calloop> |
| `calloop-wayland-source` | `0.3.0` | MIT | Kirill Chibisov &lt;contact@kchibisov.com&gt; | <https://github.com/smithay/calloop-wayland-source> |
| `cc` | `1.11.0` | MIT OR Apache-2.0 | The crate authors | <https://github.com/rust-lang/cc-rs> |
| `cfg-if` | `1.0.5` | MIT OR Apache-2.0 | Alex Crichton &lt;alex@alexcrichton.com&gt; | <https://github.com/rust-lang/cfg-if> |
| `cfg_aliases` | `0.2.2` | MIT | Zicklag &lt;zicklag@katharostech.com&gt; | <https://github.com/katharostech/cfg_aliases> |
| `cipher` | `0.4.4` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/traits> |
| `claxon` | `0.4.3` | Apache-2.0 | Ruud van Asseldonk &lt;dev@veniogames.com&gt; | <https://github.com/ruuda/claxon> |
| `clipboard-win` | `5.4.3` | BSL-1.0 | Douman &lt;douman@gmx.se&gt; | <https://github.com/DoumanAsh/clipboard-win> |
| `concurrent-queue` | `2.5.0` | Apache-2.0 OR MIT | Stjepan Glavina &lt;stjepang@gmail.com&gt;, Taiki Endo &lt;te316e89@gmail.com&gt;, John Nunley &lt;dev@notgull.net&gt; | <https://github.com/smol-rs/concurrent-queue> |
| `const-oid` | `0.9.6` | Apache-2.0 OR MIT | RustCrypto Developers | <https://github.com/RustCrypto/formats/tree/master/const-oid> |
| `cpal` | `0.15.3` | Apache-2.0 | The crate authors | <https://github.com/rustaudio/cpal> |
| `cpufeatures` | `0.2.17` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/utils> |
| `crc32fast` | `1.5.4` | MIT OR Apache-2.0 | Sam Rijs &lt;srijs@airpost.net&gt;, Alex Crichton &lt;alex@alexcrichton.com&gt; | <https://github.com/srijs/rust-crc32fast> |
| `crossbeam-utils` | `0.8.23` | MIT OR Apache-2.0 | The crate authors | <https://github.com/crossbeam-rs/crossbeam> |
| `crypto-common` | `0.1.7` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/traits> |
| `ctr` | `0.9.2` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/block-modes> |
| `cursor-icon` | `1.2.0` | MIT OR Apache-2.0 OR Zlib | Kirill Chibisov &lt;contact@kchibisov.com&gt; | <https://github.com/rust-windowing/cursor-icon> |
| `curve25519-dalek` | `4.1.3` | BSD-3-Clause | Isis Lovecruft &lt;isis@patternsinthevoid.net&gt;, Henry de Valence &lt;hdevalence@hdevalence.ca&gt; | <https://github.com/dalek-cryptography/curve25519-dalek/tree/main/curve25519-dalek> |
| `curve25519-dalek-derive` | `0.1.1` | MIT/Apache-2.0 | The crate authors | <https://github.com/dalek-cryptography/curve25519-dalek> |
| `dasp_sample` | `0.11.0` | MIT OR Apache-2.0 | mitchmindtree &lt;mitchell.nordine@gmail.com&gt; | <https://github.com/rustaudio/sample.git> |
| `data-encoding` | `2.11.1` | MIT | The crate authors | <https://github.com/ia0/data-encoding> |
| `der` | `0.7.10` | Apache-2.0 OR MIT | RustCrypto Developers | <https://github.com/RustCrypto/formats/tree/master/der> |
| `digest` | `0.10.7` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/traits> |
| `dlib` | `0.5.3` | MIT | Elinor Berger &lt;elinor@safaradeg.net&gt; | <https://github.com/elinorbgr/dlib> |
| `downcast-rs` | `1.2.1` | MIT/Apache-2.0 | Ashish Myles &lt;marcianx@gmail.com&gt;, Runji Wang &lt;wangrunji0408@163.com&gt; | <https://github.com/marcianx/downcast-rs> |
| `dpi` | `0.1.2` | Apache-2.0 AND MIT | The crate authors | <https://github.com/rust-windowing/winit> |
| `ed25519` | `2.2.3` | Apache-2.0 OR MIT | RustCrypto Developers | <https://github.com/RustCrypto/signatures/tree/master/ed25519> |
| `ed25519-dalek` | `2.2.0` | BSD-3-Clause | isis lovecruft &lt;isis@patternsinthevoid.net&gt;, Tony Arcieri &lt;bascule@gmail.com&gt;, Michael Rosenberg &lt;michael@mrosenberg.pub&gt; | <https://github.com/dalek-cryptography/curve25519-dalek/tree/main/ed25519-dalek> |
| `equivalent` | `1.0.3` | Apache-2.0 OR MIT | The crate authors | <https://github.com/indexmap-rs/equivalent> |
| `errno` | `0.3.14` | MIT OR Apache-2.0 | Chris Wong &lt;lambda.fairy@gmail.com&gt;, Dan Gohman &lt;dev@sunfishcode.online&gt; | <https://github.com/lambda-fairy/rust-errno> |
| `error-code` | `3.4.0` | BSL-1.0 | Douman &lt;douman@gmx.se&gt; | <https://github.com/DoumanAsh/error-code> |
| `fallible-iterator` | `0.3.0` | MIT/Apache-2.0 | Steven Fackler &lt;sfackler@gmail.com&gt; | <https://github.com/sfackler/rust-fallible-iterator> |
| `fallible-streaming-iterator` | `0.1.9` | MIT/Apache-2.0 | Steven Fackler &lt;sfackler@gmail.com&gt; | <https://github.com/sfackler/fallible-streaming-iterator> |
| `fdeflate` | `0.3.7` | MIT OR Apache-2.0 | The image-rs Developers | <https://github.com/image-rs/fdeflate> |
| `find-msvc-tools` | `0.1.15` | MIT OR Apache-2.0 | The crate authors | <https://github.com/rust-lang/cc-rs> |
| `fixedbitset` | `0.5.9` | MIT OR Apache-2.0 | bluss | <https://github.com/petgraph/fixedbitset> |
| `flate2` | `1.1.10` | MIT OR Apache-2.0 | Alex Crichton &lt;alex@alexcrichton.com&gt;, Josh Triplett &lt;josh@joshtriplett.org&gt; | <https://github.com/rust-lang/flate2-rs> |
| `foldhash` | `0.1.5` | Zlib | Orson Peters &lt;orsonpeters@gmail.com&gt; | <https://github.com/orlp/foldhash> |
| `generic-array` | `0.14.7` | MIT | Bartłomiej Kamiński &lt;fizyk20@gmail.com&gt;, Aaron Trent &lt;novacrazy@gmail.com&gt; | <https://github.com/fizyk20/generic-array.git> |
| `gethostname` | `1.1.0` | Apache-2.0 | Sebastian Wiesner &lt;sebastian@swsnr.de&gt; | <https://codeberg.org/swsnr/gethostname.rs.git> |
| `getrandom` | `0.2.17` | MIT OR Apache-2.0 | The Rand Project Developers | <https://github.com/rust-random/getrandom> |
| `getrandom` | `0.3.4` | MIT OR Apache-2.0 | The Rand Project Developers | <https://github.com/rust-random/getrandom> |
| `getrandom` | `0.4.3` | MIT OR Apache-2.0 | The Rand Project Developers | <https://github.com/rust-random/getrandom> |
| `ghash` | `0.5.1` | Apache-2.0 OR MIT | RustCrypto Developers | <https://github.com/RustCrypto/universal-hashes> |
| `gl_generator` | `0.14.0` | Apache-2.0 | Brendan Zabarauskas &lt;bjzaba@yahoo.com.au&gt;, Corey Richardson, Arseny Kapoulkine | <https://github.com/brendanzab/gl-rs/> |
| `glow` | `0.18.0` | MIT OR Apache-2.0 OR Zlib | Joshua Groves &lt;josh@joshgroves.com&gt;, Dzmitry Malyshau &lt;kvarkus@gmail.com&gt; | <https://github.com/grovesNL/glow> |
| `glutin` | `0.32.3` | Apache-2.0 | Kirill Chibisov &lt;contact@kchibisov.com&gt; | <https://github.com/rust-windowing/glutin> |
| `glutin-winit` | `0.5.0` | MIT | Kirill Chibisov &lt;contact@kchibisov.com&gt; | <https://github.com/rust-windowing/glutin> |
| `glutin_egl_sys` | `0.7.1` | Apache-2.0 | Kirill Chibisov &lt;contact@kchibisov.com&gt; | <https://github.com/rust-windowing/glutin> |
| `glutin_glx_sys` | `0.6.1` | Apache-2.0 | Kirill Chibisov &lt;contact@kchibisov.com&gt; | <https://github.com/rust-windowing/glutin> |
| `glutin_wgl_sys` | `0.6.1` | Apache-2.0 | Kirill Chibisov &lt;contact@kchibisov.com&gt; | <https://github.com/rust-windowing/glutin> |
| `hashbrown` | `0.14.5` | MIT OR Apache-2.0 | Amanieu d'Antras &lt;amanieu@gmail.com&gt; | <https://github.com/rust-lang/hashbrown> |
| `hashbrown` | `0.15.5` | MIT OR Apache-2.0 | Amanieu d'Antras &lt;amanieu@gmail.com&gt; | <https://github.com/rust-lang/hashbrown> |
| `hashbrown` | `0.17.1` | MIT OR Apache-2.0 | The crate authors | <https://github.com/rust-lang/hashbrown> |
| `hashlink` | `0.9.1` | MIT OR Apache-2.0 | kyren &lt;kerriganw@gmail.com&gt; | <https://github.com/kyren/hashlink> |
| `hkdf` | `0.12.4` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/KDFs/> |
| `hmac` | `0.12.1` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/MACs> |
| `http` | `1.5.0` | MIT OR Apache-2.0 | Alex Crichton &lt;alex@alexcrichton.com&gt;, Carl Lerche &lt;me@carllerche.com&gt;, Sean McArthur &lt;sean@seanmonstar.com&gt; | <https://github.com/hyperium/http> |
| `httparse` | `1.10.1` | MIT OR Apache-2.0 | Sean McArthur &lt;sean@seanmonstar.com&gt; | <https://github.com/seanmonstar/httparse> |
| `image-webp` | `0.2.4` | MIT OR Apache-2.0 | The crate authors | <https://github.com/image-rs/image-webp> |
| `indexmap` | `2.14.2` | Apache-2.0 OR MIT | The crate authors | <https://github.com/indexmap-rs/indexmap> |
| `inout` | `0.1.4` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/utils> |
| `itoa` | `1.0.18` | MIT OR Apache-2.0 | David Tolnay &lt;dtolnay@gmail.com&gt; | <https://github.com/dtolnay/itoa> |
| `jobserver` | `0.1.35` | MIT OR Apache-2.0 | Alex Crichton &lt;alex@alexcrichton.com&gt; | <https://github.com/rust-lang/jobserver-rs> |
| `khronos_api` | `3.1.0` | Apache-2.0 | Brendan Zabarauskas &lt;bjzaba@yahoo.com.au&gt;, Corey Richardson, Arseny Kapoulkine, Pierre Krieger &lt;pierre.krieger1708@gmail.com&gt; | <https://github.com/brendanzab/gl-rs/> |
| `libc` | `0.2.191` | MIT OR Apache-2.0 | The crate authors | <https://github.com/rust-lang/libc> |
| `libloading` | `0.8.9` | ISC | Simonas Kazlauskas &lt;libloading@kazlauskas.me&gt; | <https://github.com/nagisa/rust_libloading/> |
| `libsqlite3-sys` | `0.30.1` | MIT | The rusqlite developers | <https://github.com/rusqlite/rusqlite> |
| `linux-raw-sys` | `0.12.1` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Dan Gohman &lt;dev@sunfishcode.online&gt; | <https://github.com/sunfishcode/linux-raw-sys> |
| `linux-raw-sys` | `0.4.15` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Dan Gohman &lt;dev@sunfishcode.online&gt; | <https://github.com/sunfishcode/linux-raw-sys> |
| `lock_api` | `0.4.14` | MIT OR Apache-2.0 | Amanieu d'Antras &lt;amanieu@gmail.com&gt; | <https://github.com/Amanieu/parking_lot> |
| `log` | `0.4.35` | MIT OR Apache-2.0 | The Rust Project Developers | <https://github.com/rust-lang/log> |
| `memchr` | `2.8.3` | Unlicense OR MIT | Andrew Gallant &lt;jamslam@gmail.com&gt;, bluss | <https://github.com/BurntSushi/memchr> |
| `memmap2` | `0.9.12` | MIT OR Apache-2.0 | Dan Burkert &lt;dan@danburkert.com&gt;, Yevhenii Reizner &lt;razrfalcon@gmail.com&gt;, The Contributors | <https://github.com/RazrFalcon/memmap2-rs> |
| `miniz_oxide` | `0.8.9` | MIT OR Zlib OR Apache-2.0 | Frommi &lt;daniil.liferenko@gmail.com&gt;, oyvindln &lt;oyvindln@users.noreply.github.com&gt;, Rich Geldreich richgel99@gmail.com | <https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide> |
| `miniz_oxide` | `0.9.1` | MIT OR Zlib OR Apache-2.0 | Frommi &lt;daniil.liferenko@gmail.com&gt;, oyvindln &lt;oyvindln@users.noreply.github.com&gt;, Rich Geldreich richgel99@gmail.com | <https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide> |
| `nom` | `8.0.0` | MIT | contact@geoffroycouprie.com | <https://github.com/rust-bakery/nom> |
| `once_cell` | `1.21.4` | MIT OR Apache-2.0 | Aleksey Kladov &lt;aleksey.kladov@gmail.com&gt; | <https://github.com/matklad/once_cell> |
| `opaque-debug` | `0.3.1` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/utils> |
| `opus-pure` | `0.2.2` | BSD-3-Clause | The crate authors | <https://github.com/stephenberry/opus-pure> |
| `os_pipe` | `1.2.3` | MIT | Jack O'Connor | <https://github.com/oconnor663/os_pipe.rs> |
| `parking_lot` | `0.12.5` | MIT OR Apache-2.0 | Amanieu d'Antras &lt;amanieu@gmail.com&gt; | <https://github.com/Amanieu/parking_lot> |
| `parking_lot_core` | `0.9.12` | MIT OR Apache-2.0 | Amanieu d'Antras &lt;amanieu@gmail.com&gt; | <https://github.com/Amanieu/parking_lot> |
| `percent-encoding` | `2.3.2` | MIT OR Apache-2.0 | The rust-url developers | <https://github.com/servo/rust-url/> |
| `petgraph` | `0.8.3` | MIT OR Apache-2.0 | bluss, mitchmindtree | <https://github.com/petgraph/petgraph> |
| `pin-project-lite` | `0.2.17` | Apache-2.0 OR MIT | The crate authors | <https://github.com/taiki-e/pin-project-lite> |
| `pkcs8` | `0.10.2` | Apache-2.0 OR MIT | RustCrypto Developers | <https://github.com/RustCrypto/formats/tree/master/pkcs8> |
| `pkg-config` | `0.3.34` | MIT OR Apache-2.0 | Alex Crichton &lt;alex@alexcrichton.com&gt; | <https://github.com/rust-lang/pkg-config-rs> |
| `png` | `0.18.1` | MIT OR Apache-2.0 | The image-rs Developers | <https://github.com/image-rs/image-png> |
| `polling` | `3.11.0` | Apache-2.0 OR MIT | Stjepan Glavina &lt;stjepang@gmail.com&gt;, John Nunley &lt;dev@notgull.net&gt; | <https://github.com/smol-rs/polling> |
| `polyval` | `0.6.2` | Apache-2.0 OR MIT | RustCrypto Developers | <https://github.com/RustCrypto/universal-hashes> |
| `ppv-lite86` | `0.2.21` | MIT OR Apache-2.0 | The CryptoCorrosion Contributors | <https://github.com/cryptocorrosion/cryptocorrosion> |
| `proc-macro2` | `1.0.107` | MIT OR Apache-2.0 | David Tolnay &lt;dtolnay@gmail.com&gt;, Alex Crichton &lt;alex@alexcrichton.com&gt; | <https://github.com/dtolnay/proc-macro2> |
| `quick-error` | `2.0.1` | MIT/Apache-2.0 | Paul Colomiets &lt;paul@colomiets.name&gt;, Colin Kiegel &lt;kiegel@gmx.de&gt; | <http://github.com/tailhook/quick-error> |
| `quick-xml` | `0.41.0` | MIT | The crate authors | <https://github.com/tafia/quick-xml> |
| `quote` | `1.0.47` | MIT OR Apache-2.0 | David Tolnay &lt;dtolnay@gmail.com&gt; | <https://github.com/dtolnay/quote> |
| `rand` | `0.9.5` | MIT OR Apache-2.0 | The Rand Project Developers, The Rust Project Developers | <https://github.com/rust-random/rand> |
| `rand_chacha` | `0.9.0` | MIT OR Apache-2.0 | The Rand Project Developers, The Rust Project Developers, The CryptoCorrosion Contributors | <https://github.com/rust-random/rand> |
| `rand_core` | `0.6.4` | MIT OR Apache-2.0 | The Rand Project Developers, The Rust Project Developers | <https://github.com/rust-random/rand> |
| `rand_core` | `0.9.5` | MIT OR Apache-2.0 | The Rand Project Developers, The Rust Project Developers | <https://github.com/rust-random/rand> |
| `raw-window-handle` | `0.6.2` | MIT OR Apache-2.0 OR Zlib | Osspial &lt;osspial@gmail.com&gt; | <https://github.com/rust-windowing/raw-window-handle> |
| `ring` | `0.17.14` | Apache-2.0 AND ISC | The crate authors | <https://github.com/briansmith/ring> |
| `rusqlite` | `0.32.1` | MIT | The rusqlite developers | <https://github.com/rusqlite/rusqlite> |
| `rustc_version` | `0.4.1` | MIT OR Apache-2.0 | The crate authors | <https://github.com/djc/rustc-version-rs> |
| `rustix` | `0.38.44` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Dan Gohman &lt;dev@sunfishcode.online&gt;, Jakub Konka &lt;kubkon@jakubkonka.com&gt; | <https://github.com/bytecodealliance/rustix> |
| `rustix` | `1.1.6` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Dan Gohman &lt;dev@sunfishcode.online&gt;, Jakub Konka &lt;kubkon@jakubkonka.com&gt; | <https://github.com/bytecodealliance/rustix> |
| `rustls` | `0.23.46` | Apache-2.0 OR ISC OR MIT | The crate authors | <https://github.com/rustls/rustls> |
| `rustls-pki-types` | `1.15.1` | MIT OR Apache-2.0 | The crate authors | <https://github.com/rustls/pki-types> |
| `rustls-webpki` | `0.103.16` | ISC | The crate authors | <https://github.com/rustls/webpki> |
| `same-file` | `1.0.6` | Unlicense/MIT | Andrew Gallant &lt;jamslam@gmail.com&gt; | <https://github.com/BurntSushi/same-file> |
| `scoped-tls` | `1.0.1` | MIT/Apache-2.0 | Alex Crichton &lt;alex@alexcrichton.com&gt; | <https://github.com/alexcrichton/scoped-tls> |
| `scopeguard` | `1.2.0` | MIT OR Apache-2.0 | bluss | <https://github.com/bluss/scopeguard> |
| `semver` | `1.0.28` | MIT OR Apache-2.0 | David Tolnay &lt;dtolnay@gmail.com&gt; | <https://github.com/dtolnay/semver> |
| `serde` | `1.0.229` | MIT OR Apache-2.0 | Erick Tryzelaar &lt;erick.tryzelaar@gmail.com&gt;, David Tolnay &lt;dtolnay@gmail.com&gt; | <https://github.com/serde-rs/serde> |
| `serde_core` | `1.0.229` | MIT OR Apache-2.0 | Erick Tryzelaar &lt;erick.tryzelaar@gmail.com&gt;, David Tolnay &lt;dtolnay@gmail.com&gt; | <https://github.com/serde-rs/serde> |
| `serde_derive` | `1.0.229` | MIT OR Apache-2.0 | Erick Tryzelaar &lt;erick.tryzelaar@gmail.com&gt;, David Tolnay &lt;dtolnay@gmail.com&gt; | <https://github.com/serde-rs/serde> |
| `serde_json` | `1.0.154` | MIT OR Apache-2.0 | The crate authors | <https://github.com/serde-rs/json> |
| `sha1` | `0.10.7` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/hashes> |
| `sha2` | `0.10.9` | MIT OR Apache-2.0 | RustCrypto Developers | <https://github.com/RustCrypto/hashes> |
| `shlex` | `2.0.1` | MIT OR Apache-2.0 | comex &lt;comexk@gmail.com&gt;, Fenhl &lt;fenhl@fenhl.net&gt;, Adrian Taylor &lt;adetaylor@chromium.org&gt;, Alex Touchet &lt;alextouchet@outlook.com&gt;, Daniel Parks &lt;dp+git@oxidized.org&gt;, Garrett Berg &lt;googberg@gmail.com&gt; | <https://github.com/comex/rust-shlex> |
| `signature` | `2.2.0` | Apache-2.0 OR MIT | RustCrypto Developers | <https://github.com/RustCrypto/traits/tree/master/signature> |
| `simd-adler32` | `0.3.10` | MIT | Marvin Countryman &lt;me@maar.vin&gt; | <https://github.com/mcountryman/simd-adler32> |
| `slab` | `0.4.12` | MIT | Carl Lerche &lt;me@carllerche.com&gt; | <https://github.com/tokio-rs/slab> |
| `smallvec` | `1.16.5` | MIT OR Apache-2.0 | The Servo Project Developers | <https://github.com/servo/rust-smallvec> |
| `smithay-client-toolkit` | `0.19.2` | MIT | Elinor Berger &lt;elinor@safaradeg.net&gt;, i509VCB &lt;mail@i509.me&gt;, Ashley Wulber &lt;ashley@system76.com&gt; | <https://github.com/smithay/client-toolkit> |
| `smol_str` | `0.2.2` | MIT OR Apache-2.0 | Aleksey Kladov &lt;aleksey.kladov@gmail.com&gt; | <https://github.com/rust-analyzer/smol_str> |
| `socket2` | `0.6.5` | MIT OR Apache-2.0 | Alex Crichton &lt;alex@alexcrichton.com&gt;, Thomas de Zeeuw &lt;thomasdezeeuw@gmail.com&gt; | <https://github.com/rust-lang/socket2> |
| `spki` | `0.7.3` | Apache-2.0 OR MIT | RustCrypto Developers | <https://github.com/RustCrypto/formats/tree/master/spki> |
| `subtle` | `2.6.1` | BSD-3-Clause | Isis Lovecruft &lt;isis@patternsinthevoid.net&gt;, Henry de Valence &lt;hdevalence@hdevalence.ca&gt; | <https://github.com/dalek-cryptography/subtle> |
| `syn` | `2.0.119` | MIT OR Apache-2.0 | David Tolnay &lt;dtolnay@gmail.com&gt; | <https://github.com/dtolnay/syn> |
| `syn` | `3.0.9` | MIT OR Apache-2.0 | David Tolnay &lt;dtolnay@gmail.com&gt; | <https://github.com/dtolnay/syn> |
| `thiserror` | `1.0.69` | MIT OR Apache-2.0 | David Tolnay &lt;dtolnay@gmail.com&gt; | <https://github.com/dtolnay/thiserror> |
| `thiserror` | `2.0.21` | MIT OR Apache-2.0 | David Tolnay &lt;dtolnay@gmail.com&gt; | <https://github.com/dtolnay/thiserror> |
| `thiserror-impl` | `1.0.69` | MIT OR Apache-2.0 | David Tolnay &lt;dtolnay@gmail.com&gt; | <https://github.com/dtolnay/thiserror> |
| `thiserror-impl` | `2.0.21` | MIT OR Apache-2.0 | David Tolnay &lt;dtolnay@gmail.com&gt; | <https://github.com/dtolnay/thiserror> |
| `tracing` | `0.1.44` | MIT | Eliza Weisman &lt;eliza@buoyant.io&gt;, Tokio Contributors &lt;team@tokio.rs&gt; | <https://github.com/tokio-rs/tracing> |
| `tracing-core` | `0.1.36` | MIT | Tokio Contributors &lt;team@tokio.rs&gt; | <https://github.com/tokio-rs/tracing> |
| `tree_magic_mini` | `3.2.2` | MIT | Matt Brubeck &lt;mbrubeck@limpet.net&gt;, Allison Hancock &lt;aahancoc@umich.edu&gt; | <https://github.com/mbrubeck/tree_magic/> |
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
| `wayland-backend` | `0.3.17` | MIT | Elinor Berger &lt;elinor@safaradeg.net&gt; | <https://github.com/smithay/wayland-rs> |
| `wayland-client` | `0.31.15` | MIT | Elinor Berger &lt;elinor@safaradeg.net&gt; | <https://github.com/smithay/wayland-rs> |
| `wayland-csd-frame` | `0.3.0` | MIT | Kirill Chibisov &lt;contact@kchibisov.com&gt; | <https://github.com/rust-windowing/wayland-csd-frame> |
| `wayland-cursor` | `0.31.14` | MIT | Elinor Berger &lt;elinor@safaradeg.net&gt; | <https://github.com/smithay/wayland-rs> |
| `wayland-protocols` | `0.32.13` | MIT | Elinor Berger &lt;elinor@safaradeg.net&gt; | <https://github.com/smithay/wayland-rs> |
| `wayland-protocols-plasma` | `0.3.12` | MIT | Elinor Berger &lt;elinor@safaradeg.net&gt; | <https://github.com/smithay/wayland-rs> |
| `wayland-protocols-wlr` | `0.3.12` | MIT | Elinor Berger &lt;elinor@safaradeg.net&gt; | <https://github.com/smithay/wayland-rs> |
| `wayland-scanner` | `0.31.11` | MIT | Elinor Berger &lt;elinor@safaradeg.net&gt; | <https://github.com/smithay/wayland-rs> |
| `wayland-sys` | `0.31.11` | MIT | Elinor Berger &lt;elinor@safaradeg.net&gt; | <https://github.com/smithay/wayland-rs> |
| `webpki-roots` | `0.26.11` | CDLA-Permissive-2.0 | The crate authors | <https://github.com/rustls/webpki-roots> |
| `webpki-roots` | `1.0.11` | CDLA-Permissive-2.0 | The crate authors | <https://github.com/rustls/webpki-roots> |
| `winapi-util` | `0.1.11` | Unlicense OR MIT | Andrew Gallant &lt;jamslam@gmail.com&gt; | <https://github.com/BurntSushi/winapi-util> |
| `windows` | `0.54.0` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `windows-core` | `0.54.0` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `windows-link` | `0.2.1` | MIT OR Apache-2.0 | The crate authors | <https://github.com/microsoft/windows-rs> |
| `windows-result` | `0.1.2` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `windows-sys` | `0.52.0` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `windows-sys` | `0.61.2` | MIT OR Apache-2.0 | The crate authors | <https://github.com/microsoft/windows-rs> |
| `windows-targets` | `0.52.6` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `windows_x86_64_gnu` | `0.52.6` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `windows_x86_64_msvc` | `0.52.6` | MIT OR Apache-2.0 | Microsoft | <https://github.com/microsoft/windows-rs> |
| `winit` | `0.30.13` | Apache-2.0 | The winit contributors, Pierre Krieger &lt;pierre.krieger1708@gmail.com&gt; | <https://github.com/rust-windowing/winit> |
| `wl-clipboard-rs` | `0.9.5` | MIT OR Apache-2.0 | Ivan Molodetskikh &lt;yalterz@gmail.com&gt; | <https://github.com/YaLTeR/wl-clipboard-rs> |
| `x11-dl` | `2.21.0` | MIT | daggerbot &lt;daggerbot@gmail.com&gt;, Erle Pereira &lt;erle@erlepereira.com&gt;, AltF02 &lt;contact@altf2.dev&gt; | <https://github.com/AltF02/x11-rs.git> |
| `x11rb` | `0.13.2` | MIT OR Apache-2.0 | Uli Schlachter &lt;psychon@znc.in&gt;, Eduardo Sánchez Muñoz &lt;eduardosm-dev@e64.io&gt;, notgull &lt;jtnunley01@gmail.com&gt; | <https://github.com/psychon/x11rb> |
| `x11rb-protocol` | `0.13.2` | MIT OR Apache-2.0 | Uli Schlachter &lt;psychon@znc.in&gt;, Eduardo Sánchez Muñoz &lt;eduardosm-dev@e64.io&gt;, notgull &lt;jtnunley01@gmail.com&gt; | <https://github.com/psychon/x11rb> |
| `x25519-dalek` | `2.0.1` | BSD-3-Clause | Isis Lovecruft &lt;isis@patternsinthevoid.net&gt;, DebugSteven &lt;debugsteven@gmail.com&gt;, Henry de Valence &lt;hdevalence@hdevalence.ca&gt; | <https://github.com/dalek-cryptography/curve25519-dalek/tree/main/x25519-dalek> |
| `xcursor` | `0.3.11` | MIT | Samuele Esposito | <https://github.com/esposm03/xcursor-rs> |
| `xkbcommon-dl` | `0.4.2` | MIT | Francesca Frangipane &lt;francesca@comfysoft.net&gt; | <https://github.com/rust-windowing/xkbcommon-dl> |
| `xkeysym` | `0.2.1` | MIT OR Apache-2.0 OR Zlib | John Nunley &lt;jtnunley01@gmail.com&gt; | <https://github.com/notgull/xkeysym> |
| `xml-rs` | `0.8.29` | MIT | Vladimir Matveev &lt;vmatveev@citrine.cc&gt; | <https://github.com/kornelski/xml-rs> |
| `zerocopy` | `0.8.63` | BSD-2-Clause OR Apache-2.0 OR MIT | The crate authors | <https://github.com/google/zerocopy> |
| `zeroize` | `1.9.1` | Apache-2.0 OR MIT | The RustCrypto Project Developers | <https://github.com/RustCrypto/utils> |
| `zeroize_derive` | `1.5.0` | Apache-2.0 OR MIT | The RustCrypto Project Developers | <https://github.com/RustCrypto/utils> |
| `zlib-rs` | `0.6.8` | Zlib | The crate authors | <https://github.com/trifectatechfoundation/zlib-rs> |
| `zmij` | `1.0.23` | MIT | David Tolnay &lt;dtolnay@gmail.com&gt; | <https://github.com/dtolnay/zmij> |
<!-- END CARGO METADATA CRATES -->

<!-- BEGIN LIBVPX -->
## libvpx 1.17.0

The native movie decoder statically links libvpx (BSD-3-Clause).
Source: <https://github.com/webmproject/libvpx/tree/v1.17.0>.

```text
Copyright (c) 2010, The WebM Project authors. All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

  * Redistributions of source code must retain the above copyright
    notice, this list of conditions and the following disclaimer.

  * Redistributions in binary form must reproduce the above copyright
    notice, this list of conditions and the following disclaimer in
    the documentation and/or other materials provided with the
    distribution.

  * Neither the name of Google, nor the WebM Project, nor the names
    of its contributors may be used to endorse or promote products
    derived from this software without specific prior written
    permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

```text
Additional IP Rights Grant (Patents)
------------------------------------

"These implementations" means the copyrightable works that implement the WebM
codecs distributed by Google as part of the WebM Project.

Google hereby grants to you a perpetual, worldwide, non-exclusive, no-charge,
royalty-free, irrevocable (except as stated in this section) patent license to
make, have made, use, offer to sell, sell, import, transfer, and otherwise
run, modify and propagate the contents of these implementations of WebM, where
such license applies only to those patent claims, both currently owned by
Google and acquired in the future, licensable by Google that are necessarily
infringed by these implementations of WebM. This grant does not include claims
that would be infringed only as a consequence of further modification of these
implementations. If you or your agent or exclusive licensee institute or order
or agree to the institution of patent litigation or any other patent
enforcement activity against any entity (including a cross-claim or
counterclaim in a lawsuit) alleging that any of these implementations of WebM
or any code incorporated within any of these implementations of WebM
constitute direct or contributory patent infringement, or inducement of
patent infringement, then any patent rights granted to you under this License
for these implementations of WebM shall terminate as of the date such
litigation is filed.
```
<!-- END LIBVPX -->

## Tools bundled in desktop packages

The desktop packages include these programs, fetched by
`build/native/fetch-toolchain.py`, to prepare game data on the player's machine:

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
> this software without specific prior written permission.
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
