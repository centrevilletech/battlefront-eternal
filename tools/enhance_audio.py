#!/usr/bin/env python3
"""Upscale a battle's sound for the enhanced mode: offline, local, repeatable.

    python3 -m venv .venv-audio
    .venv-audio/bin/pip install -r tools/requirements-audio.txt
    .venv-audio/bin/python tools/enhance_audio.py nab2cw [--jobs 8] [--only samples|streams]

Reads what tools/extract.py wrote under public/audio (the sub-level's samples
and the streams its mission opens) and writes public/audio/enhanced/ (also
gitignored): 48 kHz FLAC per sample, Ogg Opus per stream segment, music
upmixed to 5.1 and 7.1 beside its stereo, and manifest.json with every item's settings
and parity checks. The plain decodes stay as they are for the original mode.

Each sample or segment goes through:

1. Decode (16-bit PCM, or the streams' Xbox ADPCM, exactly as the game).
2. De-click (voice, music, ambience only: isolated spikes that return to the
   local trend; gunshots and blasts are left alone) and a gentle de-noise
   (spectral gating under a noise floor taken from the quietest frames, at
   most 6 dB, only where that floor is above -60 dBFS).
3. Resample to 48 kHz with soxr at very high quality (loops resampled as a
   loop, so they still join up).
4. Bandwidth extension for sources under 40 kHz (most samples are 22050 Hz):
   the top octave the source has is transposed up an octave in the STFT
   domain (magnitudes per frame, phases doubled, so transients and harmonics
   carry over) and laid in above the old band edge, its level following the
   spectral slope of the octave below, 3 dB under that line. Nothing is
   added below the old band edge.
5. A light EQ by kind (weapons a touch of presence, explosions a little more
   weight under 100 Hz, voice-over a high-pass and presence), then the level
   set back so the item's K-weighted energy equals the original's: the
   game's own gains do the mixing, so the balance between sounds stays as
   the game has it. An item whose peaks now pass full scale is stored lower
   and carries the gain to play it back at (playback_db).
6. Loops keep their length and join; a one-shot whose last 5 ms is still
   loud (cut off in the source) gets a 10 ms fade.
7. Written as 24-bit FLAC (samples) or Ogg Opus at transparent rates
   (streams: voice 96 kb/s, stereo 256, quad 320, 5.1 448, 7.1 576), with
   discrete channel mapping above two channels so the order holds.

A sample or a stream's segment is made once, in public/audio/enhanced/
samples/ or streams/<stream>/ (each with a .json of its settings and report),
and hard-linked (or copied) into every sub-level's folder that uses it: the
samples come from one bank and an era's voice-over is every one of its
battles', so the second battle of an era finds most of its sound done.

Parity checks compare each result with the original (both at 48 kHz): the
10 ms loudness envelopes must correlate at 0.98 or more (or, for a steady
sound, differ by 0.5 dB or less in the median frame), the average
spectrum below the old band edge must stay within 1.5 dB, and what was added
above it must stay under the budget step 4 sets. An item that fails is
marked "fallback" and the game plays its original.

Bandwidth extension uses a DSP method by default. tools/models/ (gitignored)
is where a locally run super-resolution model would go if one with a
suitable license is chosen; see docs/audio.md.
"""

import argparse
import json
import math
import os
import shutil
import struct
import sys
import time
import tempfile
from concurrent.futures import ProcessPoolExecutor
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
from zero_paths import PUBLIC as _PUBLIC  # noqa: E402
PUBLIC = _PUBLIC / "audio"
OUT = PUBLIC / "enhanced"
RATE = 48000
VERSION = 1

try:
    import numpy as np
    import soundfile as sf
    import soxr
    from scipy import signal
except ImportError as error:  # pragma: no cover - optional dependencies
    print(f"enhance_audio needs the optional packages in tools/requirements-audio.txt ({error})", file=sys.stderr)
    sys.exit(2)

sys.path.insert(0, str(Path(__file__).resolve().parent))
from zero_sound import decode_xbox_adpcm  # noqa: E402

# --- kinds -------------------------------------------------------------------

# The buses and groups of the sounds that use a sample say what it is.
BUS_KIND = {
    "0xaebbad4d": "ambience",  # ambience
    "0x3b13aa4b": "voice",  # voiceover
    "0x4d9942af": "music",  # ingamemusic
    "0x9f9c4fd4": "music",
}
GROUP_KIND = {
    "0x3f7baeb6": "weapons",  # weapons
    "0x399bf05d": "weapons",  # ordnance
    "0xdd9abb6e": "ambience",  # ambientenv
}
WORD_KIND = (
    ("_exp", "explosions"),
    ("explosion", "explosions"),
    ("chatter", "voice"),
    ("_vo", "voice"),
    ("step", "foley"),
    ("foley", "foley"),
    ("_mvt_", "foley"),
    ("equip", "foley"),
    ("reload", "foley"),
    ("fire", "weapons"),
    ("_ord", "weapons"),
    ("shell_", "interface"),
    ("zoom", "interface"),
    ("amb", "ambience"),
)

