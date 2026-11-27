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
from contextlib import contextmanager
import filecmp
import hashlib
import json
import math
import os
import shutil
import struct
import subprocess
import sys
import time
import tempfile
import uuid
import zlib
from concurrent.futures import ProcessPoolExecutor
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
from zero_family import is_swbf2  # noqa: E402
from zero_paths import PUBLIC as _PUBLIC, write_text_lf  # noqa: E402
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


def battle_sample_choices():
    """Keep each battle's request when a sample needs more than one treatment."""
    requests = {}
    conflicts = set()
    for path in sorted(PUBLIC.glob("*/sound.json")):
        bank = path.parent.name
        if bank == "shell":
            continue
        try:
            manifest = json.loads(path.read_bytes())
            kinds, loops = sample_kinds(manifest)
            choices = {sid: [bank, offset, frames, rate, kinds.get(sid, "effects"), sid in loops]
                       for sid, (offset, frames, rate) in manifest["samples"].items()}
        except (OSError, ValueError, LookupError, TypeError, AttributeError) as error:
            print(f"Could not read sound treatments in {path}: {error}")
            continue
        for sid, request in choices.items():
            banks = requests.setdefault(sid, {})
            if banks and next(iter(banks.values()))[-2:] != request[-2:]:
                conflicts.add(sid)
            banks[bank] = request
    return {sid: requests[sid] for sid in sorted(conflicts)}


def shared_sample_choices():
    """Resolve conflicting treatments in the same order as a serial preparation."""
    if not is_swbf2():
        return battle_sample_choices()
    catalog = PUBLIC.parent / "common" / "shell" / "shell.json"
    maps = json.loads(catalog.read_bytes()).get("maps", []) if catalog.is_file() else []
    rank = {row["id"]: i for i, row in enumerate(maps)}
    index = PUBLIC / "index.json"
    missions = json.loads(index.read_bytes()).get("missions", {}) if index.is_file() else {}
    first = {}
    for mission, bank in sorted(missions.items()):
        first.setdefault(bank, mission)
    winners, conflicts, looped = {}, set(), set()
    for path in sorted(PUBLIC.glob("*/sound.json")):
        bank = path.parent.name
        if bank == "shell":
            continue
        try:
            manifest = json.loads(path.read_bytes())
            world = manifest.get("map", bank)
            kinds, loops = sample_kinds(manifest)
        except (OSError, ValueError, KeyError, TypeError, AttributeError) as error:
            print(f"Could not read sound treatments in {path}: {error}")
            continue
        order = (rank.get(world, len(rank)), world, first.get(bank, bank), bank)
        looped.update(loops)
        for sid, span in manifest.get("samples", {}).items():
            choice = [bank, *span, kinds.get(sid, "effects"), sid in loops]
            previous = winners.get(sid)
            if previous and previous[1][-2:] != choice[-2:]:
                conflicts.add(sid)
            if previous is None or order > previous[0]:
                winners[sid] = order, choice
    choices = {sid: winners[sid][1] for sid in sorted(conflicts)}
    if is_swbf2():
        # Loop-safe processing preserves the endpoints for any battle that repeats the sample.
        for sid in choices.keys() & looped:
            choices[sid][-1] = True
    return choices


def session_sample_choices(session):
    if session is None:
        return shared_sample_choices()
    path = session / "samples.json"
    with segment_lock(OUT, "", "sample-choices", path=session / "samples.lock"):
        if path.is_file():
            return json.loads(path.read_bytes())
        choices = shared_sample_choices()
        # A newly installed battle can change the winner for existing battles.
        repair_samples(choices)
        write_json(path, choices)
        return choices


