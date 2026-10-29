"""Vehicle intake: map spawn records, hover rigs, weapons and collision.

Retail data is written only to ignored public/maps/<map>/vehicles/, a pack
per battle (pack_file). No model or balance constants from an archive are
embedded in the runtime.

    python3 tools/zero_vehicle.py nab2 | --all-extracted
"""
import json
import array
import math
import re
import shutil
from pathlib import Path

from zero_hash import fnv1a
from zero_odf import NAMES, entity_classes, property_name
from ucfb import nodes

VEHICLE_KEYS = (
    'ControlZone', 'SpawnTime', 'DecayTime', 'SetAltitude', 'LiftSpring', 'LiftDamp',
    'Acceleration', 'Deceleration', 'ForwardSpeed', 'ReverseSpeed', 'StrafeSpeed',
    'TurnRate', 'PitchRate', 'VehicleType', 'Controllable', 'PilotAnimation', 'PilotPosition', 'Pilot9Pose',
)
NAMES.update({fnv1a(k): k for k in VEHICLE_KEYS})
# A hover's ground physics (docs/vehicles.md "Hovers on the ground"; names
# recovered from their hashes): the class's rates, and AddSpringBody with the
# keys that change the spring it last added (hover_springs).
HOVER_KEYS = (
    'GravityScale', 'Traction', 'VelocitySpring', 'VelocityDamp', 'OmegaXSpring', 'OmegaXDamp',
    'OmegaZSpring', 'OmegaZDamp', 'LevelSpring', 'LevelDamp', 'SpinRate', 'StrafeRollAngle',
    'ThrustPitchAngle', 'BankAngle', 'BankFilter', 'NoRandomSpring', 'PCPitchRate', 'PCSpinRate', 'PCTurnRate',
    'AddSpringBody', 'BodySpringLength',
)
SPRING_FACTORS = (
    'BodyVelocitySpringFactor', 'BodyVelocityDampFactor', 'BodyOmegaXSpringFactor',
    'BodyOmegaXDampFactor', 'BodyOmegaZSpringFactor', 'BodyOmegaZDampFactor',
)
NAMES.update({fnv1a(k): k for k in HOVER_KEYS + SPRING_FACTORS})
# The share of a new spring's radius it may be pressed in (its travel),
# 0.5 until a key whose name is not known (77d6d149) sets it; GeometryName
# puts it back.
SPRING_TRAVEL = 0.5
SPRINGS = 6
# A seat's camera and aim keys the original game reads anew each time they
# appear, so a section's later value replaces its earlier one (the AT-AT
# gunner's section gives PitchLimits and YawLimits twice; the second wins).
# f2aa4a90 is the unnamed key that places a turret's pivot without a node.
OVERWRITTEN = ('PitchLimits', 'YawLimits', 'EyePointOffset', 'TrackCenter', 'TrackOffset', 'TiltValue',
               'FirstPersonFOV', 'ThirdPersonFOV', 'PitchTurnFactor', 'YawTurnFactor', 'TurretNodeName', 'f2aa4a90')
# An aimer's and its barrels' keys, named from their hashes:
# BarrelNodeName 1e534b12, NextBarrel e98f377c, BarrelRecoil 5a29d1cb,
# BarrelLength ef095ea9, FireNodeName 2e5375da (a second name for
# FirePointName), HierarchyLevel 407e801e, AimerYawRate 486a6946,
# AimerPitchRate 4106141f. The AT-AT pilot's 0df83e75 ("FirePoint") is read
# by nothing in the original game.
AIMER_KEYS = ('AimerPitchLimits', 'AimerYawLimits', 'AimerYawRate', 'AimerPitchRate', 'HierarchyLevel', 'FireOutsideLimits')
BARREL_KEYS = ('BarrelNodeName', 'BarrelRecoil', 'BarrelLength')
NAMES.update({fnv1a(k): k for k in AIMER_KEYS + BARREL_KEYS + ('NextBarrel', 'FireNodeName')})
# A walker's legs on the ground (docs/vehicles.md "Standing on the ground"):
# LegPairCount, then per LegPair its thigh bones, top bones, the terrain
# primitives its feet stand on and its foot bones; LegRayHitLength (2.5 unless
# set), WalkerWidth (1) and WalkerOrientRoll.
LEG_KEYS = ('LegPairCount', 'LegPair', 'LegBoneLeft', 'LegBoneRight', 'LegBoneTopLeft', 'LegBoneTopRight',
            'TerrainLeft', 'TerrainRight', 'FootBoneLeft', 'FootBoneRight', 'LegRayHitLength', 'WalkerWidth', 'WalkerOrientRoll')
NAMES.update({fnv1a(k): k for k in LEG_KEYS})
NAMES[0x2AEA4ADD] = 'LegPair'  # the pair's name ("front_legs"); its key's spelling is not known
# Section marker independently identified by its BODY / TURRETn values.
NAMES[0x4CBE283C] = 'ControlSection'
NAMES[0x1CD9F762] = 'ControlSection'
for faction in ('All', 'CIS', 'Imp', 'Rep', 'Loc', 'His'):
    for role in ('ATK', 'DEF'):
        key = f'Class{faction}{role}'
        NAMES[fnv1a(key)] = key
