//! Mailboxes publish held bits in their admitted registry's wire order.
use bfe_sim::{gen::*,input::owner::Mailbox,store::{Strings,StrId}};
#[test]
fn registered_code_order_controls_publication_and_unknown_codes_are_refused() {
    let mut strings=Strings::default();let mouse=strings.intern_str("Mouse0");let key=strings.intern_str("KeyW");
    let mut mailbox=Mailbox::new(1.,4.,vec![mouse,key],&mut strings).unwrap();
    let input=ImInput {op:ImOperation::Push,epoch_matches:true,entry:Some(ImEntry {sequence:1.,tick:1.,generation:2.,codes:vec![mouse],..Default::default()}),..Default::default()};
    assert!(mailbox.run(input,&mut strings).accepted);
    mailbox.run(ImInput {op:ImOperation::Take,tick:1.,generation:2.,..Default::default()},&mut strings);
    assert_eq!(mailbox.publication.held.0[0],1);assert!(mailbox.publication.held_valid);
    let unknown=strings.intern_str("KeyS");
    assert!(mailbox.run(ImInput {op:ImOperation::Push,epoch_matches:true,entry:Some(ImEntry {sequence:2.,codes:vec![unknown],..Default::default()}),..Default::default()},&mut strings).error.is_some());
    assert_eq!(mailbox.state.received,1.);assert!(mailbox.state.queue.is_empty());
    let long=strings.intern_str(&"x".repeat(65));
    for codes in [vec![mouse,mouse],vec![long],vec![StrId(u32::MAX)]] {
        assert!(Mailbox::new(1.,4.,codes,&mut strings).is_none());
    }
}