def chosen_sample(sid, choice, *, variant=False):
    bank, offset, frames, rate, kind, loop = choice
    with (PUBLIC / bank / "bank.bin").open("rb") as handle:
        handle.seek(offset)
        pcm = handle.read(frames * 2)
    if len(pcm) != frames * 2:
        raise ValueError(f"Incomplete prepared sample {sid} in {bank}")
    digest = hashlib.sha256(struct.pack("<I", rate) + pcm).hexdigest()
    job = (sid, pcm, rate, kind, loop, str(OUT), digest)
    if variant:
        return (*job, f"{sid}-{kind}-{'loop' if loop else 'once'}-{digest}")
    return job


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
    if not loop or (len(x) < 64 and not is_swbf2()):
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
    n = 2048
    frames = stereo.shape[1]
    if is_swbf2() and frames < n:
        stereo = np.pad(stereo, ((0, 0), (0, n - frames)))
    L, R = stereo
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
    inv = lambda Z: signal.istft(Z, RATE, nperseg=n, noverlap=n - hop)[1][:frames]
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
    part = path.with_name(f".{uuid.uuid4().hex}.part")
    try:
        sf.write(str(part), np.clip(np.asarray(y).T, -1, 1), RATE, subtype="PCM_24", format="FLAC")
        with part.open("rb+") as handle:
            os.fsync(handle.fileno())
        part.replace(path)
    finally:
        part.unlink(missing_ok=True)


# Opus bit rates for the streams, by channel count: transparent for this
# material (the sources are 4-bit ADPCM).
OPUS_RATES = {1: "96k", 2: "256k", 4: "320k", 6: "448k", 8: "576k"}


def write_opus(path, y):
    """A stream file as Ogg Opus via ffmpeg. Above two channels it uses
    channel mapping 255 (discrete): the channels stay in the file's order and
    none is treated as an LFE, which Chromium decodes as written."""

    y = np.clip(np.asarray(y, dtype=np.float32), -1, 1)
    channels = y.shape[0]
    path.parent.mkdir(parents=True, exist_ok=True)
    cmd = [
        "ffmpeg", "-hide_banner", "-loglevel", "error", "-y",
        "-f", "f32le", "-ar", str(RATE), "-ac", str(channels), "-i", "pipe:0",
        "-c:a", "libopus", "-b:a", OPUS_RATES.get(channels, "512k"), "-vbr", "on", "-application", "audio",
        "-mapping_family", "255" if channels > 2 else "0",
    ]
    part = path.with_name(f".{uuid.uuid4().hex}.opus")
    try:
        subprocess.run([*cmd, str(part)], input=y.T.tobytes(), check=True)
        with part.open("rb+") as output:
            os.fsync(output.fileno())
        part.replace(path)
    finally:
        part.unlink(missing_ok=True)


