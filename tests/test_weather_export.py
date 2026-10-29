import sys
import unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'tools'))
from zero_weather import parse_weather, weather_textures
from zero_water import water_textures
from zero_config import NAMES
from zero_hash import fnv1a

class WeatherExportTests(unittest.TestCase):
    def test_precipitation_is_opt_in_and_preserves_native_settings(self):
        self.assertIsNone(parse_weather([]))
        self.assertIsNone(parse_weather([[['Effect',['Precipitation'],[['Enable',[0],[]]]]]]))
        config=[[['Effect',['Precipitation'],[['Enable',[1],[]],['Type',['Streaks'],[]],['Range',[12.5],[]],['ParticleDensity',[256],[]],['Velocity',[9],[]],['Texture',['rain'],[]]]],['Effect',['Wind'],[['Enable',[1],[]],['Velocity',[1,.3],[]]]]]]
        rain=parse_weather(config)
        self.assertEqual(rain['precipitation']['ParticleDensity'],[256])
        self.assertEqual(rain['wind'],[1,.3])
        self.assertEqual(weather_textures(rain),{'rain'})
    def test_snow_keeps_its_spin_and_the_wind_its_gusts(self):
        config=[[['Effect',['Precipitation'],[['Enable',[1],[]],['Type',['Quads'],[]],['Texture',['fx_snow'],[]],['RotationRange',[2],[]],
                  ['ParticleDensityRange',[0],[]],['CameraCrossVelocityScale',[1],[]]]],
                 ['Effect',['Wind'],[['Enable',[1],[]],['Velocity',[4,.3],[]],['VelocityRange',[1],[]],['VelocityChangeRate',[.2],[]]]]]]
        snow=parse_weather(config)
        self.assertEqual(snow['precipitation']['Type'],['Quads'])
        self.assertEqual(snow['precipitation']['RotationRange'],[2])
        self.assertEqual(snow['precipitation']['CameraCrossVelocityScale'],[1])
        self.assertEqual(snow['gusts'],{'range':1,'rate':.2})
        self.assertNotIn('gusts',parse_weather([[config[0][0]]]),'no Wind effect, no gusts')
    def test_ocean_foam_and_water_textures_are_both_exported(self):
        names=water_textures({'look':{'MainTexture':['sea'],'FoamTexture':['foam']}})
        self.assertTrue({'sea','foam'}.issubset(names))
        for name in ['FoamTexture','FoamTile','WindSpeed','Range','StreakLength','ParticleDensity']:
            self.assertEqual(NAMES[fnv1a(name)],name)

if __name__=='__main__':unittest.main()
