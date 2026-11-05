Synthetic codec fixtures and frozen host PCM. `inputs.json` records encoded
source hashes, native rates, lengths, channel counts and mapping families.
`host.json` records the reference revision and browser version. Each gzip
contains channel-major, little-endian float32 PCM at the stated rate.

FLAC covers the tools' S16 and PCM_24 formats, including libsndfile's encoder,
mono/stereo and every S16 value. Ogg Opus covers families 0, 1 and 255, with
1/2/4/6/8 unequal channels so an incorrect mapping cannot pass. The float WAVs
have sixteen discrete lanes and the shape of the optional headphone HRIRs;
one uses libsndfile, including its fact/PEAK metadata, as the generator does.
No original game data is included.

Source-rate FLAC/WAV must compare bit for bit. Explicit rate conversion and
Opus must keep every sample's error below -96 dBFS. `audio-codec.test.js`
also splits compressed input across metadata, frames and pages, and varies
the requested PCM chunk size. No host decoder runs in these tests.