def sample_job(args):
    sid, pcm_bytes, rate, kind, loop, out, *proof = args
    root = Path(out)
    source = proof[0] if proof else None
    variant = len(proof) == 2
    stem = proof[1] if variant else sid
    path = root / "samples" / f"{stem}.flac"
    with segment_lock(root, "samples", stem):
        cached = cached_report(path.with_suffix(".json"), kind, loop, root, source)
        if cached is not None:
            return sid, cached
        x = np.frombuffer(pcm_bytes, dtype="<i2").astype(np.float64) / 32768
        y, report = enhance(x, rate, kind, loop)
        file = None if variant and report["fallback"] else f"samples/{stem}.flac"
        if file:
            write_flac(path, y)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
        report = {"file": file, "rate": RATE, "frames": int(y.shape[-1]), "source_rate": rate, **report}
        if is_swbf2():
            record_span(report, x.size)
        proof = {"sourceSha256": source, "flacBytes": path.stat().st_size if file else 0} if source else {}
        if is_swbf2():
            proof["flacSha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
        write_record(path.with_suffix(".json"), kind, loop, report, **proof)
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


def cached_report(path, kind, loop, root=None, source=None):
    """The report of an item made before with the same settings, when every
    file of it is still there; else None."""
    entry = current_entry(path, kind, loop)
    if entry is None:
        return None
    report = entry.get("report") or {}
    if is_swbf2():
        if "source_frames" not in report:
            return None
        if report.get("reason") == "empty" and report.get("source_frames") == 0:
            return report
    if source and report.get("fallback") is True and report.get("file") is None:
        return report if entry.get("sourceSha256") == source else None
    files = report_files(report, OUT if root is None else root)
    if not files or not all(file.exists() for file in files):
        return None
    if source and (entry.get("sourceSha256") != source or entry.get("flacBytes") != files[0].stat().st_size):
        return None
    if is_swbf2() and report.get("file") and entry.get("flacSha256") != hashlib.sha256(files[0].read_bytes()).hexdigest():
        return None
    return report


def cached_segment(stream, key, kind, loop):
    return cached_report(OUT / "streams" / stream / f"{key}.json", kind, loop)


def cached_sample(sid, kind, loop):
    return cached_report(OUT / "samples" / f"{sid}.json", kind, loop)


def write_record(path, kind, loop, report, **proof):
    """An item's record in the shared folder, written whole: another
    sub-level's run finds it complete or not at all."""
    # Battles may share a sample and finish its enhancement concurrently.
    # A unique sibling keeps one writer from renaming another's staging file.
    with tempfile.NamedTemporaryFile(mode="w", dir=path.parent,
                                     prefix=path.name + ".", suffix=".part",
                                     newline="\n", delete=False) as handle:
        part = Path(handle.name)
        try:
            json.dump({"version": VERSION, "kind": kind, "loop": loop, "report": report, **proof}, handle)
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


def write_json(path, data):
    part = path.with_name(f".{uuid.uuid4().hex}.part")
    try:
        write_text_lf(part, json.dumps(data, indent=1, sort_keys=True))
        part.replace(path)
    finally:
        part.unlink(missing_ok=True)


@contextmanager
def segment_lock(out, stream, key, *, path=None):
    """Reserve one byte per segment in the existing preparation lock file."""
    if path is None:
        path = out.parent.parent / ".locks" / "audio-index.lock"
    path.parent.mkdir(parents=True, exist_ok=True)
    offset = 1 + (zlib.crc32((stream + "/" + key).encode()) & 0x7fffffff)
    with path.open("a+b") as handle:
        if os.name == "nt":
            import msvcrt
            handle.seek(offset)
            while True:
                try:
                    msvcrt.locking(handle.fileno(), msvcrt.LK_NBLCK, 1)
                    break
                except PermissionError:
                    time.sleep(0.05)
        else:
            import fcntl
            fcntl.lockf(handle, fcntl.LOCK_EX, 1, offset)
        yield  # Closing the descriptor also releases the lock after an interrupted job.


def shared_stream_settings(session, stream, settings):
    """The first reference chooses the treatment for a shared stream during this preparation."""
    if session is None:
        return settings
    choices = session / "streams.json"
    with segment_lock(OUT, "", ""):
        saved = json.loads(choices.read_bytes()) if choices.exists() else {}
        if stream in saved:
            return tuple(saved[stream])
        saved[stream] = settings
        pending = choices.with_suffix(".tmp")
        write_text_lf(pending, json.dumps(saved))
        pending.replace(choices)
        return settings


def segment_job(args):
    stream, key = args[:2]
    kind, loop, out = args[-3:]
    root = Path(out)
    with segment_lock(root, stream, key):
        report = cached_report(root / "streams" / stream / f"{key}.json", kind, loop, root)
        if report is not None:
            return stream, key, report
        return encode_segment(args)


def record_span(report, source_frames):
    report["source_frames"] = source_frames
    if report["loop"]:
        report["source_loop"] = [0, source_frames]
        report["loop_frames"] = [0, report["frames"]]


def encode_segment(args, layouts=None):
    stream, key, raw, channels, substreams, interleave, size, rate, *span, kind, loop, out = args
    base = Path(out) / "streams" / stream
    if span == [0]:
        base.mkdir(parents=True, exist_ok=True)
        report = {"files": {}, "rate": RATE, "frames": 0, "channels": channels * substreams,
                  "source_rate": rate, "source_frames": 0, "kind": kind, "loop": loop,
                  "fallback": True, "reason": "empty"}
        write_record(base / f"{key}.json", kind, loop, report)
        return stream, key, report
    if substreams > 1:
        parts = [bytearray() for _ in range(substreams)]
        for k, pos in enumerate(range(0, len(raw), interleave)):
            parts[k % substreams] += raw[pos:pos + interleave]
        decoded = []
        for part in parts:
            # Interleaved 2005 banks count playable frames across the padded planes.
            length = len(part) if span else size // substreams
            decoded.extend(decode_xbox_adpcm(bytes(part[:length]), channels))
    else:
        decoded = decode_xbox_adpcm(raw[:size], channels)
    frames = min(len(c) for c in decoded)
    if span:
        if frames < span[0]:
            raise ValueError(f"{stream}/{key}: decoded {frames} source frames, expected {span[0]}")
        frames = span[0]
    x = np.array([np.frombuffer(c, dtype=np.int16)[:frames] for c in decoded], dtype=np.float64) / 32768
    y, report = enhance(x, rate, kind, loop)
    native = "stereo" if y.shape[0] == 2 else f"{y.shape[0]}ch"
    files = {native: f"streams/{stream}/{key}.opus"}
    if layouts is None or native in layouts:
        write_opus(base / f"{key}.opus", y)
    if kind == "music" and y.shape[0] == 2:
        for layout in ("5.1", "7.1"):
            if layouts is None or layout in layouts:
                write_opus(base / f"{key}.{layout}.opus", upmix(y, layout))
            files[layout] = f"streams/{stream}/{key}.{layout}.opus"
    report = {"files": files, "rate": RATE, "frames": int(y.shape[-1]), "channels": int(y.shape[0]), "source_rate": rate, **report}
    if span:
        record_span(report, frames)
    write_record(base / f"{key}.json", kind, loop, report)
    return stream, key, report


def segment_args(stream, key, index, data, kind, loop, out):
    offset, size, frames, rate = index["segments"][key]
    count = index.get("substreams", 1)
    interleave = index.get("substreamInterleave", 0)
    block = interleave * count
    length = math.ceil(size / block) * block if count > 1 and block else size
    span = ()
    if is_swbf2():
        if (index.get("format") != 5 or count < 1 or rate <= 0 or frames < 0 or frames % count
                or bool(frames) != bool(size) or (count > 1 and interleave <= 0)
                or offset < 0 or offset + length > len(data)):
            raise ValueError(f"Invalid 2005 ADPCM segment {stream}/{key}")
        span = (frames // count,)
    return stream, key, data[offset:offset + length], index["channels"], count, interleave, size, rate, *span, kind, loop, str(out)


_BIT_REVERSE = bytes(int(f"{n:08b}"[::-1], 2) for n in range(256))


def complete_opus(path, frames, *, accept_decodable=False):
    """Check every Ogg page, one logical stream, and the Opus play length."""
    try:
        data = path.read_bytes()
    except FileNotFoundError:
        return False
    pos = seq = 0
    serial = skip = None
    while pos + 27 <= len(data):
        tag, version, flags, granule, stream, number, crc, count = struct.unpack_from("<4sBBqIIIB", data, pos)
        packets = pos + 27 + count
        end = packets + sum(data[pos + 27:packets])
        if tag != b"OggS" or version or number != seq or end > len(data):
            return False
        page = data[pos:pos+22] + bytes(4) + data[pos+26:end]
        reflected = zlib.crc32(page.translate(_BIT_REVERSE), 0xffffffff) ^ 0xffffffff
        if int.from_bytes(reflected.to_bytes(4, "little").translate(_BIT_REVERSE), "big") != crc:
            return False
        if seq == 0:
            if not flags & 2 or data[packets:packets+8] != b"OpusHead" or packets + 12 > end:
                return False
            serial = stream
            skip = int.from_bytes(data[packets+10:packets+12], "little")
        elif stream != serial or flags & 2:
            return False
        if flags & 4:
            if end != len(data) or seq == 0 or granule <= skip:
                return False
            if granule - skip == frames:
                return True
            # Older manifests can disagree with the end granule of a complete encode.
            return accept_decodable and subprocess.run(
                ["ffmpeg", "-nostdin", "-v", "error", "-xerror", "-i", str(path), "-f", "null", "-"],
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=120).returncode == 0
        pos = end
        seq += 1
    return False


def place_stream(source, target):
    target.parent.mkdir(parents=True, exist_ok=True)
    if target.exists() and source.samefile(target):
        return
    part = target.with_name(f".{uuid.uuid4().hex}.part")
    try:
        try:
            os.link(source, part)
        except OSError:
            shutil.copyfile(source, part)
            with part.open("rb+") as output:
                os.fsync(output.fileno())
        part.replace(target)
    finally:
        part.unlink(missing_ok=True)


def prepared_streams():
    """Yield battle references and any shared encodes published before their battle manifest."""
    referenced = set()
    for manifest in sorted(OUT.glob("*/manifest.json")):
        try:
            for stream, segments in json.loads(manifest.read_bytes())["streams"].items():
                for key, report in segments.items():
                    yield manifest.parent, stream, key, report
                    if isinstance(report, dict) and isinstance(report.get("files"), dict):
                        referenced.add((stream, key))
        except (OSError, ValueError, KeyError, TypeError, AttributeError) as error:
            print(f"Could not read Enhanced Sound manifest {manifest}: {error}", flush=True)
    for record in sorted((OUT / "streams").glob("*/*.json")):
        stream, key = record.parent.name, record.stem
        if (stream, key) not in referenced:
            try:
                entry = json.loads(record.read_bytes())
                if entry["version"] == VERSION:
                    yield None, stream, key, entry["report"]
            except (OSError, ValueError, KeyError, TypeError) as error:
                print(f"Could not read Enhanced Sound receipt {record}: {error}", flush=True)


def restore_battle_manifests():
    """Rebuild interrupted optional manifests from the battle's prepared bank."""
    ok = True
    paths = set(OUT.glob("*/manifest.json"))
    receipt = OUT.parent.parent / (".prepare-swbf2.json" if is_swbf2() else ".prepare.json")
    try:
        for step in json.loads(receipt.read_bytes())["done"]:
            if isinstance(step, str) and step.startswith("audio:"):
                bank = step[6:]
                if bank and bank not in (".", "..") and not any(c in bank for c in "/\\:"):
                    paths.add(OUT / bank / "manifest.json")
    except (OSError, ValueError, TypeError, KeyError):
        pass
    for path in sorted(paths):
        try:
            battle = json.loads(path.read_bytes())
            if isinstance(battle.get("streams"), dict) and isinstance(battle.get("samples"), dict):
                continue
        except (OSError, ValueError, AttributeError):
            pass
        name = path.parent.name
        if not (PUBLIC / name / "sound.json").is_file():
            print(f"Enhanced Sound for {name} is missing its source manifest", flush=True)
            ok = False
            continue
        result = subprocess.run([sys.executable, __file__, name, "--jobs", "2"])
        ok = result.returncode == 0 and ok
    return ok


def battle_sample(sid, report, battle, variant):
    """The released sound reader follows the battle-relative file and its parity verdict."""
    if not report.get("file"):
        return report
    master = OUT / report["file"]
    name = f"samples/{sid}.flac" if variant else report["file"]
    alias = battle / name
    if not alias.exists() or (not master.samefile(alias) and not filecmp.cmp(master, alias, shallow=False)):
        place_stream(master, alias)
    return {**report, "file": name} if variant else report


def repair_samples(choices=None, changed=None):
    manifests = sorted(OUT.glob("*/manifest.json"))
    if not manifests:
        return True
    if choices is None:
        choices = shared_sample_choices()
    verified = {}
    ok = True
    for path in manifests:
        try:
            manifest = json.loads(path.read_bytes())
            samples = manifest.get("samples", {})
            manifest_changed = False
            banks = choices
            if is_swbf2() and samples:
                bank = path.parent.name
                source = json.loads((PUBLIC / bank / "sound.json").read_bytes())
                kinds, loops = sample_kinds(source)
                banks = {sid: choices.get(sid, [bank, *span, kinds.get(sid, "effects"), sid in loops])
                         for sid, span in source["samples"].items()}
            for sid in banks.keys() & samples.keys():
                bank = path.parent.name
                variant = not is_swbf2()
                request = banks[sid][bank] if variant else banks[sid]
                filename = f"samples/{sid}.flac"
                if changed is not None and not any(
                    name == f"enhanced/{bank}/{filename}" or name in (f"enhanced/{filename}", f"enhanced/samples/{sid}.json")
                    or name.startswith(f"enhanced/samples/{sid}-") for name in changed
                ):
                    continue
                job = chosen_sample(sid, request, variant=True) if variant else chosen_sample(sid, request)
                cache_key = job[-1] if variant else sid
                if cache_key not in verified:
                    _, verified[cache_key] = sample_job(job)
                report = battle_sample(sid, verified[cache_key], path.parent, variant)
                if samples[sid] != report:
                    samples[sid] = report
                    manifest_changed = True
            if manifest_changed:
                write_json(path, manifest)
            if not is_swbf2():
                for sid in choices.keys() & samples.keys():
                    if samples[sid].get("file") is None:
                        (path.parent / "samples" / f"{sid}.flac").unlink(missing_ok=True)
        except (OSError, ValueError, KeyError, TypeError, AttributeError, IndexError, subprocess.SubprocessError) as error:
            ok = False
            print(f"Could not update Enhanced Sound samples for {path.parent.name}: {error}", flush=True)
    if ok and not is_swbf2():
        # Battle aliases have been published by name. Retire the old single-treatment masters.
        for sid in choices:
            for suffix in (".flac", ".json"):
                (OUT / "samples" / (sid + suffix)).unlink(missing_ok=True)
    return ok



def repair_streams(progress=None, *, record=None):
    """Verify changed files. A failed repair is retried only after its inputs change."""
    import zero_sound_health as health
    root = OUT.parent.parent
    if record is None:
        record = health.load(root)
    previous = record.get("streams", {})
    files_checked = {}
    inodes = {}
    encoded = linked = 0
    errors = (OSError, ValueError, KeyError, TypeError, AttributeError, IndexError, subprocess.SubprocessError)

    def status(path, frames, source):
        key = path.relative_to(OUT).as_posix()
        stamp = health.stamp(path)
        if key in files_checked and files_checked[key][:2] == stamp:
            return files_checked[key][2]
        old = previous.get(key)
        if isinstance(old, list) and old[:2] == stamp:
            denied = old[2:5] == ["failed", source, "permission"] and old[6:] == [os.access(path.parent, os.W_OK)]
            if old[2:] == ["ok"] or old[2:5] == ["failed", source, "source"] or denied:
                files_checked[key] = old
                return old[2]
        verdict = "damaged"
        if stamp[0] is not None:
            info = path.stat()
            inode = (info.st_dev, info.st_ino or key, *stamp)
            if inode not in inodes:
                inodes[inode] = complete_opus(path, frames, accept_decodable=True)
            if inodes[inode]:
                verdict = "ok"
        files_checked[key] = [*stamp, verdict]
        return verdict

    def failed(path, source, error):
        key = path.relative_to(OUT).as_posix()
        if health.permission_failure(error):
            reason = ["permission", str(error), os.access(path.parent, os.W_OK)]
        elif health.retryable_io(error) or isinstance(error, subprocess.SubprocessError):
            reason = ["retry", str(error)]
        else:
            reason = ["source", str(error)]
        files_checked[key] = [*health.stamp(path), "failed", source, *reason]
        print(f"Enhanced Sound kept {key}: {error}", flush=True)

    previous_battle = None
    for battle, stream, key, report in prepared_streams():
        if progress and battle is not None and battle != previous_battle:
            progress(battle.name)
            previous_battle = battle
        try:
            files = report["files"]
            if any(not name or name in (".", "..") or any(c in name for c in "/\\:") for name in (stream, key)):
                raise ValueError("Invalid stream name")
            for layout, file in files.items():
                suffix = "." + layout if layout in ("5.1", "7.1") else ""
                if file != f"streams/{stream}/{key}{suffix}.opus":
                    raise ValueError("Invalid stream layout path")
            if not files:
                continue
            frames = report["frames"]
            index = PUBLIC / "streams" / (stream + ".json")
            bank = index.with_suffix(".bin")
            source = [health.stamp(index), health.stamp(bank)]
            bad = {layout for layout, file in files.items() if status(OUT / file, frames, source) == "damaged"}
            if bad:
                with segment_lock(OUT, stream, key):
                    bad = {layout for layout in bad if status(OUT / files[layout], frames, source) == "damaged"}
                    if bad:
                        try:
                            args = segment_args(stream, key, json.loads(index.read_bytes()), bank.read_bytes(),
                                                report["kind"], report["loop"], OUT)
                            encode_segment(args, bad)
                            for layout in bad:
                                master = OUT / files[layout]
                                if status(master, frames, source) != "ok":
                                    failed(master, source, "incomplete encode")
                                else:
                                    encoded += 1
                        except errors as error:
                            for layout in bad:
                                failed(OUT / files[layout], source, error)
            if battle is not None:
                for file in files.values():
                    master, target = OUT / file, battle / file
                    master_state = status(master, frames, source)
                    alias_source = [*source, health.stamp(master)]
                    if status(target, frames, alias_source) == "damaged":
                        if master_state != "ok":
                            failed(target, alias_source, "shared stream unavailable")
                            continue
                        try:
                            place_stream(master, target)
                            files_checked[target.relative_to(OUT).as_posix()] = [*health.stamp(target), "ok"]
                            linked += 1
                        except errors as error:
                            failed(target, alias_source, error)
        except errors as error:
            print(f"Could not verify Enhanced Sound stream {stream}/{key}: {error}", flush=True)
    record["streams"] = files_checked
    record["failedStreams"] = sum(row[2] == "failed" for row in files_checked.values())
    if any(row[2] == "failed" and row[4] == "retry" for row in files_checked.values()):
        record["retryEnhanced"] = health.retry_later(OUT, "stream encoder or filesystem error")
    else:
        record.pop("retryEnhanced", None)
    health.save(root, record)
    return encoded, linked


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
    parser.add_argument("--stream-session", type=Path, help=argparse.SUPPRESS)
    parser.add_argument("--repair-streams", action="store_true", help="repair incomplete prepared Opus streams")
    parser.add_argument("--limit", type=int, default=0, help="process only this many items of each kind (a quick look)")
    args = parser.parse_args()
    if args.repair_streams:
        import zero_sound_health as health
        if not os.access(OUT, os.W_OK):
            return 0
        manifests_ok = restore_battle_manifests()
        record = health.load(OUT.parent.parent)
        previous = record.get("enhanced", {})
        current = health.inputs(PUBLIC, enhanced=True)
        changed = {name for name in current.keys() | previous.keys() if previous.get(name) != current.get(name)}
        if not previous or any(not name.startswith("enhanced/") or name.endswith("/manifest.json") for name in changed):
            changed = None
        samples_ok = True
        try:
            if changed is None or changed:
                samples_ok = repair_samples(changed=changed)
        except (OSError, ValueError, KeyError, TypeError, IndexError, AttributeError, subprocess.SubprocessError) as error:
            samples_ok = False
            print(f"Enhanced Sound sample repair deferred: {error}", flush=True)
        from prepare import emit
        def progress(bank):
            emit("step", key="sound:enhanced", status="running", phase="extra", weight=1,
                 label="Verifying Enhanced Sound", detail=f"Verifying Enhanced Sound: {bank}")
        encoded, linked = repair_streams(progress, record=record)
        print(f"Enhanced Sound repair: {encoded} streams encoded, {linked} battle files restored", flush=True)
        return int(not (manifests_ok and samples_ok) or bool(record["failedStreams"]))
    src = PUBLIC / args.sublevel / "sound.json"
    if not src.exists():
        print(f"no {src}: run tools/extract.py first", file=sys.stderr)
        return 1
    manifest = json.loads(src.read_text())
    pcm = (PUBLIC / args.sublevel / "bank.bin").read_bytes()
    out = OUT / args.sublevel
    out.mkdir(parents=True, exist_ok=True)
    result_path = out / "manifest.json"
    try:
        result = json.loads(result_path.read_bytes())
        if not isinstance(result.get("streams"), dict) or not isinstance(result.get("samples"), dict):
            raise ValueError("Invalid Enhanced Sound manifest")
    except (OSError, ValueError, TypeError, AttributeError):
        result = {}
    result.update({"version": VERSION, "tool": "tools/enhance_audio.py", "rate": RATE, "method": "dsp"})
    result.setdefault("samples", {})
    result.setdefault("streams", {})
    started = time.time()
    if args.only != "streams":
        kinds, loops = sample_kinds(manifest)
        choices = session_sample_choices(args.stream_session)
        jobs = []
        reused = 0
        for sid, (offset, frames, rate) in manifest["samples"].items():
            kind, loop = kinds.get(sid, "effects"), sid in loops
            variant = sid in choices and not is_swbf2()
            if variant:
                job = chosen_sample(sid, choices[sid][args.sublevel], variant=True)
                report = cached_report(OUT / "samples" / f"{job[-1]}.json", kind, loop, OUT, job[6])
            elif is_swbf2():
                job = chosen_sample(sid, choices.get(sid, [args.sublevel, offset, frames, rate, kind, loop]))
                report = cached_report(OUT / "samples" / f"{sid}.json", job[3], job[4], OUT, job[6])
            else:
                job = (sid, pcm[offset:offset + 2 * frames], rate, kind, loop, str(OUT))
                report = cached_sample(sid, kind, loop)
            if report:
                result["samples"][sid] = battle_sample(sid, report, out, variant)
                reused += 1
                continue
            jobs.append(job)
        if args.limit:
            jobs = jobs[: args.limit]
        if jobs:
            with ProcessPoolExecutor(args.jobs) as pool:
                for sid, report in pool.map(sample_job, jobs, chunksize=4):
                    result["samples"][sid] = battle_sample(sid, report, out, sid in choices and not is_swbf2())
        print(f"samples {len(jobs)} made, {reused} made before: {sum(1 for r in result['samples'].values() if r['fallback'])} fall back", flush=True)
    if args.only != "samples":
        kinds = stream_kinds(manifest)
        jobs = []
        reused = 0
        for name in manifest.get("streams", []):
            index = json.loads((PUBLIC / "streams" / f"{name}.json").read_text())
            data = (PUBLIC / "streams" / f"{name}.bin").read_bytes()
            settings = kinds.get(name, ("voice" if "vo" in name else "ambience", False))
            kind, loop = shared_stream_settings(args.stream_session, name, settings)
            items = list(index["segments"].items())
            if args.limit:
                items = items[: args.limit]
            for key, _ in items:
                report = cached_segment(name, key, kind, loop)
                if report:
                    result["streams"].setdefault(name, {})[key] = report
                    reused += 1
                    continue
                jobs.append(segment_args(name, key, index, data, kind, loop, OUT))
        # The long segments (music) first, so no one of them ends the run alone.
        jobs.sort(key=lambda job: -job[6])
        if jobs:
            with ProcessPoolExecutor(args.jobs) as pool:
                for stream, key, report in pool.map(segment_job, jobs):
                    result["streams"].setdefault(stream, {})[key] = report
        for stream, segments in result["streams"].items():
            for report in segments.values():
                for layout, source in segment_files(report, OUT).items():
                    place_stream(source, out / report["files"][layout])
        print(f"stream segments {len(jobs)} made, {reused} made before", flush=True)
    write_json(result_path, result)
    print(f"wrote {out} in {time.time() - started:.0f} s", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