# Per kind: EQ (high-pass Hz, then (type, Hz, dB, Q) bells and shelves),
# whether to de-click, and how much bandwidth extension (dB re the slope).
KINDS = {
    "weapons": {"hp": 25, "eq": [("bell", 3200, 1.0, 0.8)], "declick": False, "extend": -3},
    "explosions": {"hp": 18, "eq": [("lowshelf", 100, 1.5, 0.7)], "declick": False, "extend": -3},
    "foley": {"hp": 40, "eq": [], "declick": False, "extend": -3},
    "voice": {"hp": 80, "eq": [("bell", 3000, 1.0, 0.9)], "declick": True, "extend": -6},
    "ambience": {"hp": 20, "eq": [], "declick": True, "extend": -4},
    "music": {"hp": 18, "eq": [], "declick": True, "extend": -6},
    "interface": {"hp": 30, "eq": [], "declick": False, "extend": -4},
    "effects": {"hp": 25, "eq": [], "declick": False, "extend": -3},
}


# What a class's sound key says a sound is.
KEY_KIND = {
    "FireSound": "weapons", "FireEmptySound": "weapons", "ReloadSound": "foley", "WeaponChangeSound": "foley",
    "OverheatSound": "weapons", "ChargeSound": "weapons", "FireLoopSound": "weapons", "OrdnanceSound": "weapons",
    "CollisionSound": "weapons", "edc6251a": "foley", "SoundProperty": "explosions", "JumpSound": "foley",
    "LandSound": "foley", "RollSound": "foley", "SquatSound": "foley", "ProneSound": "foley", "StandSound": "foley",
    "HurtSound": "voice", "DeathSound": "voice", "FleeSound": "voice", "Footstep0Sound": "foley",
    "Footstep1Sound": "foley", "Footstep2Sound": "foley", "PowerupSound": "effects", "AmbientSound": "ambience",
    "Ambient2Sound": "ambience",
}


def sample_kinds(manifest):
    """Each sample id's kind (what plays it: a weapon's fire, an explosion,
    foley, voice, ambience, the interface) and whether any sound loops it."""
    from zero_hash import fnv1a

    names = manifest.get("names", {})
    by_type = {}
    for o in manifest["objects"]:
        # A block can declare Name more than once (the GCW battles have one):
        # it answers to each of them.
        for name in o.get("Name") if isinstance(o.get("Name"), list) else [o.get("Name")]:
            by_type.setdefault(o["type"], {})[name] = o
    key = lambda name: "0x%08x" % fnv1a(name)

    def inherited(obj, field):
        seen = set()
        while obj is not None and id(obj) not in seen:
            seen.add(id(obj))
            if field in obj:
                return obj[field]
            obj = by_type.get("SoundProperties", {}).get(obj.get("Inherit"))
        return None

    # Sound name -> kind, from who plays it.
    sound_kind = {}

    def claim(sound, kind, depth=0):
        if not sound or depth > 4 or sound in sound_kind:
            return
        sound_kind[sound] = kind
        for kind_name in ("SoundLayered", "SoundParameterized"):
            obj = by_type.get(kind_name, {}).get(sound)
            if obj:
                for layer in obj.get("Layer", []) or []:
                    if len(layer) > 1:
                        claim(layer[1], kind, depth + 1)
                if obj.get("SoundLayered"):
                    claim(obj["SoundLayered"], kind, depth + 1)

    for props in manifest.get("classes", {}).values():
        for k, value in props.items():
            if k in KEY_KIND and isinstance(value, str) and value.split():
                claim(key(value.split()[0]), KEY_KIND[k])
    for entry in by_type.get("FoleyFXSoldier", {}).values():
        for k, v in entry.items():
            if k not in ("type", "Name", "Class") and isinstance(v, str):
                claim(v, "foley")
    for emitter in manifest.get("world", {}).get("emitters", []):
        claim(key(emitter["props"].get("Sound", "")), "ambience")
    for name in (manifest.get("mission", {}).get("effects") or {}).values():
        claim(key(name), "interface")

    kinds = {}
    loops = set()
    for name, obj in by_type.get("SoundProperties", {}).items():
        label = names.get(name, "")
        bus = inherited(obj, "Bus")
        group = inherited(obj, "Group")
        kind = sound_kind.get(name) or BUS_KIND.get(bus) or GROUP_KIND.get(group)
        if not kind:
            for word, k in WORD_KIND:
                if word in label.lower():
                    kind = k
                    break
        looping = (inherited(obj, "Looping") or 0) > 0
        for entry in inherited(obj, "SampleList") or []:
            sid = entry[0]
            if kind and sid not in kinds:
                kinds[sid] = kind
            if looping:
                loops.add(sid)
    return kinds, loops