# What the hull shows beyond its model (vehicle_fx), named by their hashes
# (docs/vehicles.md, "Effects and sounds"). ExplosionDestruct is a third
# name for ExplosionName in the flyer and hover parsers. A walker's dying
# keys (07fe74d0 and 871eb169 before they were named) are read by the
# walker parser alone.
FX_KEYS = (
    'ThrustEffect', 'ThrustAttachPoint', 'ThrustAttachOffset', 'ThrustEffectMinScale', 'ThrustEffectMaxScale',
    'ThrustEffectScaleStart', 'MinSpeed', 'DamageStartPercent', 'DamageStopPercent', 'DamageEffect', 'DamageEffectScale',
    'DamageInheritVelocity', 'DamageAttachPoint', 'DamageAttachOffset', 'WaterEffect', 'WakeEffect', 'TerrainCollision',
    'StompEffect', 'StompDecal', 'StompDecalSize', 'StompDetectionType', 'StompThreshold', 'DeathDustEffect',
    'DeathDustOffset', 'DeathDustDelay', 'ChunkSmokeEffect', 'ChunkSmokeNodeName', 'ChunkBounciness', 'ChunkStickiness',
    'ChunkUpFactor', 'ExplosionDestruct', 'ExplosionOffset', 'DeathAnimationExplosion', 'DeathAnimationExplosionTime',
    'FinalExplosion', 'FinalExplosionOffset', 'HydraulicSoundHeight',
)
# A walker's foot rules at the walker class's defaults:
# StompDetectionType 0, StompThreshold 0.15 m, HydraulicSoundHeight 1 m.
STOMP_DEFAULTS = {'detection': 0.0, 'threshold': 0.15, 'hydraulic': 1.0}
# The walker's death animation, and the clips' rate (tools/zero_anim.py).
DEATH_CLIP = 'death01'
# The clips a walker's bank holds for the walker class.
WALKER_ACTIONS = ('idle', 'death01', 'idle_to_leftfoot', 'idle_to_rightfoot',
                  'leftfoot_to_idle', 'rightfoot_to_idle', 'leftfoot_up', 'rightfoot_up',
                  'turnl', 'turnr', 'walk_leftfoot_rightfoot', 'walk_rightfoot_leftfoot')
# A flyer's: the flyer class looks up its AnimationName bank's "takeoff"
# (the landing gear, gear doors and an X-wing's S-foils, landed at its first
# frame and stowed at its last; src/flight/gear.js) and "fins", which no
# stock bank holds.
FLYER_ACTIONS = ('takeoff',)
DYING_INTERVAL = 0.4  # s between dying explosions, DeathAnimationExplosionTime's default
DYING_EXPLOSIONS = 4  # the walker class keeps the first four
NAMES.update({fnv1a(k): k for k in FX_KEYS})


def sections(props):
    """Keep seat boundaries and fire points; repeated WeaponName is not a dict."""
    seats = []
    seat = None
    weapon = None
    aimer = barrel = None
    for key, value in props:
        if key == 'CHUNKSECTION':
            break
        if key == 'ControlSection' and value == 'BODY' and seat is not None and seat['name'] == 'BODY':
            continue
        if key == 'ControlSection' or seat is None:
            seat = {'name': value if key == 'ControlSection' else 'BODY', 'settings': {}, 'weapons': []}
            seats.append(seat)
            weapon = aimer = barrel = None
        if key == 'WeaponSection':
            weapon = {'aimers': []}
            seat['weapons'].append(weapon)
            aimer = barrel = None
        elif key == 'WeaponName':
            # WeaponName fills the seat's current slot (WEAPONSECTION n picks
            # slot n; a seat starts at the first). A
            # second name without a section of its own renames that slot's
            # weapon and its aimers carry on: the gunship's TURRET1 is one
            # wing laser firing from both wings, not one per trigger.
            if weapon is None:
                weapon = {'aimers': []}
                seat['weapons'].append(weapon)
            weapon['class'] = value
        elif key == 'WeaponAmmo' and weapon is not None:
            weapon['ammo'] = float(value)
        elif key == 'NextAimer':
            # The next aimer's keys follow, whichever comes first: the AT-ST
            # gunner's give each gun's limits before its AimerNodeName.
            aimer = barrel = None
        elif key == 'NextBarrel':
            barrel = None
        elif key in ('AimerNodeName', 'FirePointName', 'FireNodeName') + AIMER_KEYS + BARREL_KEYS:
            if aimer is None:
                aimer = {}
                barrel = None
                # Turret aimers precede their WeaponName. Retain them on the seat.
                (weapon['aimers'] if weapon is not None else seat.setdefault('aimers', [])).append(aimer)
            if key == 'AimerNodeName':
                aimer['node'] = value
            elif key in BARREL_KEYS:
                if barrel is None:
                    barrel = {}
                    aimer.setdefault('barrels', []).append(barrel)
                barrel[key] = value
            elif key in ('FirePointName', 'FireNodeName'):
                # A barrel's own fire point, else the aimer's.
                (barrel if barrel is not None else aimer)['FirePointName'] = value
            else:
                aimer[key] = value
        elif key in OVERWRITTEN:
            seat['settings'][key] = value
        else:
            seat['settings'].setdefault(key, value)
    # A record that names no aimer node and no barrel (a hover's lone
    # FirePointName, a seat's stray limits) aims nothing: its fire point is
    # the seat's, as it was read before records were kept.
    for seat in seats:
        for owner in [seat, *seat['weapons']]:
            kept = []
            for aimer in owner.get('aimers', []):
                if aimer.get('node') or any(b.get('BarrelNodeName') or b.get('FirePointName') for b in aimer.get('barrels', [])):
                    kept.append(aimer)
                    continue
                for key, value in aimer.items():
                    if key != 'barrels':
                        seat['settings'].setdefault(key, value)
            if 'aimers' in owner:
                owner['aimers'] = kept
    return seats


