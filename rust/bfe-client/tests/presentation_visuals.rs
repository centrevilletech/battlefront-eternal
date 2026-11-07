//! Native session commands borrow the same numeric kernels as the browser.
//! The recorded random service remains command data, not a second renderer.
use bfe_client::presentation_bridge::{PageSniper,PageSniperCommand};
use bfe_presentation::sniper_layout::{sniper_frame as f,sniper_command as c,sniper_kind as k,sniper_counts as h};
#[test]
fn sniper_session_uses_the_recorded_random_service(){
 let mut trace=PageSniper::new();
 let input=unsafe{std::slice::from_raw_parts_mut(trace.input_ptr()as *mut f64,trace.input_len())};
 input[f::TIME]=10.0;input[f::EYE+1]=1e6;input[f::BEGIN]=1.0;input[f::COMMANDS]=1.0;
 let at=f::STRIDE;input[at+c::KIND]=k::TOUCH as f64;input[at+c::SOURCE]=1.0;
 input[at+c::DIRECTION+2]=1.0;input[at+c::HEAD+2]=40.0;input[at+c::REACH]=300.0;input[at+c::COLOR]=255.0;
 PageSniperCommand::collect(vec![0.5;23]).execute(&mut trace).unwrap();
 PageSniperCommand::frame(Vec::new()).execute(&mut trace).unwrap();
 let counts=unsafe{std::slice::from_raw_parts(trace.counts_ptr(),trace.counts_len())};
 assert_eq!(counts[h::RECORDS],1);assert_eq!(counts[h::STREAKS],8);assert_eq!(counts[h::FLARES],1);
}