# --- DSP ---------------------------------------------------------------------

def k_weight(rate):
    """BS.1770 K-weighting as two biquads (as libebur128 designs them;
    Copyright (c) 2011 Jan Kokemüller, MIT; see THIRD-PARTY-NOTICES.md)."""
    f0, G, Q = 1681.974450955533, 3.999843853973347, 0.7071752369554196
    K = math.tan(math.pi * f0 / rate)
    Vh = 10 ** (G / 20)
    Vb = Vh ** 0.4996667741545416
    a0 = 1 + K / Q + K * K
    pre_b = [(Vh + Vb * K / Q + K * K) / a0, 2 * (K * K - Vh) / a0, (Vh - Vb * K / Q + K * K) / a0]
    pre_a = [1, 2 * (K * K - 1) / a0, (1 - K / Q + K * K) / a0]
    f0, Q = 38.13547087602444, 0.5003270373238773
    K = math.tan(math.pi * f0 / rate)
    a0 = 1 + K / Q + K * K
    rlb_b = [1, -2, 1]
    rlb_a = [1, 2 * (K * K - 1) / a0, (1 - K / Q + K * K) / a0]
    return (pre_b, pre_a), (rlb_b, rlb_a)


def k_energy(x, rate):
    """Mean K-weighted power over the whole signal (channels summed)."""
    x = np.atleast_2d(x)
    (pb, pa), (rb, ra) = k_weight(rate)
    total = 0.0
    for ch in x:
        y = signal.lfilter(rb, ra, signal.lfilter(pb, pa, ch))
        total += float(np.mean(y * y))
    return total


def resample(x, src, dst, loop):
    """soxr VHQ; a loop is resampled as a loop (wrapped so it joins up)."""
    if src == dst:
        return x.copy()
    if not loop or len(x) < 64:
        return soxr.resample(x, src, dst, quality="VHQ")
    pad = min(len(x), int(0.05 * src))
    wrapped = np.concatenate([x[-pad:], x, x[:pad]])
    y = soxr.resample(wrapped, src, dst, quality="VHQ")
    start = int(round(pad * dst / src))
    n = int(round(len(x) * dst / src))
    return y[start:start + n]


def declick(x):
    """Replace isolated spikes (1-3 samples that leave and rejoin the local
    trend) by interpolation. Returns the signal and how many were fixed."""
    if len(x) < 32:
        return x, 0
    d2 = np.abs(np.diff(x, 2))
    med = signal.medfilt(np.abs(np.diff(x)), 63) + 1e-6
    spikes = np.where(d2 > 12 * med[:-1])[0] + 1
    fixed = 0
    y = x.copy()
    for i in spikes:
        if i < 4 or i > len(x) - 5:
            continue
        before = x[i - 3:i]
        after = x[i + 1:i + 4]
        trend = (before.mean() + after.mean()) / 2
        if abs(x[i] - trend) > 8 * (np.std(np.concatenate([before, after])) + 1e-6):
            y[i] = trend
            fixed += 1
    return y, fixed