def hover_springs(props):
    """A hover's springs, as the original game builds them:
    each AddSpringBody x y z r is a sphere at (x, y, z) in the hull's frame,
    pressed in at most its travel (SPRING_TRAVEL x r), with six factors: on
    its lift, its lift's damping, its pitch, its pitch's damping, its roll
    and its roll's damping. The pitch factor is -1 in front (z over 0.1),
    1 behind (under -0.1), else 0; the roll factor 1 on the +x side, -1 on
    the other, else 0; the rest 1. BodySpringLength (its travel, m) and
    the Body...Factor keys set the spring added last. Six at most."""
    springs, travel = [], SPRING_TRAVEL
    for key, value in props:
        if key == 'GeometryName':
            travel = SPRING_TRAVEL
        elif key == '77d6d149':
            travel = _leading(value, travel)
        elif key == 'AddSpringBody':
            at = _leading_floats(value)
            if len(at) < 4 or len(springs) >= SPRINGS:
                continue
            x, y, z, r = at[:4]
            pitch = -1.0 if z > 0.1 else 1.0 if z < -0.1 else 0.0
            roll = 1.0 if x > 0.1 else -1.0 if x < -0.1 else 0.0
            springs.append({'at': [x, y, z], 'radius': r, 'length': travel * r, 'factors': [1.0, 1.0, pitch, 1.0, roll, 1.0]})
        elif key == 'BodySpringLength' and springs:
            springs[-1]['length'] = _leading(value, springs[-1]['length'])
        elif key in SPRING_FACTORS and springs:
            i = SPRING_FACTORS.index(key)
            springs[-1]['factors'][i] = _leading(value, springs[-1]['factors'][i])
    return springs


_NUMBER = re.compile(r'\s*([-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?)')


def _leading_floats(value):
    """The numbers a value starts with, as the parser's sscanf reads them
    ("2.0//low" is 2)."""
    out, text = [], str(value)
    while True:
        m = _NUMBER.match(text)
        if not m:
            return out
        out.append(float(m.group(1)))
        text = text[m.end():]


def _leading(value, default):
    out = _leading_floats(value)
    return out[0] if out else default


def _floats(value, count=None):
    try:
        out = [float(v) for v in str(value).split()]
    except ValueError:
        return None
    return out if count is None or len(out) >= count else None


def _number(value, default=0.0):
    out = _floats(value)
    return out[0] if out else default


def _quaternion(m):
    """The rotation of a column-major 4x4 (its first three columns) as a
    unit quaternion [x, y, z, w]."""
    m00, m10, m20, m01, m11, m21, m02, m12, m22 = m[0], m[1], m[2], m[4], m[5], m[6], m[8], m[9], m[10]
    trace = m00 + m11 + m22
    if trace > 0:
        s = 0.5 / math.sqrt(trace + 1.0)
        q = [(m21 - m12) * s, (m02 - m20) * s, (m10 - m01) * s, 0.25 / s]
    elif m00 > m11 and m00 > m22:
        s = 2.0 * math.sqrt(1.0 + m00 - m11 - m22)
        q = [0.25 * s, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s]
    elif m11 > m22:
        s = 2.0 * math.sqrt(1.0 + m11 - m00 - m22)
        q = [(m01 + m10) / s, 0.25 * s, (m12 + m21) / s, (m02 - m20) / s]
    else:
        s = 2.0 * math.sqrt(1.0 + m22 - m00 - m11)
        q = [(m02 + m20) / s, (m12 + m21) / s, 0.25 * s, (m10 - m01) / s]
    n = math.sqrt(sum(v * v for v in q)) or 1.0
    return [round(v / n, 4) + 0.0 for v in q]


def foot_bottom(shape):
    """Where a walker's foot primitive meets the ground, in its bone's frame,
    as the walker's foot update finds it: the primitive's centre less its up
    axis (its Y) times half the height it reports (the collision shape's
    second size: a sphere's diameter, a cylinder's height, a box's second
    size; kinds 0, 1 and 3 are spheres, 2 a cylinder and 4 a box). With it,
    the primitive's turn in that frame: the stomp effect plays turned as the
    foot is."""
    m, size, kind = shape['matrix'], shape.get('size') or (0.0, 0.0, 0.0), shape.get('kind')
    height = 2 * size[0] if kind in (0, 1, 3) else size[1] if kind in (2, 4) else 0.0
    return ([round(m[12 + i] - m[4 + i] * height / 2, 4) + 0.0 for i in range(3)], _quaternion(m))


