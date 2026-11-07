//! Exercise the same generated push/take exchange used by native and WASM
//! hosts. The allocator includes decoding, retirement and result encoding.
use bfe_sim::{gen::*,input::owner::Mailbox,store::{calls::Calls,Strings}};
#[path="support/allocations.rs"] mod allocations;
#[global_allocator] static ALLOCATOR:allocations::Counted=allocations::Counted;

fn send(owner:&mut Mailbox,calls:&mut Calls,strings:&mut Strings,call:&InputMailboxCall) {
    calls.result(InputMailboxCall::WORDS,|row,heap|call.put(row,heap));
    assert!(owner.exchange(calls,strings).is_some());
}
fn fixture(capacity:f64)->(Mailbox,Calls,Strings,InputMailboxCall) {
    let mut strings=Strings::default();let code=strings.intern_str("KeyW");
    let action=strings.intern_str("jump");let field=strings.intern_str("aspect");
    let owner=Mailbox::new(1.,capacity,vec![code],&mut strings).unwrap();
    let call=InputMailboxCall {op:ImOperation::Push,epoch_matches:true,entry:Some(ImEntry {
        identity:8.,down:8.,sequence:1.,tick:1.,generation:1.,codes:vec![code;64],actions:vec![action;2048],
        look:vec![ImLook {point:[-0.,0.5],object:Some(9.)};2048],
        view:vec![ImField {key:field,value:8.};7],human:vec![HsCommand {kind:HsCommandKind::Suicide,..Default::default()};16],
        fire:vec![ImFire {id:1.,button:0.,pressed:true};16],
        publication:WiView {flags:64.,aspect:1.5,..Default::default()},..Default::default()
    }),..Default::default()};
    (owner,Calls::new(),strings,call)
}
fn cycle(owner:&mut Mailbox,calls:&mut Calls,strings:&mut Strings,push:&mut InputMailboxCall,tick:f64) {
    let entry=push.entry.as_mut().unwrap();entry.sequence=tick;entry.tick=tick;
    send(owner,calls,strings,push);assert_eq!(owner.state.queue.len(),1);
    assert_eq!(&owner.state.queue[0],push.entry.as_ref().unwrap());
    let take=InputMailboxCall {op:ImOperation::Take,tick,generation:1.,..Default::default()};
    send(owner,calls,strings,&take);
    assert!(owner.state.queue.is_empty());assert_eq!(owner.state.ack,tick);
    assert_eq!(owner.taken().actions.len(),2048);assert_eq!(owner.taken().look.len(),2048);
    assert_eq!(owner.taken().human.len(),16);assert_eq!(owner.taken().fire.len(),16);
    assert_eq!(owner.down().len(),64);assert_eq!(owner.publication.view.aspect,1.5);
    assert_eq!(owner.taken().look[0].point[0].to_bits(),(-0.0_f64).to_bits());
}

#[test]
fn ten_thousand_generated_push_take_exchanges_allocate_nothing_after_warmup() {
    let (mut owner,mut calls,mut strings,mut push)=fixture(4.);
    // Warm the shared exchange to the largest encoded input/result. Packet
    // banks themselves were reserved before the connection could receive input.
    cycle(&mut owner,&mut calls,&mut strings,&mut push,1.);
    allocations::start();
    for tick in 2..=10_001 {cycle(&mut owner,&mut calls,&mut strings,&mut push,tick as f64);}
    let count=allocations::stop();assert_eq!(count,0);
}