def denoise(x, rate):
    """Gentle spectral gating: a floor from the quietest 10% of frames;
    bins near it come down by up to 6 dB. Skipped when the floor is low."""
    n = 1024
    if len(x) < n * 4:
        return x, 0.0
    f, t, Z = signal.stft(x, rate, nperseg=n, noverlap=n * 3 // 4)
    mag = np.abs(Z)
    frame_power = (mag ** 2).mean(axis=0)
    # Only where there is noise to find: the quiet frames well under the
    # sound (a steady loop's quiet frames are the sound itself).
    if 10 * np.log10((np.percentile(frame_power, 95) + 1e-20) / (np.percentile(frame_power, 10) + 1e-20)) < 30:
        return x, 0.0
    quiet = frame_power <= np.percentile(frame_power, 10)
    floor = mag[:, quiet].mean(axis=1, keepdims=True) + 1e-12
    floor_db = 10 * np.log10(float((floor ** 2).mean()) * 2 / n + 1e-20)
    if floor_db < -60:
        return x, 0.0
    snr = mag / floor
    gain = np.clip((snr - 1.0) / 3.0, 10 ** (-6 / 20), 1.0)
    gain = signal.lfilter([0.3], [1, -0.7], gain, axis=1)
    _, y = signal.istft(Z * gain, rate, nperseg=n, noverlap=n * 3 // 4)
    return y[: len(x)], floor_db


def extend_bandwidth(y, rate, edge, level_db):
    """Lay an octave above `edge` (Hz), transposed up from the octave below
    it, following that octave's spectral slope, level_db under the line."""
    n = 2048
    hop = 512
    if len(y) < n:
        return y, 0.0
    f, t, Z = signal.stft(y, rate, nperseg=n, noverlap=n - hop, boundary="even")
    bins = len(f)
    df = rate / n
    k_edge = int(edge / df)
    k_low = k_edge // 2
    k_top = min(2 * k_edge, int(19500 / df), bins - 1)
    if k_top <= k_edge + 4:
        return y, 0.0
    mag = np.abs(Z)
    phase = np.angle(Z)
    out = np.zeros_like(Z)
    # The source octave's slope per frame, in dB per octave, from two halves.
    lo = mag[k_low:(k_low + k_edge) // 2].mean(axis=0) + 1e-12
    hi = mag[(k_low + k_edge) // 2:k_edge].mean(axis=0) + 1e-12
    slope = np.clip(20 * np.log10(hi / lo) / 0.5, -24, -3)  # dB per octave
    for k in range(k_edge, k_top):
        src = k / 2
        k0 = int(src)
        frac = src - k0
        m = mag[k0] * (1 - frac) + mag[min(k0 + 1, bins - 1)] * frac
        ph = 2 * phase[k0]
        # The source bin sits an octave below: one octave up the slope says
        # `slope` dB quieter, and the conservative offset takes more off.
        gain = 10 ** ((slope + level_db) / 20)
        out[k] = m * gain * np.exp(1j * ph)
    # A short ramp in at the edge so the seam doesn't show.
    ramp = np.clip((np.arange(bins) - k_edge) / 6.0, 0, 1)[:, None]
    out *= ramp
    _, add = signal.istft(out, rate, nperseg=n, noverlap=n - hop, boundary=True)
    add = add[: len(y)]
    if len(add) < len(y):
        add = np.pad(add, (0, len(y) - len(add)))
    added = float(np.mean(add ** 2)) / (float(np.mean(y ** 2)) + 1e-20)
    return y + add, 10 * math.log10(added + 1e-20)


def biquad(kind, f0, gain_db, q, rate):
    A = 10 ** (gain_db / 40)
    w = 2 * math.pi * f0 / rate
    alpha = math.sin(w) / (2 * q)
    c = math.cos(w)
    if kind == "bell":
        b = [1 + alpha * A, -2 * c, 1 - alpha * A]
        a = [1 + alpha / A, -2 * c, 1 - alpha / A]
    elif kind == "lowshelf":
        sq = 2 * math.sqrt(A) * alpha
        b = [A * ((A + 1) - (A - 1) * c + sq), 2 * A * ((A - 1) - (A + 1) * c), A * ((A + 1) - (A - 1) * c - sq)]
        a = [(A + 1) + (A - 1) * c + sq, -2 * ((A - 1) + (A + 1) * c), (A + 1) + (A - 1) * c - sq]
    else:
        raise ValueError(kind)
    return np.array(b) / a[0], np.array(a) / a[0]


def equalize(y, kind, rate):
    spec = KINDS.get(kind, KINDS["effects"])
    sos = signal.butter(2, spec["hp"], "highpass", fs=rate, output="sos")
    y = signal.sosfilt(sos, y)
    for band in spec["eq"]:
        b, a = biquad(*band, rate)
        y = signal.lfilter(b, a, y)
    return y


def fix_tail(y, loop, rate):
    """A one-shot cut off in the source (its last 5 ms still loud) gets a
    10 ms fade instead of a click."""
    if loop or len(y) < rate // 50:
        return y, False
    tail = y[-int(0.005 * rate):]
    peak = np.max(np.abs(y)) + 1e-12
    if np.sqrt(np.mean(tail ** 2)) < 0.02 * peak:
        return y, False
    n = int(0.01 * rate)
    y = y.copy()
    y[-n:] *= 0.5 + 0.5 * np.cos(np.linspace(0, math.pi, n))
    return y, True


def envelope_db(x, rate):
    hop = int(0.01 * rate)
    n = len(x) // hop
    if n < 2:
        return np.array([20 * math.log10(np.sqrt(np.mean(x ** 2)) + 1e-9)])
    frames = x[: n * hop].reshape(n, hop)
    return 10 * np.log10(np.mean(frames ** 2, axis=1) + 1e-12)


def parity(ref, out, rate, edge, faded=False):
    """How far the result strays from the original (both at `rate`). A tail
    faded on purpose (fix_tail) isn't counted against it."""
    n = min(len(ref), len(out))
    if faded:
        n -= int(0.02 * rate)
    ref, out = ref[:n], out[:n]
    er = envelope_db(ref, rate)
    eo = envelope_db(out, rate)
    loud = er > er.max() - 30
    corr = 1.0
    if loud.sum() > 2 and np.std(er[loud]) > 1e-6 and np.std(eo[loud]) > 1e-6:
        corr = float(np.corrcoef(er[loud], eo[loud])[0, 1])
    # A steady sound's envelope barely moves, so its correlation says
    # little: what counts there is how far the levels moved.
    drift = float(np.median(np.abs(er[loud] - eo[loud]))) if loud.any() else 0.0
    m = min(4096, 1 << int(math.log2(max(n, 256))))
    fr, pr = signal.welch(ref, rate, nperseg=m)
    _, po = signal.welch(out, rate, nperseg=m)
    # The average spectrum's change below the old band edge, weighted by the
    # original's energy and leaving out bands 50 dB under its loudest (where
    # a change is inaudible and the ratio is noise).
    band = (fr > 100) & (fr < min(edge, 10000))
    spec = 0.0
    if band.any() and pr[band].max() > 0:
        p = pr[band]
        keep = p > p.max() * 1e-5
        diff = np.abs(10 * np.log10((po[band][keep] + 1e-20) / (p[keep] + 1e-20)))
        spec = float(np.sum(diff * p[keep]) / np.sum(p[keep]))
    return {"envelope": round(corr, 4), "drift_db": round(drift, 2), "spectrum_db": round(spec, 2)}


def enhance(x, rate, kind, loop, *, extension=True):
    """One mono or multichannel signal (channels first) through the chain."""
    x = np.atleast_2d(np.asarray(x, dtype=np.float64))
    spec = KINDS.get(kind, KINDS["effects"])
    notes = {"declicked": 0, "noise_floor_db": None, "extended_db": None, "faded": False}
    chans = []
    refs = []
    edge = 0.48 * rate
    for ch in x:
        ref48 = resample(ch, rate, RATE, loop)
        if spec["declick"]:
            ch, fixed = declick(ch)
            notes["declicked"] += fixed
        # A loop is processed as a loop: wrapped round on both sides so the
        # filters see its end before its start, then cut back, so it still
        # joins without a click.
        wrap = min(len(ch), int(0.25 * rate)) if loop else 0
        work = np.concatenate([ch[len(ch) - wrap:], ch, ch[:wrap]]) if wrap else ch
        work, floor = denoise(work, rate)
        if floor:
            notes["noise_floor_db"] = round(floor, 1)
        ch = work[wrap:wrap + len(ch)] if wrap else work
        y = resample(ch, rate, RATE, loop)
        n48 = len(y)
        wrap48 = min(n48, int(0.25 * RATE)) if loop else 0
        if wrap48:
            y = np.concatenate([y[n48 - wrap48:], y, y[:wrap48]])
        # Too short to see a spectrum (under ~90 ms): left as it is.
        if extension and rate < 40000 and len(y) >= 4096:
            y, added = extend_bandwidth(y, RATE, edge, spec["extend"])
            notes["extended_db"] = round(added, 1)
        y = equalize(y, kind, RATE)
        if wrap48:
            y = y[wrap48:wrap48 + n48]
        y, faded = fix_tail(y, loop, RATE)
        notes["faded"] = notes["faded"] or faded
        chans.append(y)
        refs.append(ref48)
    y = np.array(chans)
    ref = np.array(refs)
    # Back to the original's K-weighted energy: the game's gains mix it.
    e0 = k_energy(ref, RATE)
    e1 = k_energy(y, RATE)
    gain = math.sqrt(e0 / e1) if e1 > 0 and e0 > 0 else 1.0
    y *= gain
    checks = [parity(r, o, RATE, edge, notes["faded"]) for r, o in zip(ref, y)]
    # Sources at full scale come out of any filtering with peaks a little
    # over it. FLAC holds integers, so such an item is stored that much
    # quieter and the game plays it back up by playback_db: the level is
    # the original's either way.
    peak = float(np.max(np.abs(y))) if y.size else 0.0
    notes["playback_db"] = 0.0
    if peak > 0.999:
        y *= 0.999 / peak
        notes["playback_db"] = round(20 * math.log10(peak / 0.999), 3)
    worst = {
        "envelope": min(c["envelope"] for c in checks),
        "drift_db": max(c["drift_db"] for c in checks),
        "spectrum_db": max(c["spectrum_db"] for c in checks),
    }
    ok = (worst["envelope"] >= 0.98 or worst["drift_db"] <= 0.5) and worst["spectrum_db"] <= 1.5
    if notes["extended_db"] is not None and notes["extended_db"] > -6:
        ok = False
    return y, {"kind": kind, "loop": loop, "gain_db": round(20 * math.log10(gain), 2), **notes, "checks": worst, "fallback": not ok}


# --- music upmix -------------------------------------------------------------

def upmix(stereo, layout):
    """Stereo to 5.1 or 7.1 by primary/ambient extraction in the STFT domain
    (after Avendano and Jot 2004): per bin, the inter-channel coherence
    splits each channel into a direct part and an ambient part; the direct
    part's pan position sends its middle to the centre (6 dB down, to keep
    out of the dialogue's way) and the rest to the front pair; the ambient
    part, decorrelated by delays, goes to the surrounds. The LFE stays
    empty (the speakers' bass management takes the lows). Folded back down
    (L + 0.71 C + 0.71 S...), the fronts carry what they had."""
    L, R = stereo
    n = 2048
    hop = 512
    f, t, ZL = signal.stft(L, RATE, nperseg=n, noverlap=n - hop)
    _, _, ZR = signal.stft(R, RATE, nperseg=n, noverlap=n - hop)
    a = 0.8
    pLL = signal.lfilter([1 - a], [1, -a], np.abs(ZL) ** 2, axis=1)
    pRR = signal.lfilter([1 - a], [1, -a], np.abs(ZR) ** 2, axis=1)
    pLR = signal.lfilter([1 - a], [1, -a], ZL * np.conj(ZR), axis=1)
    coh = np.abs(pLR) / np.sqrt(pLL * pRR + 1e-20)
    amb = np.clip(1 - coh, 0, 1) ** 0.5
    direct = 1 - amb
    pan = (np.abs(ZL) - np.abs(ZR)) / (np.abs(ZL) + np.abs(ZR) + 1e-12)
    centre_share = direct * np.clip(1 - np.abs(pan), 0, 1) ** 2 * 0.5
    C = (ZL + ZR) * 0.5 * centre_share
    FL = ZL * (1 - amb) - C * 0.7071
    FR = ZR * (1 - amb) - C * 0.7071
    AL = ZL * amb
    AR = ZR * amb
    inv = lambda Z: signal.istft(Z, RATE, nperseg=n, noverlap=n - hop)[1][: len(L)]
    fl, fr, c, al, ar = (inv(Z) for Z in (FL, FR, C, AL, AR))
    delay = lambda x, ms: np.concatenate([np.zeros(int(ms * RATE / 1000)), x])[: len(x)]
    lfe = np.zeros_like(fl)
    if layout == "5.1":
        return np.array([fl, fr, c * 0.5, lfe, delay(al, 12) * 0.7071, delay(ar, 15) * 0.7071])
    # 7.1 (FL FR FC LFE BL BR SL SR): sides take the ambience, backs a later,
    # quieter copy.
    return np.array([fl, fr, c * 0.5, lfe, delay(al, 23) * 0.5, delay(ar, 27) * 0.5, delay(al, 10) * 0.7071, delay(ar, 13) * 0.7071])


# --- jobs --------------------------------------------------------------------

def write_flac(path, y):
    path.parent.mkdir(parents=True, exist_ok=True)
    sf.write(str(path), np.clip(np.asarray(y).T, -1, 1), RATE, subtype="PCM_24", format="FLAC")


# Opus bit rates for the streams, by channel count: transparent for this
# material (the sources are 4-bit ADPCM).
OPUS_RATES = {1: "96k", 2: "256k", 4: "320k", 6: "448k", 8: "576k"}


def write_opus(path, y):
    """A stream file as Ogg Opus via ffmpeg. Above two channels it uses
    channel mapping 255 (discrete): the channels stay in the file's order and
    none is treated as an LFE, which Chromium decodes as written."""
    import subprocess

    y = np.clip(np.asarray(y, dtype=np.float32), -1, 1)
    channels = y.shape[0]
    path.parent.mkdir(parents=True, exist_ok=True)
    cmd = [
        "ffmpeg", "-hide_banner", "-loglevel", "error", "-y",
        "-f", "f32le", "-ar", str(RATE), "-ac", str(channels), "-i", "pipe:0",
        "-c:a", "libopus", "-b:a", OPUS_RATES.get(channels, "512k"), "-vbr", "on", "-application", "audio",
        "-mapping_family", "255" if channels > 2 else "0", str(path),
    ]
    subprocess.run(cmd, input=y.T.tobytes(), check=True)


def sample_job(args):
    sid, pcm_bytes, rate, kind, loop, out = args
    x = np.frombuffer(pcm_bytes, dtype="<i2").astype(np.float64) / 32768
    y, report = enhance(x, rate, kind, loop)
    path = Path(out) / "samples" / f"{sid}.flac"
    write_flac(path, y)
    report = {"file": f"samples/{sid}.flac", "rate": RATE, "frames": int(y.shape[-1]), "source_rate": rate, **report}
    write_record(path.with_suffix(".json"), kind, loop, report)
    return sid, report


def segment_files(report, root):
    """A segment's files by layout, as the report names them (relative to a
    sub-level's folder), under root."""
    return {layout: Path(root) / file for layout, file in report["files"].items()}


def report_files(report, root):
    """Every file an item's report names, under root: a sample's one `file`,
    or a segment's `files` by layout."""
    if report.get("files"):
        return list(segment_files(report, root).values())
    return [Path(root) / report["file"]] if report.get("file") else []


def current_entry(path, kind, loop):
    """The record at path when it was made by this version with these
    settings; else None."""
    try:
        entry = json.loads(path.read_text())
    except (OSError, ValueError):
        return None
    if entry.get("version") != VERSION or entry.get("kind") != kind or entry.get("loop") != loop:
        return None
    return entry


def cached_report(path, kind, loop):
    """The report of an item made before with the same settings, when every
    file of it is still there; else None."""
    entry = current_entry(path, kind, loop)
    if entry is None:
        return None
    report = entry.get("report") or {}
    files = report_files(report, OUT)
    if not files or not all(file.exists() for file in files):
        return None
    return report


def cached_segment(stream, key, kind, loop):
    return cached_report(OUT / "streams" / stream / f"{key}.json", kind, loop)


def cached_sample(sid, kind, loop):
    return cached_report(OUT / "samples" / f"{sid}.json", kind, loop)


def write_record(path, kind, loop, report):
    """An item's record in the shared folder, written whole: another
    sub-level's run finds it complete or not at all."""
    # Battles may share a sample and finish its enhancement concurrently.
    # A unique sibling keeps one writer from renaming another's staging file.
    with tempfile.NamedTemporaryFile(mode="w", dir=path.parent,
                                     prefix=path.name + ".", suffix=".part",
                                     delete=False) as handle:
        part = Path(handle.name)
        try:
            json.dump({"version": VERSION, "kind": kind, "loop": loop, "report": report}, handle)
        except BaseException:
            part.unlink(missing_ok=True)
            raise
    try:
        replace_shared(part, path, kind, loop)
    finally:
        part.unlink(missing_ok=True)


def replace_shared(part, path, kind, loop):
    """part over path. On Windows the rename is refused while another battle
    has path open (reading it, or replacing it with the same record): retry
    briefly, then count it done when a current record is already there
    (every writer writes the item's files before its record)."""
    delay = 0.01
    for _ in range(8):
        try:
            part.replace(path)
            return
        except PermissionError:
            time.sleep(delay)
            delay *= 2
    try:
        part.replace(path)
    except PermissionError:
        if current_entry(path, kind, loop) is None:
            raise


def place(source, target):
    """target as a hard link to source (the same bytes once on disk), or a
    copy where links can't be made."""
    target.parent.mkdir(parents=True, exist_ok=True)
    try:
        if target.exists() and os.path.samefile(source, target):
            return
    except OSError:
        pass
    target.unlink(missing_ok=True)
    try:
        os.link(source, target)
    except OSError:
        shutil.copyfile(source, target)


def segment_job(args):
    stream, key, raw, channels, substreams, interleave, size, rate, kind, loop, out = args
    if substreams > 1:
        parts = [bytearray() for _ in range(substreams)]
        for k, pos in enumerate(range(0, len(raw), interleave)):
            parts[k % substreams] += raw[pos:pos + interleave]
        decoded = []
        for part in parts:
            decoded.extend(decode_xbox_adpcm(bytes(part[: size // substreams]), channels))
    else:
        decoded = decode_xbox_adpcm(raw[:size], channels)
    frames = min(len(c) for c in decoded)
    x = np.array([np.frombuffer(c, dtype=np.int16)[:frames] for c in decoded], dtype=np.float64) / 32768
    y, report = enhance(x, rate, kind, loop)
    base = Path(out) / "streams" / stream
    files = {"stereo" if y.shape[0] == 2 else f"{y.shape[0]}ch": f"streams/{stream}/{key}.opus"}
    write_opus(base / f"{key}.opus", y)
    if kind == "music" and y.shape[0] == 2:
        for layout in ("5.1", "7.1"):
            write_opus(base / f"{key}.{layout}.opus", upmix(y, layout))
            files[layout] = f"streams/{stream}/{key}.{layout}.opus"
    report = {"files": files, "rate": RATE, "frames": int(y.shape[-1]), "channels": int(y.shape[0]), "source_rate": rate, **report}
    write_record(base / f"{key}.json", kind, loop, report)
    return stream, key, report


def stream_kinds(manifest):
    """Each stream's kind and whether it loops, from the properties that
    play it."""
    names = manifest.get("names", {})
    kinds = {}
    for o in manifest["objects"]:
        if o["type"] not in ("SoundStreamProperties", "SoundStream") or not o.get("Stream"):
            continue
        stream = names.get(o["Stream"], o["Stream"])
        bus = o.get("Bus")
        kind = "music" if o["type"] == "SoundStream" or bus in ("0x4d9942af", "0x9f9c4fd4") else None
        if not kind:
            kind = "voice" if "vo" in stream else "ambience"
        kinds.setdefault(stream, (kind, (o.get("Looping") or 0) > 0 or kind == "ambience"))
    return kinds


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("sublevel", nargs="?", default="nab2cw")
    parser.add_argument("--jobs", type=int, default=os.cpu_count() or 4)
    parser.add_argument("--only", choices=["samples", "streams"])
    parser.add_argument("--limit", type=int, default=0, help="process only this many items of each kind (a quick look)")
    args = parser.parse_args()
    src = PUBLIC / args.sublevel / "sound.json"
    if not src.exists():
        print(f"no {src}: run tools/extract.py first", file=sys.stderr)
        return 1
    manifest = json.loads(src.read_text())
    pcm = (PUBLIC / args.sublevel / "bank.bin").read_bytes()
    out = OUT / args.sublevel
    out.mkdir(parents=True, exist_ok=True)
    result_path = out / "manifest.json"
    result = json.loads(result_path.read_text()) if result_path.exists() else {}
    result.update({"version": VERSION, "tool": "tools/enhance_audio.py", "rate": RATE, "method": "dsp"})
    result.setdefault("samples", {})
    result.setdefault("streams", {})
    started = time.time()
    if args.only != "streams":
        kinds, loops = sample_kinds(manifest)
        jobs = []
        reused = 0
        for sid, (offset, frames, rate) in manifest["samples"].items():
            kind, loop = kinds.get(sid, "effects"), sid in loops
            report = cached_sample(sid, kind, loop)
            if report:
                result["samples"][sid] = report
                reused += 1
                continue
            jobs.append((sid, pcm[offset:offset + 2 * frames], rate, kind, loop, str(OUT)))
        if args.limit:
            jobs = jobs[: args.limit]
        if jobs:
            with ProcessPoolExecutor(args.jobs) as pool:
                for sid, report in pool.map(sample_job, jobs, chunksize=4):
                    result["samples"][sid] = report
        for report in result["samples"].values():
            place(OUT / report["file"], out / report["file"])
        print(f"samples {len(jobs)} made, {reused} made before: {sum(1 for r in result['samples'].values() if r['fallback'])} fall back", flush=True)
    if args.only != "samples":
        kinds = stream_kinds(manifest)
        jobs = []
        reused = 0
        for name in manifest.get("streams", []):
            index = json.loads((PUBLIC / "streams" / f"{name}.json").read_text())
            data = (PUBLIC / "streams" / f"{name}.bin").read_bytes()
            kind, loop = kinds.get(name, ("voice" if "vo" in name else "ambience", False))
            items = list(index["segments"].items())
            if args.limit:
                items = items[: args.limit]
            for key, (offset, size, frames, rate) in items:
                report = cached_segment(name, key, kind, loop)
                if report:
                    result["streams"].setdefault(name, {})[key] = report
                    reused += 1
                    continue
                count = index.get("substreams", 1)
                chunk = index.get("substreamInterleave", 0) * count
                length = math.ceil(size / chunk) * chunk if count > 1 and chunk else size
                jobs.append((name, key, data[offset:offset + length], index["channels"], count, index.get("substreamInterleave", 0), size, rate, kind, loop, str(OUT)))
        # The long segments (music) first, so no one of them ends the run alone.
        jobs.sort(key=lambda job: -job[6])
        if jobs:
            with ProcessPoolExecutor(args.jobs) as pool:
                for stream, key, report in pool.map(segment_job, jobs):
                    result["streams"].setdefault(stream, {})[key] = report
        for stream, segments in result["streams"].items():
            for report in segments.values():
                for layout, source in segment_files(report, OUT).items():
                    place(source, out / report["files"][layout])
        print(f"stream segments {len(jobs)} made, {reused} made before", flush=True)
    result_path.write_text(json.dumps(result, indent=1, sort_keys=True))
    print(f"wrote {out} in {time.time() - started:.0f} s", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