def vehicle_fx(props, shapes=(), skeleton=None, walker=False):
    """What a hull shows and plays beyond its model, read from the whole
    class as the original game reads it (sections() stops at the first
    CHUNKSECTION, after which most of these come). docs/vehicles.md,
    "Effects and sounds", has the rules the runtime applies.

    - thrust: one entry per ThrustEffect (four at most; the attach point,
      offset and scales that follow belong to the last), with speed: the
      flyer's MinSpeed and MaxSpeed (10 and 30 unless set), which place the
      scale's ramp.
    - damage: one entry per DamageStartPercent (ten at most), its stop,
      effect, scale, inherited velocity, attach point and offset following;
      the percentages become shares of MaxHealth.
    - water, wake: the hover's WaterEffect and WakeEffect (the last of each);
      a hover's wake plays at its TerrainCollision primitives (none set any).
    - feet (walker): its TerrainCollision primitives, each with its bone and
      its point in that bone's frame, in their order (FootstepSound<i> is
      the i-th one's), and where it meets the ground (bottom, with the
      primitive's turn; foot_bottom); stomp, for a walker with feet: the
      rules a foot comes down and lifts by (StompDetectionType,
      StompThreshold and HydraulicSoundHeight, STOMP_DEFAULTS unless set)
      and its StompEffect, played where a foot lands. Other hulls'
      TerrainCollision primitives are spheres, not feet.
    - deathDust: DeathDustEffect, its offset in the hull's frame and delay.
    - explosionOffset: where the hull's blast goes off, in its frame (the
      walkers': the AT-AT's body, the AT-ST's head).
    - chunks: the CHUNKSECTIONs, keyed by ODF name as a droid's are.
    - dying (walker): its DeathAnimationExplosion effects (the first four),
      one of which goes off every DeathAnimationExplosionTime (0.4 s unless
      set) while its death animation plays, and finalOffset: where the
      FinalExplosion goes off once it has played, in the hull's frame.
      extract_vehicles adds the animation's length (span) or, for a walker
      without one, drops the entry.
    """
    fx, thrust, damage, chunks, terrain = {}, [], [], [], []
    speed = [10.0, 30.0]
    stomp = dict(STOMP_DEFAULTS)
    dying = {'effects': [], 'interval': DYING_INTERVAL, 'finalOffset': [0.0, 0.0, 0.0]}
    for key, value in props:
        if key == 'ThrustEffect':
            thrust.append({'effect': value, 'node': '', 'offset': [0.0, 0.0, 0.0], 'minScale': 1.0, 'maxScale': 1.0, 'scaleStart': 0.0}
                          if len(thrust) < 4 else None)
        elif key.startswith('Thrust') and thrust and thrust[-1] is not None:
            last = thrust[-1]
            if key == 'ThrustAttachPoint':
                last['node'] = value.lower()
            elif key == 'ThrustAttachOffset':
                last['offset'] = _floats(value, 3)[:3] if _floats(value, 3) else last['offset']
            elif key in ('ThrustEffectMinScale', 'ThrustEffectMaxScale', 'ThrustEffectScaleStart'):
                last[{'ThrustEffectMinScale': 'minScale', 'ThrustEffectMaxScale': 'maxScale',
                      'ThrustEffectScaleStart': 'scaleStart'}[key]] = _number(value)
        elif key == 'MinSpeed':
            speed[0] = _number(value, speed[0])
        elif key == 'MaxSpeed':
            speed[1] = _number(value, speed[1])
        elif key == 'DamageStartPercent':
            damage.append({'start': _number(value) / 100, 'stop': 0.0, 'effect': None, 'scale': 1.0, 'inherit': 0.0, 'node': '',
                           'offset': [0.0, 0.0, 0.0]} if len(damage) < 10 else None)
        elif key.startswith('Damage') and damage and damage[-1] is not None:
            last = damage[-1]
            if key == 'DamageStopPercent':
                last['stop'] = _number(value) / 100
            elif key == 'DamageEffect':
                last['effect'] = value
            elif key == 'DamageEffectScale':
                last['scale'] = _number(value, 1.0)
            elif key == 'DamageInheritVelocity':
                last['inherit'] = _number(value)
            elif key == 'DamageAttachPoint':
                last['node'] = value.lower()
            elif key == 'DamageAttachOffset' and _floats(value, 3):
                last['offset'] = _floats(value, 3)[:3]
        elif key in ('WaterEffect', 'WakeEffect'):
            fx['water' if key == 'WaterEffect' else 'wake'] = value
        elif key == 'TerrainCollision':
            terrain.append(value.lower())
        elif key in ('StompEffect', 'StompDetectionType', 'StompThreshold', 'HydraulicSoundHeight'):
            stomp[{'StompEffect': 'effect', 'StompDetectionType': 'detection', 'StompThreshold': 'threshold',
                   'HydraulicSoundHeight': 'hydraulic'}[key]] = value if key == 'StompEffect' else _number(value)
        elif key == 'DeathDustEffect':
            fx.setdefault('deathDust', {'offset': [0.0, 0.0, 0.0], 'delay': 0.0})['effect'] = value
        elif key in ('DeathDustOffset', 'DeathDustDelay'):
            dust = fx.setdefault('deathDust', {'offset': [0.0, 0.0, 0.0], 'delay': 0.0})
            if key == 'DeathDustOffset' and _floats(value, 3):
                dust['offset'] = _floats(value, 3)[:3]
            elif key == 'DeathDustDelay':
                dust['delay'] = _number(value)
        elif key == 'ExplosionOffset' and _floats(value, 3):
            fx['explosionOffset'] = _floats(value, 3)[:3]
        elif key == 'DeathAnimationExplosion':
            if len(dying['effects']) < DYING_EXPLOSIONS:
                dying['effects'].append(value)
        elif key == 'DeathAnimationExplosionTime':
            dying['interval'] = _number(value, DYING_INTERVAL)
        elif key == 'FinalExplosionOffset' and _floats(value, 3):
            dying['finalOffset'] = _floats(value, 3)[:3]
        elif key == 'CHUNKSECTION':
            chunks.append({})
        elif key.startswith('Chunk') and chunks:
            numbers = _floats(value)
            # The extras pack files a piece's model under its lower-case name.
            chunks[-1][key] = (numbers if len(numbers) > 1 else numbers[0]) if numbers else value.lower() if key == 'ChunkGeometryName' else value
    thrust = [t for t in thrust if t]
    if thrust:
        fx['thrust'], fx['speed'] = thrust, speed
    damage = [d for d in damage if d and d['effect']]
    if damage:
        fx['damage'] = damage
    # A primitive's frame is its parent bone's (vehicle_collision); a foot
    # without one hangs from the hull's origin. A name the model lacks keeps
    # its place (and its FootstepSound index) at the origin.
    names = skeleton['names'] if skeleton else []
    feet = []
    for name in terrain:
        shape = next((s for s in shapes if s.get('name', '').lower() == name), None)
        parent = (shape or {}).get('parent') or ''
        at = shape['matrix'][12:15] if shape else [0.0, 0.0, 0.0]
        foot = {'name': name, 'bone': parent.lower() if parent in names else '', 'at': [round(v, 4) for v in at]}
        if shape and 'kind' in shape:
            foot['bottom'], foot['turn'] = foot_bottom(shape)
        feet.append(foot)
    if fx.get('wake'):
        fx['wakeNodes'] = feet
    if feet and walker:
        fx['feet'] = feet
        fx['stomp'] = stomp
    if walker:
        legs = leg_pairs(props, shapes, skeleton)
        if legs:
            fx['legs'] = legs
    if 'deathDust' in fx and not fx['deathDust'].get('effect'):
        del fx['deathDust']
    if chunks:
        fx['chunks'] = chunks
    if walker:
        fx['dying'] = dying
    return fx


