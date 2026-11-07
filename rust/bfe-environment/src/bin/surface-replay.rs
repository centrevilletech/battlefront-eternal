//! Replay the packed surface frames emitted by tools/surface-replay.mjs.
//! Captures contain numeric fixtures, collision query replies and final output
//! bits. No renderer, page object or map format is needed by this native host.
use bfe_environment::{surfaces::Phase, surface_math::V8, shore::Queries, layout::{surface_frame as frame, surface_command as command, surface_result as out, surface as lim}};
use std::{env, fs};
struct Reader { bytes: Vec<u8>, at: usize }
impl Reader {
    fn u32(&mut self) -> usize { let v=u32::from_le_bytes(self.bytes[self.at..self.at+4].try_into().unwrap());self.at+=4;v as usize }
    fn f64(&mut self) -> f64 { let v=f64::from_le_bytes(self.bytes[self.at..self.at+8].try_into().unwrap());self.at+=8;v }
    fn doubles(&mut self, out: &mut [f64]) { for v in out { *v=self.f64(); } }
    fn check_f64(&mut self, values: &[f64]) { for (i,value) in values.iter().enumerate() { let expected=self.f64();assert_eq!(value.to_bits(),expected.to_bits(),"result {i}"); } }
    fn check_f32(&mut self, values: &[f32]) { for (i,value) in values.iter().enumerate() { let expected=u32::from_le_bytes(self.bytes[self.at..self.at+4].try_into().unwrap());self.at+=4;assert_eq!(value.to_bits(),expected,"draw {i}"); } }
}
struct Tape { tiles: Vec<Vec<f64>>, exposed: Vec<bool>, tile: usize, probe: usize }
impl Queries for Tape {
    fn triangles(&mut self, _:f64,_:f64,_:f64,output:&mut [f64])->usize {let data=&self.tiles[self.tile];self.tile+=1;output[..data.len()].copy_from_slice(data);data.len()/9}
    fn exposed(&mut self,_:f64,_:f64,_:f64)->bool {let answer=self.exposed[self.probe];self.probe+=1;answer}
}
fn main() {
    let mut input=Reader {bytes:fs::read(env::args().nth(1).expect("surface-replay CAPTURE")).unwrap(),at:0};
    assert_eq!(input.u32(),0x31434653);let frames=input.u32();let mut phase=Phase::new(true);input.doubles(&mut phase.config);
    for index in 0..frames {
        input.doubles(&mut phase.frame);
        let commands=(phase.frame[frame::COMMANDS] as usize).min(phase.commands.len()/command::STRIDE);
        let start=(phase.frame[frame::COMMAND_START] as usize).min(lim::COMMANDS-1);
        for i in 0..commands {let at=(start+i)%lim::COMMANDS*command::STRIDE;input.doubles(&mut phase.commands[at..at+command::STRIDE]);}
        let tile_count=input.u32();let mut tiles=Vec::with_capacity(tile_count);
        for _ in 0..tile_count {let n=input.u32();let mut data=vec![0.0;n*9];input.doubles(&mut data);tiles.push(data);}
        let n=input.u32();let mut exposed=Vec::with_capacity(n);for _ in 0..n {exposed.push(input.u32()!=0);}
        let mut tape=Tape {tiles,exposed,tile:0,probe:0};phase.update::<V8,_>(&mut tape);
        assert_eq!(tape.tile,tape.tiles.len(),"frame {index} tile requests");assert_eq!(tape.probe,tape.exposed.len(),"frame {index} probes");
        input.check_f64(&phase.result);let vertices=phase.result[out::QUADS] as usize*4;
        input.check_f32(&phase.positions[..vertices*3]);input.check_f32(&phase.uvs[..vertices*2]);input.check_f32(&phase.alpha[..vertices]);
        input.check_f32(&phase.shore.points[..phase.shore.count*4]);input.check_f32(&phase.shore.phases[..phase.shore.count]);
        let n=phase.storm.count;input.check_f32(&phase.storm.positions[..n*3]);input.check_f32(&phase.storm.tangents[..n*3]);
        input.check_f32(&phase.storm.edges[..n]);input.check_f32(&phase.storm.widths[..n]);input.check_f32(&phase.storm.energies[..n]);
    }
    assert_eq!(input.at,input.bytes.len());println!("{frames} native surface frames exact");
}
