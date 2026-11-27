"""2005 stream slots, parameter bindings, room gains and inherited sound properties."""

from pathlib import Path
import struct
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
from test_addon_import import chunk, ucfb
from test_extract_swbf2 import _make_entc
from test_swbf2_readers import cfg_scop, lvl
from ucfb import root_section
from zero_hash import fnv1a
import zero_sound
import zero_sound2


def field(name, *args):
    key = fnv1a(name) if isinstance(name, str) else name
    words = [struct.pack("<f", v) if isinstance(v, float) else struct.pack("<I", v) for v in args]
    return chunk(b"DATA", struct.pack("<IB", key, len(words)) + b"".join(words))


class Sound2Tests(unittest.TestCase):
    def test_append_targets_follow_the_recorder_handles(self):
        calls = [("OpenAudioStream", ["a.lvl", "spindle"]), ("OpenAudioStream", ["b.lvl", "loom"]),
                 ("AudioStreamAppendSegments", ["b.lvl", "reed", 2]), ("AudioStreamAppendSegments", ["a.lvl", "draft", 1]),
                 ("AudioStreamAppendSegments", ["c.lvl", "lost", 8]), ("ScaleSoundParameter", ["warp", "pitch", .25]),
                 ("ScriptCB_SetDopplerFactor", [.125]), ("ScriptCB_EnableHeroVO", [1]), ("ScriptCB_EnableHeroMusic", [0]),
                 ("SetLowReinforcementsVoiceOver", [4, 6, "bobbin", .375, 1])]
        mission = zero_sound2.mission_audio(calls)
        self.assertEqual(mission["appendedStreams"], [["loom","reed"], ["spindle","draft"]])
        self.assertEqual(mission["streamRequests"], [a[:2] for _,a in calls[:5]])
        self.assertEqual(mission["streams"], [a for _,a in calls[:2]])
        self.assertEqual(mission["unresolvedStreamAppends"], [["c.lvl","lost",8]])
        self.assertEqual(mission["lowReinforcementsVo"], [[6,.375,"bobbin",True,4]])
        self.assertEqual(mission["scaledParameters"], [["warp","pitch",.25]])
        self.assertEqual((mission["dopplerFactor"],mission["enableHeroVo"],mission["enableHeroMusic"]), (.125,True,False))
        self.assertNotIn("appendedStreams", zero_sound.read_mission_audio(calls))

    def test_all_graphs_neighbor_hashes_and_playback_time(self):
        fields = field("SoundParameterized") + cfg_scop(field("Layer"), cfg_scop(
            field("Name", 10), field("ParameterGraph", 11, 12, 13), field("ParameterGraph", 21, 22, 23)))
        fields += field("space") + cfg_scop(field(0x148f0d58), cfg_scop(
            field("Space", 0xffdbeeff, .25), field("Space", 0x12345678, .5)), field("MaxPlaybackTime", 3.25))
        graph, room = zero_sound2.config_objects(fields, [("snd_", 0, len(fields))])
        rows = [[zero_sound.hexname(n) for n in r] for r in ((11,12,13),(21,22,23))]
        self.assertEqual(graph["Layer.children"][0]["ParameterGraphs"], rows)
        self.assertNotIn("ParameterGraph", graph["Layer.children"][0])
        self.assertEqual(room["SpaceAttenuations"], [["0xffdbeeff",.25],["0x12345678",.5]])
        self.assertEqual(room["MaxPlaybackTime"], 3.25)
        legacy = zero_sound.config_objects(fields, [("snd_",0,len(fields))])
        self.assertEqual(legacy[0]["Layer.children"][0]["ParameterGraph"], rows[-1])

    def test_class_parent_outside_loaded_package_and_child_override(self):
        child = _make_entc("shuttle", "relay", [("OpenSound","click"), ("OpenSound","chime")])
        parent = _make_entc("relay", "prop", [("BoostSound","hum"), ("OpenSound","clank"), ("WarmUpTime","1.25")])
        outer = ucfb(child, lvl("hidden", parent))
        classes = zero_sound2.class_sounds([], set(), [(outer,[root_section(outer)])])
        self.assertEqual(classes["shuttle"], {"OpenSound":["click","chime"], "BoostSound":"hum","WarmUpTime":"1.25"})