def leg_pairs(props, shapes=(), skeleton=None):
    """A walker's leg pairs as the original game keeps them: each
    LegPair's left and right TerrainLeft/TerrainRight primitive, with where
    its bottom (foot_bottom) sits in the hull's frame in the bind pose, and
    its thigh and foot bones; with LegRayHitLength (the reach of the foot
    rays up and down, 2.5 unless set), WalkerWidth (1) and WalkerOrientRoll.
    None for a walker without leg pairs (the AT-TE, the riding animals)."""
    names = skeleton['names'] if skeleton else []
    pairs, out = [], {'rayLength': 2.5, 'width': 1.0, 'orientRoll': False}
    side = {'TerrainLeft': ('left', 'prim'), 'TerrainRight': ('right', 'prim'), 'LegBoneLeft': ('left', 'leg'),
            'LegBoneRight': ('right', 'leg'), 'FootBoneLeft': ('left', 'foot'), 'FootBoneRight': ('right', 'foot')}
    for key, value in props:
        if key == 'CHUNKSECTION':
            break
        if key == 'LegPair':
            pairs.append({'name': value, 'left': {}, 'right': {}})
        elif key in side and pairs:
            which, field = side[key]
            pairs[-1][which][field] = value.lower()
        elif key == 'LegRayHitLength':
            out['rayLength'] = _number(value, 2.5)
        elif key == 'WalkerWidth':
            out['width'] = _number(value, 1.0)
        elif key == 'WalkerOrientRoll':
            out['orientRoll'] = _number(value) != 0
    for pair in pairs:
        for which in ('left', 'right'):
            foot = pair[which]
            shape = next((s for s in shapes if s.get('name', '').lower() == foot.get('prim')), None)
            if not shape or 'kind' not in shape:
                return None
            bottom, _ = foot_bottom(shape)
            parent = (shape.get('parent') or '').lower()
            lowered = [n.lower() for n in names]
            if skeleton and parent in lowered:
                m = skeleton['model'][lowered.index(parent)]
                bottom = [m[i] * bottom[0] + m[4 + i] * bottom[1] + m[8 + i] * bottom[2] + m[12 + i] for i in range(3)]
            foot['at'] = [round(v, 4) + 0.0 for v in bottom]
    if not pairs:
        return None
    out['pairs'] = pairs
    return out


def dying_span(fx, animation, settings):
    """A walker's dying, in seconds: its death animation's frames at the
    clips' rate over its AnimationSpeed (1 unless set), which the walker
    class applies to every one of its animations. Sets fx['dying']['span'],
    or drops fx['dying'] for a walker without a death animation (it is gone
    at once, as every other hull is)."""
    if 'dying' not in fx:
        return fx
    clip = ((animation or {}).get('clips') or {}).get(DEATH_CLIP)
    rate = (animation or {}).get('fps') or 30
    speed = _number(settings.get('AnimationSpeed'), 1.0) or 1.0
    if clip and clip.get('frames', 0) > 0:
        fx['dying']['span'] = round(clip['frames'] / rate / speed, 4)
    else:
        del fx['dying']
    return fx


def bank_clips(archives, bank_name, names, actions):
    """The clips named `actions` of the animation bank `bank_name` (a zaa_
    in the first archive holding one), decoded for the skeleton `names` as
    tools/zero_anim.py writes them: (meta, floats), or None without a bank."""
    from zero_anim import write_anims
    from zero_hash import crc
    import tempfile
    from ucfb import node_name
    source = next((a for a in archives if any(node_name(a, b, e).lower() == bank_name for b, e in nodes(a, b'zaa_'))), None)
    if not source:
        return None
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp)
        banks = [node_name(source, b, e) for b, e in nodes(source, b'zaa_') if node_name(source, b, e).lower() == bank_name]
        write_anims(source, names, path, banks=banks, names={crc(n): n for n in actions}, file='vehicle')
        return json.loads((path / 'vehicle.json').read_text()), (path / 'vehicle.bin').read_bytes()


def flyer_clips(meta, names):
    """A flyer's bank as the port draws it: the clips that move a bone
    other than DummyRoot (whose constant lift in a takeoff clip seats the
    model on its origin; the port seats a hull by its collision). The cloud
    car's takeoff moves only that: its other tracks name bones its model
    lacks. None when nothing is left."""
    clips = {name: clip for name, clip in meta['clips'].items()
             if any(names[t].lower() != 'dummyroot' for t in clip['tracks'])}
    return {**meta, 'clips': clips} if clips else None


