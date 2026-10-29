#!/usr/bin/env python3
"""Build the enhanced mode's headphone decoder: third-order ambisonics to
binaural, from a measured dummy head.

    .venv-audio/bin/pip install -r tools/requirements-audio.txt
    .venv-audio/bin/python tools/make_binaural.py [--sofa path/to/D1_48K_24bit_256tap_FIR_SOFA.sofa]

The enhanced mode's headphone renderer encodes every voice into a 3rd-order
ambisonic bed (16 channels, ACN order, orthonormal real spherical
harmonics) and turns the bed into two ears with one filter per channel. This
builds those filters from SADIE II's Neumann KU 100 (subject D1, 48 kHz,
8802 directions; University of York, Apache-2.0) by magnitude least squares
(Schörkhuber, Zaunschirm and Höldrich 2018), using spaudiopy (MIT).

The HRIR file is downloaded into tools/models/ (gitignored) when it isn't
given. Writes public/audio/hrtf/ (gitignored):

    sadie2_d1_3oa_left.wav, sadie2_d1_3oa_right.wav
                        16 channels each, 256 taps at 48 kHz: channel k is
                        that ear's filter for ambisonic channel k (both ears
                        are kept: the filters MagLS gives aren't mirror
                        images of each other channel by channel)
    sadie2_d1_3oa.json  provenance, the convention, the level, test values
                        of the harmonics for the browser to check its
                        encoder against, and each ear's level for a few
                        directions and frequencies, decoded and as measured
                        on the head, for the browser to check its decoding
                        against

Level: scaled so a source straight ahead comes out at -3 dB in each ear
(the level a centred voice has on stereo speakers), measured over 200 Hz to
4 kHz, so switching between speakers and headphones keeps the balance.
"""

import argparse
import json
import math
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MODELS = ROOT / "tools" / "models"
from zero_paths import PUBLIC  # noqa: E402
OUT = PUBLIC / "audio" / "hrtf"
SOFA_URL = "https://sofacoustics.org/data/database/sadie/D1_48K_24bit_256tap_FIR_SOFA.sofa"
ORDER = 3
TAPS = 256

try:
    import numpy as np
    import scipy.special
    import soundfile as sf

    # spaudiopy 0.2.0 calls scipy.special.sph_harm(m, n, azimuth, colatitude),
    # which SciPy 1.17 removed for sph_harm_y(n, m, colatitude, azimuth).
    if not hasattr(scipy.special, "sph_harm"):
        scipy.special.sph_harm = lambda m, n, azi, zen: scipy.special.sph_harm_y(n, m, zen, azi)
    from spaudiopy import decoder, io, sph
except ImportError as error:  # pragma: no cover - optional dependencies
    print(f"make_binaural needs the optional packages in tools/requirements-audio.txt ({error})", file=sys.stderr)
    sys.exit(2)


