//! Sight client preparation keeps storage generations separate from the
//! gameplay life that service validates after obtaining current requests.
use bfe_sim::ai::{owned::{controllers::Controllers,entities::Entities},combat::perception::{DeferredLos,GLOBAL_CAPACITY}};
#[test]
fn sight_clients_deduplicate_bursts_and_reject_reused_controller_slots() {
    let mut entities=Entities::new(16);let mut controllers=Controllers::new(3,1);
    let first=controllers.adopt(9,&mut entities);let other=controllers.adopt(5000,&mut entities);
    let mut los=DeferredLos::default();
    for i in 0..400 {los.submit(if i%2==0 {first}else{other},if i%3==0 {99.}else{1.},None,100.);}
    assert_eq!(los.active.count,GLOBAL_CAPACITY);
    let mut clients=[usize::MAX;GLOBAL_CAPACITY];let count=controllers.los_clients(&entities,&los,&mut clients);
    assert_eq!(count,2);assert!(clients[..count].contains(&9));assert!(clients[..count].contains(&5000));
    // A stale gameplay life remains a client: service must first read the new
    // controller life, then reject that request, rather than retain old input.
    assert_eq!(controllers.los_clients(&entities,&los,&mut [0;1]),1);
    assert_eq!(controllers.los_clients(&entities,&los,&mut []),0);
    assert!(controllers.retire(&mut entities,first));let replacement=controllers.adopt(12,&mut entities);
    assert_ne!(replacement,first);let count=controllers.los_clients(&entities,&los,&mut clients);
    assert_eq!(&clients[..count],&[5000]);
    los.submit(replacement,2.,None,200.);let count=controllers.los_clients(&entities,&los,&mut clients);
    assert_eq!(count,2);assert!(clients[..count].contains(&12));assert!(clients[..count].contains(&5000));
}
