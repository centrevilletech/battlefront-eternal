//! Regression for attachPresentationCapture's fault guard: a rejected
//! cosmetic payload must leave the capture usable for later events.
#[cfg(test)]
mod tests {
    use super::super::capture::Capture;
    use super::super::capture::Token;
    use crate::abi::context::Context;
    use crate::gen::WbCall;
    use crate::world::admit::{Call, Returned};

    fn handle(context: &Context, name: &str) -> u32 {
        match context.invoke_export(name, &Call::default()).unwrap() {
            Returned::Scalar(value) => value as u32,
            _ => panic!("capture handle"),
        }
    }

    #[test]
    fn invalid_visual_records_a_fault_and_the_next_event_still_commits() {
        let context = Context::new();
        let capture = Capture {
            combat: handle(&context, "wire_combat_create"),
            presentation: handle(&context, "wire_presentation_create"),
            store_source: 0,
            explosions: Vec::new(),
        };
        // An absent action fails the same text validation as the page.
        // It must not poison or stop the authority that owns this capture.
        assert!(!capture
            .emit(
                &context,
                1.0,
                "weaponAction",
                0.0,
                &[("action", Token::Undefined)]
            )
            .unwrap());
        let faults = capture
            .run(
                &context,
                WbCall {
                    op: 8.0,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(faults.faults.len(), 1);
        let text = context.string(faults.faults[0]).unwrap();
        assert_eq!(String::from_utf16_lossy(&text), "Invalid presentation text");
        assert!(capture.emit(&context, 2.0, "shot", 0.0, &[]).unwrap());
        let faults = capture
            .run(
                &context,
                WbCall {
                    op: 8.0,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(faults.faults.len(), 1);
    }
}
