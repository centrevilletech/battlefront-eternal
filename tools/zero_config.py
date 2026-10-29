"""Config chunks (`sky_`, `fx__`, ...): nested sections of named values.

A DATA chunk is the hashed name (u32), a value count (u8), that many 4-byte
values, then a string table (u32 size, strings). A value is a float, or an
offset into the string table: the string sits at values_start + offset + 4.
A SCOP chunk right after a DATA holds that DATA's section.

The names hash like property names (zero_hash.fnv1a); NAMES lists the ones
the port reads.
"""

import struct

from ucfb import chunks, iter_archives, u32
from zero_hash import fnv1a

NAMES = {fnv1a(name): name for name in """
SkyInfo SunInfo DomeInfo DomeModel LowResTerrain FlatInfo RainInfo StarsInfo WaterInfo SkyObject
Enable FogColor FogRange NearSceneRange FarSceneRange AmbientColor TopDirectionalAmbientColor
BottomDirectionalAmbientColor CharacterAmbientColor Angle Color ShadowColor Texture Degree BackAngle BackColor
BackDegree Geometry Ambient Filter Intensity Softness SoftnessParam PatchResolution MaxDistance TextureScale
FogNear FogFar NumObjects Height Velocity Distance InDirectionFactor VelocityX VelocityY VelocityZ Acceleration LifeTime
ColorControl GammaBrightness GammaContrast GammaColorBalance GammaHue Blur Mode ConstantBlend DownSizeFactor
SunFlare Size FlareOutSize HaloInnerRing HaloMiddleRing HaloOutterRing SpikeColor SpikeSize Godray FogCloud
TerrainColorDarkening TerrainEnable
Effect PatchDivisions LODDecimation Tile MainTexture RefractionColor DiffuseColor ReflectionColor UnderwaterColor
FresnelMinMax NormalMapTextures BumpMapTextures SpecularMaskTextures SpecularMaskTile SpecularMaskScrollSpeed
OscillationEnable OceanEnable WaterRingColor WaterWakeColor WaterSplashColor
FoamTexture FoamTile WindDirection WindSpeed
Type Range ParticleDensity ParticleSize VelocityRange StreakLength AlphaMinMax
GroundEffect GroundEffectSpread GroundEffectsPerSec
Rotation RotationRange ParticleDensityRange CameraCrossVelocityScale CameraAxialVelocityScale VelocityChangeRate
Offset MovementScale
""".split()}


def _data(data, b, e, names):
    key = u32(data, b)
    count = data[b + 4]
    at = b + 5
    table = at + 4 * count
    table_size = u32(data, table) if table + 4 <= e else 0
    values = []
    for i in range(count):
        raw = data[at + 4 * i:at + 4 * i + 4]
        as_int = struct.unpack("<I", raw)[0]
        if table_size and 4 * count <= as_int < 4 * count + table_size + 4:
            start = at + as_int + 4
            stop = data.find(b"\x00", start, e)
            values.append(data[start:stop].decode("latin1"))
        else:
            values.append(struct.unpack("<f", raw)[0])
    return names.get(key, "%08x" % key), values


def parse(data, b, e, names=NAMES):
    """A config body as a list of (name, values, children) entries."""
    entries = []
    for tag, cb, ce in chunks(data, b, e):
        if tag == b"DATA":
            name, values = _data(data, cb, ce, names)
            entries.append([name, values, []])
        elif tag == b"SCOP" and entries:
            entries[-1][2] = parse(data, cb, ce, names)
    return entries


def configs(data, tag):
    """Every config chunk with this tag in an archive, parsed."""
    found = []
    for start, end in iter_archives(data):
        for t, b, e in chunks(data, start, end):
            if t == tag:
                found.append(parse(data, b, e))
    return found


def section(entries, *path):
    """The children of a nested section, by name path."""
    for name in path:
        match = next((entry for entry in entries if entry[0] == name), None)
        if match is None:
            return []
        entries = match[2]
    return entries


def value(entries, name, default=None):
    """The values of a named entry in a section."""
    match = next((entry for entry in entries if entry[0] == name), None)
    return match[1] if match else default
