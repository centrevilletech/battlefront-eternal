"""Map-authored precipitation; absent/disabled effects never invent weather.

Type("Streaks") is rain (src/weather.js); Type("Quads") is snow
(src/snow.js): textured squares spinning at Rotation ± RotationRange / 2
turns a second. The Wind effect's velocity changes every
1 / VelocityChangeRate seconds by up to VelocityRange.
"""
from zero_config import value


def parse_weather(configs):
    effects = {values[0]: children for entries in configs
               for name, values, children in entries if name == 'Effect' and values}
    rain = effects.get('Precipitation')
    if not rain or not value(rain, 'Enable', [0])[0]:
        return None
    keys = ('Type', 'Range', 'Color', 'Velocity', 'VelocityRange', 'ParticleDensity', 'ParticleDensityRange',
            'ParticleSize', 'StreakLength', 'AlphaMinMax', 'Texture', 'Rotation', 'RotationRange',
            'CameraCrossVelocityScale', 'CameraAxialVelocityScale',
            'GroundEffect', 'GroundEffectSpread', 'GroundEffectsPerSec')
    wind = effects.get('Wind', [])
    blowing = bool(value(wind, 'Enable', [0])[0])
    weather = {'precipitation': {k: value(rain, k) for k in keys if value(rain, k) is not None},
               'wind': value(wind, 'Velocity', [0, 0]) if blowing else [0, 0]}
    if blowing:
        weather['gusts'] = {'range': value(wind, 'VelocityRange', [0])[0], 'rate': value(wind, 'VelocityChangeRate', [0])[0]}
    return weather


def weather_textures(weather):
    return set(weather['precipitation'].get('Texture', [])[:1]) if weather else set()
