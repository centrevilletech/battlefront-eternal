//! The recon gun notification's authority-side listener. F3Recon.fired in
//! src/sim/wasm/f3-recon.js calls Game's RemoteDroids.onFired callback with
//! the sampled position and immutable droid definition, not its final pose.
use super::capture::{Capture, Token};
use super::journal::Journal;
use crate::abi::context::Context;
use crate::gen::F3ReconShot;

/// A recon shot retains its class independently of the thrower's current
/// infantry class or the flying droid's subsequent lifecycle.
#[derive(Clone, Copy, Debug)]
pub struct Record {
    pub class: u32,
    pub channel: f64,
    pub position: [f64; 3],
}

/// attachPresentationCapture records the registered droid class, because
/// cloned droid gun classes have no independent combat-registry identity.
/// Presentation normalizes the channel while the journal keeps the raw one.
pub fn fired(
    context: &Context,
    journal: &mut Journal,
    capture: Option<&Capture>,
    tick: f64,
    class: u32,
    definition_id: f64,
    event: &F3ReconShot,
) -> Result<(), String> {
    // The renderer resolves the cold definition only when it fingerprints
    // the journal; retaining its class keeps this record heap-free.
    journal.emit(super::journal::Event::Recon(Record {
        class,
        channel: event.channel,
        position: event.position,
    }));
    // The journal and presentation intentionally use different shapes.
    // The wire sends a definition id and normalized channel; the journal
    // retains the full class and the unnormalized gun channel.
    if let Some(capture) = capture {
        capture.emit(
            context,
            tick,
            "remoteShot",
            0.0,
            &[
                ("definitionId", Token::Number(definition_id)),
                (
                    "channel",
                    Token::Number(if event.channel == 1.0 { 1.0 } else { 0.0 }),
                ),
                ("position", Token::Vector(Some(event.position))),
            ],
        )?;
    }
    Ok(())
}

/// Cold definitions are supplied by the admission owner as the same generated
/// F3ReconClass value stream used by F3Recon.prepare. Keeping their JSON once
/// avoids reconstructing a whole content object on each remote shot.
#[derive(Default)]
pub struct Definitions {
    values: Vec<Option<String>>,
}

impl Definitions {
    pub fn bind(&mut self, context: &Context, class: u32, words: &[f64]) -> Result<(), String> {
        // Admission is a cold boundary. Decode the authored value here,
        // before any tick can refer to this class. Binding a later class
        // must preserve earlier indices and their immutable definitions.
        let text = definition_json(context, words)?;
        self.values
            .resize_with(self.values.len().max(class as usize + 1), || None);
        self.values[class as usize] = Some(text);
        Ok(())
    }

    // Returned text stays owned by the host's cold definitions. A retained
    // event stores just the class index and never clones this object.
    pub fn get(&self, class: u32) -> Option<&str> {
        self.values.get(class as usize)?.as_deref()
    }
}

/// valueWords in weapon-content-sim.js emits tags and ordered own keys. A
/// recon class is sent without the optional weapon-reference table, so every
/// nested gun is a complete value, including its class and charge levels.
pub fn definition_json(context: &Context, words: &[f64]) -> Result<String, String> {
    fn next(words: &[f64], at: &mut usize) -> Result<f64, String> {
        let word = words
            .get(*at)
            .copied()
            .ok_or("truncated recon definition")?;
        *at += 1;
        Ok(word)
    }
    fn text(context: &Context, words: &[f64], at: &mut usize) -> Result<String, String> {
        let units = context
            .string(crate::store::StrId(next(words, at)? as u32))
            .map_err(|e| e.message().to_string())?;
        serde_json::to_string(&String::from_utf16_lossy(&units)).map_err(|e| e.to_string())
    }
    fn value(
        context: &Context,
        words: &[f64],
        at: &mut usize,
        out: &mut String,
    ) -> Result<(), String> {
        // valueWords uses the original ordered own-key list. Build its JSON
        // directly at this cold boundary rather than sorting through a map.
        match next(words, at)? as u32 {
            0 | 1 | 9 => out.push_str("null"),
            2 => out.push_str("false"),
            3 => out.push_str("true"),
            4 => out.push_str(&jsrt::numfmt::json_number(next(words, at)?)),
            5 => out.push_str(&text(context, words, at)?),
            6 => {
                let count = next(words, at)? as usize;
                out.push('[');
                for i in 0..count {
                    if i != 0 {
                        out.push(',');
                    }
                    value(context, words, at, out)?;
                }
                out.push(']');
            }
            7 => {
                let count = next(words, at)? as usize;
                out.push('{');
                let mut comma = false;
                for _ in 0..count {
                    let key = text(context, words, at)?;
                    // JSON.stringify omits undefined own properties, while
                    // an undefined array element becomes null above.
                    if matches!(words.get(*at), Some(0.0 | 9.0)) {
                        *at += 1;
                        continue;
                    }
                    if comma {
                        out.push(',');
                    }
                    comma = true;
                    out.push_str(&key);
                    out.push(':');
                    value(context, words, at, out)?;
                }
                out.push('}');
            }
            _ => return Err("unsupported recon definition value tag".into()),
        }
        Ok(())
    }
    // Definitions are decoded only at admission. No per-tick serializer or
    // string-keyed content lookup belongs in the recon flight phase.
    // Reject trailing data so a whole generated row cannot accidentally
    // masquerade as the class value stream that this boundary expects.
    let mut out = String::new();
    let mut at = 0;
    value(context, words, &mut at, &mut out)?;
    if at != words.len() {
        return Err("trailing recon definition words".into());
    }
    Ok(out)
}