def vehicle_collision(model, cls, meshes, shapes, skeleton):
    """Collision attached to walker bones must retain its bind transform.

    The static world exporter deliberately skips those bones; vehicles cannot.
    """
    from extract import new_polys, place_mesh, place_shape, mul, IDENTITY
    poly = new_polys()
    wanted = {v.lower() for k, v in cls['props'] if k == 'OrdnanceCollision'}
    if not wanted or 'collisionmesh' in wanted:
        for positions, faces in meshes.get(model, []):
            place_mesh(positions, faces, IDENTITY, poly)
    for shape in shapes.get(model, []):
        if wanted and shape['name'].lower() not in wanted:
            continue
        matrix = shape['matrix']
        parent = shape.get('parent')
        if skeleton and parent in skeleton['names']:
            matrix = mul(skeleton['model'][skeleton['names'].index(parent)], matrix)
        place_shape(shape['kind'], shape['size'], matrix, poly)
    return {k: list(poly[v]) for k, v in [('positions','pos'), ('indices','idx'), ('polygons','poly')]}


def vehicle_parts(parts, skeleton):
    """Keep per-vertex joints and shared triangles on deforming creatures.

    Rigid-triangle partitioning tears seams where a triangle spans joints.
    Skinned positions here are already in model bind space, as for soldiers.
    """
    out = []
    for part in parts:
        joints = part.get('joints')
        if not joints and part.get('skin') and part.get('bmap') and skeleton:
            bmap = part['bmap']
            joints = [bmap[k] if k < len(bmap) and bmap[k] < len(skeleton['names']) else 0 for k in part['skin']]
        if joints and skeleton:
            if len(joints) != len(part['positions']):
                raise ValueError('Vehicle skin joint count does not match its vertices')
            out.append({**part, 'bone': None, 'joints': joints,
                        'local_positions': part['positions'], 'local_normals': part['normals']})
        else:
            out.append(part)
    return out


def vehicle_spawns(records, factions, attacker=1, historical=False):
    """Import only native pads and the vehicle choices for this mission's
    sides. A mission that calls SetHistorical gives teams 1 and 2 the pads'
    ClassHisATK/ClassHisDEF instead (docs/spawn-screen-assets.md)."""
    from extract import mat4
    spawns = []
    for record in records:
        if record['class'].lower() != 'com_item_vehicle_spawn':
            continue
        p = dict(record['props'])
        choices = {}
        for team, faction in factions.items():
            # A faction's pad can intentionally use another faction's asset
            # (Endor rebel bikes), or a map-local class (Cloud Cars, skiffs).
            # Class name prefixes therefore cannot select the spawn roster.
            prefix = {'all': 'All', 'cis': 'CIS', 'imp': 'Imp', 'rep': 'Rep'}.get(faction, 'Loc')
            if historical and int(team) in (1, 2):
                prefix = 'His'
            role = 'ATK' if int(team) == attacker else 'DEF'
            choice = p.get(f'Class{prefix}{role}')
            if choice:
                choices[team] = choice.lower()
        if choices:
            spawns.append({'name': f"{record['name']}_{len(spawns)}", 'matrix': mat4(record['xfrm']),
                           'team': int(p.get('Team', 0)), 'post': p.get('ControlZone', '').lower(),
                           'respawnTime': float(p.get('SpawnTime', 45)), 'classes': choices})
    return spawns


def pack_file(mission):
    """A battle's vehicle pack, relative to its map's folder: each mission
    has its own (maps/<map>/vehicles/<mission>/), as each era fields its own
    vehicles from the same pads."""
    return f'vehicles/{mission}/vehicles.json'


# The one pack a map had before each battle had its own.
SINGLE_PACK = ('vehicles.json', 'models.bin', 'models_index.bin', 'extras.bin', 'extras_index.bin', 'animations.bin')


def clear_single_pack(map_name):
    """Removes a map's one pack of an older import, if it has one."""
    from extract import PUBLIC
    folder = PUBLIC / 'maps' / map_name / 'vehicles'
    for name in SINGLE_PACK:
        (folder / name).unlink(missing_ok=True)
    shutil.rmtree(folder / 'textures', ignore_errors=True)