def sh_closed_form(x, y, z):
    """The 16 orthonormal real harmonics at a unit vector (x front, y left,
    z up), the same closed forms the browser's encoder uses."""
    return [
        0.28209479177387814,
        0.4886025119029199 * y,
        0.4886025119029199 * z,
        0.4886025119029199 * x,
        1.0925484305920792 * x * y,
        1.0925484305920792 * y * z,
        0.31539156525252005 * (3 * z * z - 1),
        1.0925484305920792 * x * z,
        0.5462742152960396 * (x * x - y * y),
        0.5900435899266435 * y * (3 * x * x - y * y),
        2.890611442640554 * x * y * z,
        0.4570457994644658 * y * (5 * z * z - 1),
        0.3731763325901154 * z * (5 * z * z - 3),
        0.4570457994644658 * x * (5 * z * z - 1),
        1.445305721320277 * z * (x * x - y * y),
        0.5900435899266435 * x * (x * x - 3 * y * y),
    ]


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--sofa", type=Path, help="the SADIE II D1 48 kHz SOFA file (downloaded if not given)")
    args = parser.parse_args()
    path = args.sofa
    if not path:
        MODELS.mkdir(parents=True, exist_ok=True)
        path = MODELS / Path(SOFA_URL).name
        if not path.exists():
            print(f"downloading {SOFA_URL}", flush=True)
            urllib.request.urlretrieve(SOFA_URL, path)
    hrirs = io.load_sofa_hrirs(str(path))
    fs = int(hrirs.fs)

    # Check the closed forms against spaudiopy's harmonics.
    rng = np.random.default_rng(1)
    v = rng.normal(size=(24, 3))
    v /= np.linalg.norm(v, axis=1, keepdims=True)
    azi = np.arctan2(v[:, 1], v[:, 0])
    zen = np.arccos(np.clip(v[:, 2], -1, 1))
    ref = sph.sh_matrix(ORDER, azi, zen, "real")
    mine = np.array([sh_closed_form(*p) for p in v])
    if not np.allclose(ref, mine, atol=1e-9):
        raise SystemExit(f"closed-form harmonics disagree with spaudiopy: {np.abs(ref - mine).max()}")

    # Magnitude least squares decoder: (2 ears, 16 channels, taps), already
    # tapered at both ends.
    filters = np.asarray(decoder.magls_bin(hrirs, ORDER))[:, :, :TAPS]

    # The decoder's own check: a source at the left ear and at the right
    # should come out mirrored.
    def ears(azimuth):
        y = np.array(sh_closed_form(math.cos(azimuth), math.sin(azimuth), 0))
        return [float(np.sqrt(((filters[e] * y[:, None]).sum(axis=0) ** 2).sum())) for e in (0, 1)]

    left90, right90 = ears(math.pi / 2), ears(-math.pi / 2)

    # The level: a source ahead, -3 dB per ear over 200 Hz..4 kHz.
    ahead = np.array(sh_closed_form(1, 0, 0))
    left = (filters[0] * ahead[:, None]).sum(axis=0)
    spectrum = np.abs(np.fft.rfft(left, 8192))
    freqs = np.fft.rfftfreq(8192, 1 / fs)
    band = (freqs > 200) & (freqs < 4000)
    level = float(np.sqrt(np.mean(spectrum[band] ** 2)))
    scale = math.sqrt(0.5) / level
    filters *= scale

    # Each ear's level (dB) for a few directions, decoded and as measured
    # (the nearest of the head's directions).
    def level(ir, f):
        n = np.arange(len(ir))
        return round(20 * math.log10(abs(np.sum(ir * np.exp(-2j * math.pi * f * n / fs)))), 2)

    grid = np.stack([np.sin(hrirs.zen) * np.cos(hrirs.azi), np.sin(hrirs.zen) * np.sin(hrirs.azi), np.cos(hrirs.zen)], 1)
    levels = []
    for azimuth, elevation in [(0, 0), (90, 0), (-90, 0), (180, 0), (45, 0), (0, 60)]:
        a, e = math.radians(azimuth), math.radians(elevation)
        d = [math.cos(e) * math.cos(a), math.cos(e) * math.sin(a), math.sin(e)]
        y = np.array(sh_closed_form(*d))
        i = int(np.argmax(grid @ np.array(d)))
        for f in (500, 1000, 2000, 4000):
            levels.append({
                "direction": d,
                "azimuth": azimuth,
                "elevation": elevation,
                "freq": f,
                "decoded": [level((filters[ear] * y[:, None]).sum(axis=0), f) for ear in (0, 1)],
                "measured": [level(scale * ir[i], f) for ir in (hrirs.left, hrirs.right)],
            })

    OUT.mkdir(parents=True, exist_ok=True)
    sf.write(OUT / "sadie2_d1_3oa_left.wav", filters[0].T.astype(np.float32), fs, subtype="FLOAT")
    sf.write(OUT / "sadie2_d1_3oa_right.wav", filters[1].T.astype(np.float32), fs, subtype="FLOAT")
    test = [{"direction": [float(a) for a in p], "harmonics": [float(h) for h in r]} for p, r in zip(v[:6], ref[:6])]
    (OUT / "sadie2_d1_3oa.json").write_text(json.dumps({
        "source": "SADIE II D1 (Neumann KU 100), University of York, Apache-2.0",
        "file": Path(path).name,
        "method": "MagLS (spaudiopy.decoder.magls_bin), 3rd order",
        "convention": "ACN, orthonormal real harmonics, x front, y left, z up; one 16-channel file per ear",
        "rate": fs,
        "taps": TAPS,
        "scale": scale,
        "ears_at_left": [round(x, 4) for x in left90],
        "ears_at_right": [round(x, 4) for x in right90],
        "test": test,
        "levels": levels,
    }, indent=1))
    print(f"wrote {OUT}/sadie2_d1_3oa_left.wav and _right.wav ({fs} Hz, {TAPS} taps); ears for a source at the left {left90}, at the right {right90}", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