/// Lifecycle and pose notices publish canonical recon rows before delivery.
/// Their JavaScript callbacks only maintained read facades; a native host
/// must not apply those poses or lifecycle transitions a second time.
pub struct Notifications<'a> {
    pub definitions: &'a Definitions,
    pub context: &'a Context,
    pub journal: &'a mut Journal,
    pub capture: Option<&'a Capture>,
    pub store_source: u32,
    pub tick: f64,
}

impl Notifications<'_> {
    pub fn handle(&mut self, code: u32, record: &[f64]) -> Result<bool, String> {
        use crate::gen;
        // The canonical recon owner already applied context rebinds,
        // piloting, camera updates, retirements and retained launch lives.
        // Only the fired callback has an extra authority-side observer.
        match code {
            gen::EVENT_F3_RECON_CONTEXT_CREATED
            | gen::EVENT_F3_RECON_DROID_CREATED
            | gen::EVENT_F3_RECON_CONTEXT
            | gen::EVENT_F3_RECON_DROID
            | gen::EVENT_F3_RECON_RETIRED => Ok(true),
            gen::EVENT_F3_RECON_SHOT if record.len() >= F3ReconShot::WORDS => {
                let e =
                    F3ReconShot::take(&record[..F3ReconShot::WORDS], &record[F3ReconShot::WORDS..]);
                // The sampled class id survives an owner taking another
                // infantry class. Resolve the definition from cold storage,
                // never from the thrower's current combat arsenal.
                // Flying droids retain their launch class through owner
                // respawns. A missing cold binding is an admission error;
                // replacing it with the current infantry class is incorrect.
                let _definition = self
                    .definitions
                    .get(e.class)
                    .ok_or("recon shot has no bound cold definition")?;
                let id = if self.capture.is_some() {
                    self.context
                        .wire_remote_id(self.store_source, e.class)
                        .map_err(|e| e.message().to_string())?
                } else {
                    0.0
                };
                fired(
                    self.context,
                    self.journal,
                    self.capture,
                    self.tick,
                    e.class,
                    id,
                    &e,
                )?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}

impl Context {
    /// Read a retained journal together with its immutable recon definitions.
    /// One host borrow avoids re-entering the Context while observing a record.
    pub fn world_host_journal_with_recon<T>(
        &self,
        read: impl FnOnce(&Journal, &Definitions) -> T,
    ) -> Result<T, crate::abi::context::Error> {
        self.host(|host| read(&host.journal, &host.recon_definitions))
    }

    /// Admission supplies the generated F3ReconClass's encoded value once.
    /// Native admission and recorded admission share this content boundary.
    pub fn world_host_recon_class(
        &self,
        class: u32,
        words: &[f64],
    ) -> Result<(), crate::abi::context::Error> {
        self.host(|host| host.recon_definitions.bind(self, class, words))?
            .map_err(crate::abi::context::Error::host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shot_without_network_capture_still_reaches_the_journal() {
        let context = Context::new();
        let mut journal = Journal::new();
        journal.time = 0.5;
        fired(
            &context,
            &mut journal,
            None,
            30.0,
            2,
            7.0,
            &F3ReconShot::default(),
        )
        .unwrap();
        let record = journal.records().next().unwrap();
        assert_eq!(record.event.kind(), "remoteShot");
        assert_eq!(record.time, 0.5);
        assert!(matches!(
            record.event,
            super::super::journal::Event::Recon(Record {
                class: 2,
                channel: 0.0,
                position: [0.0, 0.0, 0.0]
            })
        ));
    }
}

#[cfg(test)]
mod definition_tests {
    use super::*;

    #[test]
    fn cold_value_stream_keeps_key_order_and_array_holes() {
        // The fixture separates an omitted own property from an undefined
        // array element and a non-finite number. JSON.stringify omits the
        // first and writes null for the latter two, retaining key order.
        let context = Context::new();
        let id = |s: &str| {
            context
                .intern(&s.encode_utf16().collect::<Vec<_>>())
                .unwrap()
                .0 as f64
        };
        let mut defs = Definitions::default();
        let words = [
            7.0,
            3.0,
            id("class"),
            5.0,
            id("scout"),
            id("absent"),
            0.0,
            id("guns"),
            6.0,
            3.0,
            4.0,
            1.0,
            0.0,
            4.0,
            f64::NAN,
        ];
        defs.bind(&context, 2, &words).unwrap();
        assert_eq!(
            defs.get(2),
            Some(r#"{"class":"scout","guns":[1,null,null]}"#)
        );
        assert!(defs.get(1).is_none());
        assert!(definition_json(&context, &[7.0, 1.0]).is_err());
    }

    #[test]
    fn context_notice_does_not_duplicate_native_lifecycle() {
        // The oracle emits its first empty-context publication at tick 1.
        // Its callback only updates facades, so no journal event or wire
        // capture allocation is correct for the native authority.
        let context = Context::new();
        let defs = Definitions::default();
        let mut journal = Journal::new();
        let mut notices = Notifications {
            definitions: &defs,
            context: &context,
            journal: &mut journal,
            capture: None,
            store_source: 0,
            tick: 1.0,
        };
        assert!(notices
            .handle(crate::gen::EVENT_F3_RECON_CONTEXT, &[])
            .unwrap());
        assert_eq!(journal.seq, 0);
    }
}
