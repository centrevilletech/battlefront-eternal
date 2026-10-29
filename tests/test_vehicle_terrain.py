"""A terrain patch may carry a separate low-LOD vertex/index pair."""
import struct
import sys
import unittest
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
from zero_terrain import parse_terrain

def chunk(tag, data): return tag + struct.pack('<I', len(data)) + data

def ibuf(indices): return chunk(b'IBUF', struct.pack('<I',len(indices)) + struct.pack('<%dH'%len(indices),*indices))

def terrain(cut=False, lod=True):
    info=bytearray(28);struct.pack_into('<f',info,0,1);struct.pack_into('<HH',info,16,8,8)
    vertices=[(0,0,0),(8,0,0),(0,0,8),(8,0,8)]
    v=chunk(b'VBUF',struct.pack('<III',4,28,290)+b''.join(struct.pack('<6f4B',*p,0,1,0,255,255,255,0)for p in vertices))
    if cut:v+=ibuf([0,1,2])
    if lod:v+=chunk(b'VBUF',struct.pack('<III',3,16,130)+bytes(48))+ibuf([0,0,0])
    return chunk(b'INFO',info)+chunk(b'PCHS',chunk(b'COMN',ibuf([0,1,2,3]))+chunk(b'PTCH',v))

class TerrainLOD(unittest.TestCase):
    def test_low_lod_does_not_replace_detailed_strip(self):
        for cut,expected in [(False,6),(True,3)]:
            with self.subTest(cut=cut):
                data=terrain(cut);t=parse_terrain(data,0,len(data))
                self.assertEqual(len(t['indices']),expected)
                baseline=terrain(cut,False)
                self.assertEqual(t,parse_terrain(baseline,0,len(baseline)))

if __name__=='__main__':unittest.main()
