//! Context consumers of the existing callback-free input cores. Protocol
//! validation errors remain in each generated result, including partial
//! state; the outer Result reports context lifetime errors only.
use super::{Context, Error, Record};
use crate::gen::*;
use crate::input::{actions, controls, fire, mailbox};

impl Context {
    pub fn input_actions(&self, input: IaInput) -> Result<IaResult, Error> {
        let _flight = self.enter()?;
        let out = self.store(|s| actions::run(input, &mut s.strings))?;
        self.publish(&Record::new(IaResult::WORDS, |r, h| out.put(r, h)))?;
        Ok(out)
    }

    pub fn input_fire(&self, input: IfInput) -> Result<IfResult, Error> {
        let _flight = self.enter()?;
        let out = self.store(|s| fire::run(input, &mut s.strings))?;
        self.publish(&Record::new(IfResult::WORDS, |r, h| out.put(r, h)))?;
        Ok(out)
    }

    pub fn input_mailbox(&self, input: ImInput) -> Result<ImResult, Error> {
        let _flight = self.enter()?;
        let out = self.store(|s| mailbox::run(input, &mut s.strings))?;
        self.publish(&Record::new(ImResult::WORDS, |r, h| out.put(r, h)))?;
        Ok(out)
    }

    pub fn input_queue(&self, input: IqInput) -> Result<IqResult, Error> {
        let _flight = self.enter()?;
        let out = self.store(|s| controls::queue(input, &mut s.strings))?;
        self.publish(&Record::new(IqResult::WORDS, |r, h| out.put(r, h)))?;
        Ok(out)
    }

    /// Admission and clearing only, as in input_controls_run. Iterating a
    /// host command's opaque down/actions values remains the host's job.
    pub fn input_controls(&self, input: IcInput) -> Result<IcResult, Error> {
        let _flight = self.enter()?;
        let out = controls::controls(input);
        self.publish(&Record::new(IcResult::WORDS, |r, h| out.put(r, h)))?;
        Ok(out)
    }

    /// Like the WASM scalar export, this leaves the result block alone.
    pub fn input_axis(&self, input: IkInput) -> Result<f64, Error> {
        let _flight = self.enter()?;
        Ok(controls::axis(&input.down, &input.plus, &input.minus))
    }
}
