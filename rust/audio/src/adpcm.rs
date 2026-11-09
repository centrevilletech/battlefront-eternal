//! Xbox IMA ADPCM asset decoding. Each channel contributes a four-byte
//! header and eight interleaved four-byte code groups, 64 frames per block.
const STEPS: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190,
    209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272, 2499,
    2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350,
    22385, 24623, 27086, 29794, 32767,
];
const INDEX: [i32; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];
#[derive(Default)]
pub struct Decoder {
    pub bytes: Vec<u8>,
    pub output: Vec<Vec<f32>>,
}

fn decode(bytes: &[u8], channels: usize, output: &mut [Vec<f32>]) {
    let blocks = bytes.len() / (36 * channels);
    for dest in output.iter_mut() {
        dest.resize(blocks * 64, 0.0);
    }
    for block in 0..blocks {
        let base = block * 36 * channels;
        for channel in 0..channels {
            let header = base + 4 * channel;
            let mut sample = i16::from_le_bytes([bytes[header], bytes[header + 1]]) as i32;
            let mut index = bytes[header + 2].min(88) as i32;
            let dest = &mut output[channel];
            dest[block * 64] = sample as f32 / 32768.0;
            for frame in 1..64 {
                let code_index = frame - 1;
                let group = code_index / 8;
                let byte = bytes[base + 4 * channels + (group * channels + channel) * 4 + code_index % 8 / 2];
                let code = if code_index % 2 == 0 { byte & 15 } else { byte >> 4 };
                let step = STEPS[index as usize];
                let mut diff = step >> 3;
                if code & 1 != 0 {
                    diff += step >> 2;
                }
                if code & 2 != 0 {
                    diff += step >> 1;
                }
                if code & 4 != 0 {
                    diff += step;
                }
                sample = (sample + if code & 8 != 0 { -diff } else { diff }).clamp(-32768, 32767);
                index = (index + INDEX[(code & 7) as usize]).clamp(0, 88);
                dest[block * 64 + frame] = sample as f32 / 32768.0;
            }
        }
    }
}
impl Decoder {
    pub fn decode(&mut self, channels: usize, count: usize, interleave: usize) -> usize {
        assert!(channels > 0 && count > 0 && channels * count <= 12);
        self.output.resize_with(channels * count, Vec::new);
        if count == 1 {
            decode(&self.bytes, channels, &mut self.output);
        } else {
            assert!(interleave > 0);
            let chunks = self.bytes.len() / interleave;
            let mut parts = vec![Vec::new(); count];
            for chunk in 0..chunks {
                parts[chunk % count].extend_from_slice(&self.bytes[chunk * interleave..(chunk + 1) * interleave]);
            }
            for (part, part_bytes) in parts.iter().enumerate() {
                decode(part_bytes, channels, &mut self.output[part * channels..(part + 1) * channels]);
            }
        }
        self.output[0].len()
    }
}