def extract_vehicles(map_name, mission, shared=None, setup=None):
    """One battle's vehicle pack (pack_file): its pads' classes for the
    mission's teams, and those classes' rigs, weapons and effects. setup is
    the mission's (extract.map_missions), read when not given."""
    from extract import (PUBLIC, map_archive, map_missions, side_archive, shared_archives,
                         instance_records, mat4, collect_models, MODEL_BOUNDS,
                         write_meshes, collect_textures, write_textures, collision_sources,
                         new_polys, place_collision, IDENTITY, weapon_chain, explosion_class,
                         rounds, weapon_texture_dependencies, effects, effect, effect_textures,
                         localization, localize, GAME)
    shared = shared if shared is not None else shared_archives()
    data = map_archive(map_name).read_bytes()
    setup = setup if setup is not None else map_missions(map_name).get(mission)
    if setup is None:
        raise ValueError(f'{map_name} has no mission {mission}')
    teams = setup.get('teams', {})
    factions = {t: info['classes'][0][0].split('_')[0] for t, info in teams.items() if info.get('classes')}
    from zero_addon import missions
    from zero_lua import calls, load
    historical = any(call == 'SetHistorical' for call, _ in calls(load(missions(GAME)[mission])))
    spawns = vehicle_spawns((record for b, e in nodes(data, b'wrld')
                            for record in instance_records(data, b, e, property_name)), factions, setup.get('attacker') or 1, historical)
    out = PUBLIC / 'maps' / map_name / Path(pack_file(mission)).parent
    out.mkdir(parents=True, exist_ok=True)
    wanted = {name for s in spawns for name in s['classes'].values()}
    archives = [side_archive(f).read_bytes() for f in sorted({n.split('_')[0] for n in wanted}) if side_archive(f).exists()]
    archives += [data, *shared]
    classes, models, skeletons, textures = {}, {}, {}, {}
    for archive in archives:
        classes.update({k.lower(): v for k, v in entity_classes(archive).items() if k.lower() not in classes})
        collect_models(archive, models, skeletons)
        collect_textures(archive, textures)
    from zero_addon import localization as game_localization
    locl = game_localization(GAME)
    meshes, shapes = collision_sources(archives)
    meshes = {k.lower(): v for k,v in meshes.items()}
    shapes = {k.lower(): v for k,v in shapes.items()}
    # Normalize model lookup exactly as the game does.
    models = {k.lower(): v for k, v in models.items()}
    skeletons = {k.lower(): v for k, v in skeletons.items()}
    defs, packed, rigs, collisions = {}, {}, {}, {}
    animations, animation_floats = {}, array.array('f')
    flyer_banks = {}
    used, wanted_fx, flying = set(), set(), set()
    for name in sorted(wanted):
        cls = classes.get(name)
        if not cls or cls['base'] not in ('hover', 'commandhover', 'walker', 'commandwalker', 'flyer'):
            raise ValueError(f'Unsupported or missing vehicle: {name}')
        seats = sections(cls['props'])
        settings = seats[0]['settings']
        model = settings.get('GeometryName', name).lower()
        if not models.get(model):
            raise ValueError(f'Missing vehicle model: {model}')
        skeleton = skeletons.get(model)
        names = skeleton['names'] if skeleton else []
        packed[model] = vehicle_parts(models[model], skeleton)
        rigs[model] = {'bones': [{'name': n, 'parent': names.index(p) if p in names and p != n else -1,
                                'local': skeleton['local'][i]}
                               for i, (n, p) in enumerate(zip(names, skeleton['parents'] if skeleton else []))],
                       **MODEL_BOUNDS.get(model, {})}
        if cls['base'] in ('walker', 'commandwalker') and skeleton:
            bank = bank_clips(archives, settings.get('AnimationName', model).lower(), names, WALKER_ACTIONS)
            if bank:
                meta, floats = bank
                for clip in meta['clips'].values(): clip['offset'] += len(animation_floats)
                animation_floats.frombytes(floats)
                animations[model] = meta
        elif cls['base'] == 'flyer' and skeleton and settings.get('AnimationName'):
            # Only a class that names its bank has one.
            flyer_banks.setdefault(model, (settings['AnimationName'].lower(), names))
        collisions[model] = vehicle_collision(model, cls, meshes, shapes, skeleton)
        if not collisions[model]['indices']:
            raise ValueError(f'Missing vehicle collision: {model}')
        for seat in seats:
            for w in seat['weapons']:
                resolved = weapon_chain(archives, w['class'])
                if not resolved or not resolved.get('ordnance'):
                    raise ValueError(f"Missing vehicle weapon: {w['class']}")
                w['def'] = {**resolved, 'ammo': w.get('ammo', 0)}
                w['aimers'] = w['aimers'] or seat.get('aimers', [])
                used |= weapon_texture_dependencies(resolved)
                if resolved.get('MuzzleFlashEffect'):
                    wanted_fx.add(resolved['MuzzleFlashEffect'])
                for o in rounds(resolved):
                    if o.get('GeometryName'): flying.add(o['GeometryName'])
                    wanted_fx |= {v for k, v in o.items() if ('Effect' in k) and isinstance(v, str)}
                    for k in ('ExplosionImpact', 'ExplosionExpire'):
                        wanted_fx |= {v for e, v in o.get(k, {}).items() if e in ('Effect', 'WaterEffect')}
        # Flyers name their blast after the chunk sections (sections() stops
        # there), most of them as ExplosionDestruct.
        blast = settings.get('ExplosionName') or settings.get('Explosion') or next(
            (v for k, v in cls['props'] if k in ('ExplosionName', 'Explosion', 'ExplosionDestruct')), '')
        explosion = explosion_class(archives, blast)
        if explosion: wanted_fx |= {v for k, v in explosion.items() if k in ('Effect', 'WaterEffect')}
        wanted_fx |= {'vehiclesmoke', 'vehicleflame', 'explosion', 'dustwake'}
        fx = vehicle_fx(cls['props'], shapes.get(model, []), skeleton, cls['base'] in ('walker', 'commandwalker'))
        # The walker class applies its AnimationSpeed wherever it is set.
        dying_span(fx, animations.get(model), {'AnimationSpeed': next((v for k, v in reversed(cls['props']) if k == 'AnimationSpeed'), None)})
        # Once its death animation has played a walker's FinalExplosion goes
        # off (its ExplosionName where it has none).
        final = next((v for k, v in cls['props'] if k == 'FinalExplosion'), '') if 'dying' in fx else ''
        final_explosion = explosion_class(archives, final) if final else None
        if final_explosion: wanted_fx |= {v for k, v in final_explosion.items() if k in ('Effect', 'WaterEffect')}
        wanted_fx |= {e for e in [*(t['effect'] for t in fx.get('thrust', [])), *(d['effect'] for d in fx.get('damage', [])),
                                  fx.get('water'), fx.get('wake'), fx.get('stomp', {}).get('effect'), fx.get('deathDust', {}).get('effect'),
                                  *(c.get(k) for c in fx.get('chunks', []) for k in ('ChunkTrailEffect', 'ChunkTerrainEffect', 'ChunkSmokeEffect')),
                                  *fx.get('dying', {}).get('effects', [])]
                      if isinstance(e, str) and e}
        flying |= {c['ChunkGeometryName'].lower() for c in fx.get('chunks', []) if isinstance(c.get('ChunkGeometryName'), str)}
        used |= {settings[k] for k in ('MapTexture', 'HealthTexture') if settings.get(k)}
        defs[name] = {'base': cls['base'], 'model': model, 'settings': settings, 'seats': seats, 'explosion': explosion, 'fx': fx,
                      'label': localize(locl, 'entity.' + name.replace('_', '.', 1)) or settings.get('Label') or name}
        if cls['base'] in ('hover', 'commandhover'):
            defs[name]['springs'] = hover_springs(cls['props'])
        if final_explosion:
            defs[name]['finalExplosion'] = final_explosion
    # Flyers' clips go after every walker's, so a walker's clips keep the
    # offsets they had before flyers had any. They are only drawn, and the
    # content fingerprint leaves them out (src/runtime/content-hash.js).
    for model, (bank_name, names) in sorted(flyer_banks.items()):
        bank = bank_clips(archives, bank_name, names, FLYER_ACTIONS)
        meta = bank and flyer_clips(bank[0], names)
        if not meta or model in animations:
            continue
        for clip in meta['clips'].values(): clip['offset'] += len(animation_floats)
        animation_floats.frombytes(bank[1])
        animations[model] = meta
    # Vehicle-only architectural barriers keep large hulls out of infantry
    # arcades. Keep these out of soldier/projectile queries.
    blockers = new_polys()
    from extract import parse_instances
    for b, e in nodes(data, b'wrld'):
        for kind, raw in parse_instances(data, b, e):
            cls = classes.get(kind.lower(), {})
            only = {v.lower() for k, v in cls.get('props', []) if k == 'VehicleCollisionOnly'}
            if only:
                # The shapes are the model's (GeometryName), as for soldier collision.
                model = next((v for k, v in cls.get('props', []) if k == 'GeometryName'), kind).lower()
                selected = {model: [s for s in shapes.get(model, []) if s['name'].lower() in only]}
                place_collision(model, mat4(raw), {}, selected, skeletons.get(model), None, blockers)
    fx_sources = [effects(a) for a in archives]
    fx = {n: effect(fx_sources, n) for n in sorted(wanted_fx)}
    fx = {n: v for n, v in fx.items() if v}
    for emitters in fx.values():
        t, m = effect_textures(emitters); used |= t; flying |= m
    parts = write_meshes(packed, sorted(packed), out, 'models', local=True)
    for name, records in parts.items():
        for record, part in zip(records, packed[name]):
            if part.get('joints'):
                record['joints'] = part['joints']
    for name in rigs: rigs[name]['parts'] = parts[name]
    extras = write_meshes(models, sorted(flying), out, 'extras')
    used |= {p['texture'] for ps in [*parts.values(), *extras.values()] for p in ps}
    used.add('lightglow')
    texture_paths = write_textures(textures, used, out / 'textures')
    (out / 'animations.bin').write_bytes(animation_floats.tobytes())
    manifest = {'version': 1, 'animations': animations, 'classes': defs, 'spawns': spawns, 'models': rigs,
                'collision': collisions, 'blockers': {k: list(blockers[v]) for k,v in [('positions','pos'),('indices','idx'),('polygons','poly')]}, 'extras': extras, 'effects': fx, 'textures': texture_paths}
    (out / 'vehicles.json').write_text(json.dumps(manifest))
    print(f'vehicles {map_name} {mission}: {len(spawns)} spawns, {len(defs)} classes, {len(texture_paths)} textures', flush=True)
    return manifest


