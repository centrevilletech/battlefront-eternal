Basis Universal 2.50's transcoder is distributed under Apache-2.0 in
`basis-universal/LICENSE`. Transcoder and Zstandard source files are byte-for-byte
copies of `basisu_c_sys` 0.9.1's vendored source. The registry archive SHA-256 is
`cd84020bf440612079c6ddb372d4c293a91e7f1cbc055f2cbacc1fc943b88667`.

Two notice files were supplied separately: the Apache LICENSE was added, and
`basis-universal/zstd/LICENSE` contains Zstandard 1.5.7's BSD-3-Clause notice
with Meta's current copyright wording, rather than the registry's older wording.
Those notice files are not described as unchanged copies of the crate archive.
The encoder and JavaScript bindings are excluded. The bridge builds the
transcoder with all GPU output formats enabled and release assertions disabled.

Desktop binaries include these texts and the PNG/WebP, libc/libc++ and signal
shim notices from `rust/image-codec/licenses` under `legal/image-codecs`.