#[test]
fn full_bursts_refusals_clear_and_old_lives_return_the_admitted_banks() {
    let (mut owner,mut calls,mut strings,mut push)=fixture(4.);
    // Each queued packet has the maximum legal lists. Takes defer later
    // packets when the combined action/look result would exceed its bounds.
    for sequence in 1..=4 {
        push.entry.as_mut().unwrap().sequence=sequence as f64;
        send(&mut owner,&mut calls,&mut strings,&push);
    }
    assert_eq!(owner.state.queue.len(),4);
    push.entry.as_mut().unwrap().sequence=5.;
    send(&mut owner,&mut calls,&mut strings,&push);
    assert_eq!(owner.rejected,1);assert_eq!(owner.state.received,4.);
    assert_eq!(owner.state.queue.len(),4);
    for remaining in (0..4).rev() {
        send(&mut owner,&mut calls,&mut strings,&InputMailboxCall {op:ImOperation::Take,tick:1.,generation:1.,..Default::default()});
        assert_eq!(owner.state.queue.len(),remaining);
    }
    // A stale sequence and a malformed projected value must return their
    // decoder bank too, rather than gradually exhausting a connection.
    send(&mut owner,&mut calls,&mut strings,&push);
    send(&mut owner,&mut calls,&mut strings,&push);
    push.entry.as_mut().unwrap().sequence=6.;push.entry.as_mut().unwrap().tick=0.;
    send(&mut owner,&mut calls,&mut strings,&push);
    assert_eq!(owner.state.received,5.);
    send(&mut owner,&mut calls,&mut strings,&InputMailboxCall {op:ImOperation::Clear,..Default::default()});
    push.entry.as_mut().unwrap().tick=1.;push.entry.as_mut().unwrap().sequence=6.;
    send(&mut owner,&mut calls,&mut strings,&push);
    send(&mut owner,&mut calls,&mut strings,&InputMailboxCall {op:ImOperation::Take,tick:1.,generation:2.,..Default::default()});
    assert_eq!(owner.state.discarded,6.);assert!(owner.state.queue.is_empty());
    push.entry.as_mut().unwrap().generation=2.;
    allocations::start();
    for batch in 0..100 {
        for slot in 0..4 {
            push.entry.as_mut().unwrap().sequence=(7+batch*4+slot) as f64;
            send(&mut owner,&mut calls,&mut strings,&push);
        }
        assert_eq!(owner.state.queue.len(),4);
        // Exhaustion refuses before decoding; its warmed error result reuses
        // the exchange without touching the four retained queued records.
        send(&mut owner,&mut calls,&mut strings,&push);
        send(&mut owner,&mut calls,&mut strings,&InputMailboxCall {op:ImOperation::Clear,..Default::default()});
        assert!(owner.state.queue.is_empty());
    }
    let count=allocations::stop();assert_eq!(count,0);
}

#[test]
fn native_phase_takes_and_host_pushes_share_the_same_banks() {
    let (mut owner,mut calls,mut strings,mut push)=fixture(2.);
    send(&mut owner,&mut calls,&mut strings,&push);
    assert!(owner.take_command(1.,1.,&mut strings));
    // The next exchange also returns the producer identities retained by the
    // players' phase. Include that output before starting the frame counter.
    cycle(&mut owner,&mut calls,&mut strings,&mut push,2.);
    push.entry.as_mut().unwrap().tick=0.;
    send(&mut owner,&mut calls,&mut strings,&push);
    allocations::start();
    for tick in 3..=10_002 {
        let entry=push.entry.as_mut().unwrap();entry.sequence=tick as f64;entry.tick=tick as f64;
        send(&mut owner,&mut calls,&mut strings,&push);
        assert!(owner.take_command(tick as f64,1.,&mut strings));
        assert_eq!(owner.state.ack,tick as f64);assert!(owner.state.queue.is_empty());
        entry_refusal(&mut owner,&mut calls,&mut strings,&mut push);
    }
    let count=allocations::stop();assert_eq!(count,0);
}
fn entry_refusal(owner:&mut Mailbox,calls:&mut Calls,strings:&mut Strings,push:&mut InputMailboxCall) {
    push.entry.as_mut().unwrap().tick=0.;send(owner,calls,strings,push);
    assert!(owner.state.queue.is_empty());
}