def refresh_map(map_name):
    """Every battle's pack of an extracted map, named in its map.json.
    Discoverability is explicit: a battle without one plays without
    vehicles, and one whose pack cannot be read fails visibly."""
    from extract import PUBLIC
    map_file = PUBLIC / 'maps' / map_name / 'map.json'
    world = json.loads(map_file.read_text())
    if 'missions' not in world:
        print(f'{map_name}: an import holding one battle; tools/extract.py {map_name} extracts them all', flush=True)
        return
    clear_single_pack(map_name)
    world.pop('vehicles', None)
    for mission, battle in (world.get('missions') or {}).items():
        if not battle['mission'].get('world'):
            extract_vehicles(map_name, mission, setup=battle['mission'])
            battle['vehicles'] = pack_file(mission)
    map_file.write_text(json.dumps(world))


if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('map', nargs='?', default='nab2', help='an extracted map: every battle of it (map.json missions)')
    parser.add_argument('--all-extracted', action='store_true', help='refresh vehicles for every already extracted map')
    args = parser.parse_args()
    from extract import PUBLIC
    names = sorted(path.parent.name for path in (PUBLIC / 'maps').glob('*/map.json')) if args.all_extracted else [args.map]
    for name in names:
        refresh_map(name)
    from ktx2 import encode_optional  # KTX2 copies of the textures just written; optional
    encode_optional([PUBLIC / 'maps' / name / 'vehicles' for name in names])
