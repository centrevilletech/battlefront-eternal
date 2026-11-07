//! AI writes only the intent fields touched at that source observation. Body
//! and seat owners have different intent types and retain their other edges.
use crate::{gen::Intent, seats::lease::SeatIntent};
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    Exit,
    Use,
    Crouch,
    Prone,
    Jet(Option<[f64; 3]>),
    Analogs([f64; 4]),
    Dodge(f64),
    Buttons([f64;2]),
    Aim(Option<[f64;3]>),
}
impl Command {
    /// Input setters retain their source writes, but a repeated held value
    /// needs no mirror row. Match delta encoding's bit comparison so signed
    /// zero, NaN and absent vectors keep their existing publication behavior.
    pub fn changes_unit(self, i:&Intent)->bool {
        fn same(a:f64,b:f64)->bool {a.to_bits()==b.to_bits()}
        fn vector(a:Option<[f64;3]>,b:Option<[f64;3]>)->bool {
            match(a,b){(Some(a),Some(b))=>a.into_iter().zip(b).all(|(a,b)|same(a,b)),(None,None)=>true,_=>false}
        }
        !match self {
            Self::Buttons(v)=>same(i.buttons[0],v[0])&&same(i.buttons[1],v[1]),
            Self::Aim(v)=>vector(i.aim,v),
            Self::Exit=>i.exit,Self::Use=>i.r#use,Self::Crouch=>i.crouch,Self::Prone=>i.prone,
            Self::Jet(v)=>i.jump&&vector(i.jet_direction,v),
            Self::Analogs([f,r,t,p])=>same(i.forward,f)&&same(i.right,r)&&same(i.turn,t)&&same(i.pitch,p),
            Self::Dodge(v)=>i.jump&&same(i.right,if v<0. {-1.}else{1.})&&same(i.forward,0.),
        }
    }
    pub fn unit(self, intent: &mut Intent) {
        match self {
            Self::Buttons(value)=>intent.buttons=value,
            Self::Aim(value)=>intent.aim=value,
            Self::Exit => intent.exit = true,
            Self::Use => intent.r#use = true,
            Self::Crouch => intent.crouch = true,
            Self::Prone => intent.prone = true,
            Self::Jet(direction) => {
                intent.jump = true;
                intent.jet_direction = direction;
            }
            Self::Analogs([forward, right, turn, pitch]) => {
                intent.forward = forward;
                intent.right = right;
                intent.turn = turn;
                intent.pitch = pitch;
            }
            Self::Dodge(value) => {
                intent.jump = true;
                intent.right = if value < 0. { -1. } else { 1. };
                intent.forward = 0.;
            }
        }
    }
    pub fn seat(self, intent: &mut SeatIntent) {
        match self {
            Self::Buttons(value)=>intent.buttons=value,
            Self::Aim(value)=>intent.aim=value.map(|p|three::Vector3::new(p[0],p[1],p[2])),
            Self::Exit => intent.exit = true,
            Self::Use => intent.r#use = true,
            Self::Crouch => intent.crouch = true,
            Self::Prone => intent.prone = true,
            // SeatInput has no jet-direction reader. Keep its jump field;
            // craft actions and held/pressed weapon edges remain untouched.
            Self::Jet(_) => intent.jump = true,
            Self::Analogs([forward, right, turn, pitch]) => {
                intent.forward = forward;
                intent.right = right;
                intent.turn = turn;
                intent.pitch = pitch;
            }
            Self::Dodge(value) => {
                intent.jump = true;
                intent.right = if value < 0. { -1. } else { 1. };
                intent.forward = 0.;
            }
        }
    }
}
