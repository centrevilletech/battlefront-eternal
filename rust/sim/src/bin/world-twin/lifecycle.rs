//! Prepared lifecycle gates. Contexts are constructed on their owning thread;
//! only owned corpus values cross threads, never Context's Rc or World loans.
use super::*;
fn first_battle(frames:&[Value])->&[Value] {
    let battle=&frames[0]["battle"];
    let end=frames.iter().position(|row|&row["battle"]!=battle).unwrap_or(frames.len());
    &frames[..end]
}
pub(super) fn interleave(admission:&Value,frames:&[Value]) {
    let frames=first_battle(frames);let expected=fingerprints(admission,frames);
    let a=admit(admission);let b=admit(admission);
    // Both admissions reuse the corpus's same local strings and entity IDs.
    // A0/B0/A1/B1 exercises A/B/A without passing either root to the other.
    for (row,want) in frames.iter().zip(&expected) {
        assert_eq!(&fingerprint(&frame(&a,row)),want,"interleaved A");
        assert_eq!(&fingerprint(&frame(&b,row)),want,"interleaved B");
    }
    close(&a);close(&b);
}
pub(super) fn threads(admission:&Value,frames:&[Value]) {
    let frames=first_battle(frames);let expected=fingerprints(admission,frames);
    std::thread::scope(|scope| {
        let a=scope.spawn(||fingerprints(admission,frames));
        let b=scope.spawn(||fingerprints(admission,frames));
        assert_eq!(a.join().unwrap(),expected,"thread A");
        assert_eq!(b.join().unwrap(),expected,"thread B");
    });
}
pub(super) fn sequential(admission:&Value,frames:&[Value]) {
    let frames=first_battle(frames);let expected=fingerprints(admission,frames);
    // replay closes and checks resource counts on every iteration, including
    // the final one. Equal IDs in a fresh owner must not revive a dead handle.
    for _ in 0..20 {assert_eq!(fingerprints(admission,frames),expected);}
}
pub(super) fn close_twice(admission:&Value,frames:&[Value]) {
    let context=admit(admission);close(&context);
    if let Err(error)=context.dispose(){assert_eq!(error.kind(),ErrorKind::Disposed);}
    assert_eq!(context.world_replay_live_handles(),0);
    let error=context.world_step(input(&frames[0]["world"][0])).unwrap_err();
    assert_eq!(error.kind(),ErrorKind::Disposed);
}
pub(super) fn poison(admission:&Value,frames:&[Value]) {
    let context=admit(admission);
    assert!(context.world_replay_panic().is_err(),"panic must be caught inside the owning entry");
    let error=context.world_step(input(&frames[0]["world"][0])).unwrap_err();
    assert_eq!(error.kind(),ErrorKind::Poisoned);
    close(&context);
}
