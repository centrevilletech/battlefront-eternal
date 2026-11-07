//! Diagnostic Context operations, absent from packaged addons. These inspect
//! actual strings, generated Store records and active native invocations;
//! they do not pretend the foundation Context has a complete native battle.
use super::*;
use bfe_sim::abi::context::{ErrorKind, Record};
use bfe_sim::gen::{Bolt, Fill, COMMAND_FILL, KIND_BOLT};
use bfe_sim::store::{block::Block, StrId};
use napi::bindgen_prelude::Uint32Array;
use napi::JsObject;
use std::sync::atomic::AtomicU32;
use std::time::{Duration, Instant};

#[napi(object)]
pub struct Identity {
    pub string_id: u32,
    pub slot: u32,
    pub generation: u32,
    pub speed: f64,
}

#[napi]
impl NativeHost {
    #[napi]
    pub fn probe_failure(&self, acquisition: u32) -> Result<NativeOwner> {
        if !(1..=4).contains(&acquisition) { return Err(Error::from_reason("Unknown acquisition")); }
        self.create_owner(None, Some(acquisition))
    }
}

#[napi]
impl NativeOwner {
    // Copy into a caller-owned reusable arena. JS owns both buffers, and Rust
    // retains neither. Byte-preserving copies keep signed zero and NaN payloads.
    // This operation validates the common header, not schema-specific records.
    #[napi]
    pub fn copy_block(&self, env: Env, source: JsObject, output: JsObject) -> Result<u32> {
        boundary(|| {
            if self.owner.borrow().context.is_none() {return Err(Error::from_reason("Native owner closed"));}
            blocks::copy(env, source, output)
        })
    }

    #[napi]
    pub fn probe_identity(&self, text: String, speed: f64) -> Result<Identity> {
        boundary(|| {
            let owner = self.owner.borrow();
            let cx = owner.context.as_ref().ok_or_else(|| Error::from_reason("Native owner closed"))?;
            let error = |e: bfe_sim::abi::context::Error| Error::from_reason(e.to_string());
            let string_id = cx.intern(&text.encode_utf16().collect::<Vec<_>>()).map_err(error)?;
            cx.command(COMMAND_FILL, Record::new(Fill::WORDS, |row, heap| {
                Fill { kind: KIND_BOLT as u32, count: 1 }.put(row, heap);
            })).map_err(|e| error(e.error))?;
            let words = cx.export(KIND_BOLT as u32, 1).map_err(error)?;
            let block = Block { words, heap: Vec::new() };
            let (source, _) = block.read(0);
            // The shared Store row prefix precedes every generated record.
            const SLOT: usize = 0;
            const GENERATION: usize = 1;
            let slot = source[SLOT] as u32;
            let generation = source[GENERATION] as u32;
            let mut input = Block::default();
            input.begin(1, Bolt::FULL_STRIDE);
            let (row, heap) = input.row(0);
            row[SLOT] = slot as f64; row[GENERATION] = generation as f64;
            Bolt { speed, ..Default::default() }.put_full(row, heap);
            input.finish(); cx.import(KIND_BOLT as u32, 1, input).map_err(error)?;
            Ok(Identity { string_id: string_id.0, slot, generation, speed: self.probe_speed()? })
        })
    }
    #[napi]
    pub fn probe_read(&self, string_id: u32) -> Result<String> {
        boundary(|| {
            let owner = self.owner.borrow();
            let cx = owner.context.as_ref().ok_or_else(|| Error::from_reason("Native owner closed"))?;
            let units = cx.string(StrId(string_id)).map_err(|e| Error::from_reason(e.to_string()))?;
            Ok(String::from_utf16_lossy(&units))
        })
    }
    #[napi]
    pub fn probe_speed(&self) -> Result<f64> {
        boundary(|| {
            let owner = self.owner.borrow();
            let cx = owner.context.as_ref().ok_or_else(|| Error::from_reason("Native owner closed"))?;
            let words = cx.export(KIND_BOLT as u32, 1).map_err(|e| Error::from_reason(e.to_string()))?;
            let block = Block { words, heap: Vec::new() };let (row, heap) = block.read(0);
            let mut bolt = Bolt::default();bolt.take_full(row, heap);Ok(bolt.speed)
        })
    }
    #[napi]
    pub fn probe_work(&self, signal: Uint32Array, milliseconds: u32) -> Result<()> {
        boundary(|| {
            if signal.len() < 2 || milliseconds == 0 || milliseconds > 5000 {
                return Err(Error::from_reason("Invalid active-call fixture"));
            }
            let owner = self.owner.borrow();
            let cx = owner.context.as_ref().ok_or_else(|| Error::from_reason("Native owner closed"))?;
            // N-API retains the typed arena for this synchronous invocation.
            // Only atomic accesses touch its shared words while JS observes it.
            let address = signal.as_ptr() as usize;
            const IMPORT: u32 = 0x7fff_ff01;
            cx.register(IMPORT, move |context, _| {
                assert_eq!(context.dispose().unwrap_err().kind(), ErrorKind::Busy);
                let started = unsafe { &*(address as *const AtomicU32) };
                let release = unsafe { &*((address as *const AtomicU32).add(1)) };
                started.store(1, Ordering::SeqCst);
                let deadline = Instant::now() + Duration::from_millis(milliseconds as u64);
                while release.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
                    std::hint::spin_loop();
                }
                started.store(2, Ordering::SeqCst);
                Ok(Record::default())
            }).map_err(|e| Error::from_reason(e.to_string()))?;
            // Even an unexpected Rust panic removes the pointer-bearing hook
            // before the N-API arena can be released.
            struct Hook<'a>(&'a Context, u32);
            impl Drop for Hook<'_> { fn drop(&mut self) { let _ = self.0.unregister(self.1); } }
            let _hook = Hook(cx, IMPORT);
            cx.call_import(IMPORT, Record::default()).map_err(|e| Error::from_reason(e.error.to_string()))?;
            Ok(())
        })
    }
}

// Instrument only real registrations; no Context or hook is retained here.
#[napi]
pub fn cleanup_hook_counts() -> Vec<u32> {
    vec![HOOKS_CREATED.load(Ordering::SeqCst), HOOKS_RELEASED.load(Ordering::SeqCst)]
}
#[napi]
impl NativeOwner {
    #[napi]
    pub fn probe_operation_panic(&self) -> Result<()> {
        self.invoke(|_| panic!("operation fixture"))
    }
    #[napi]
    pub fn probe_close_panic(&self) {self.owner.borrow_mut().panic_on_close = true;}
}

// Caller-owned diagnostic storage avoids allocating a result Vec while an
// allocation census observes cleanup. Counters never retain Context handles.
#[napi]
pub fn lifecycle_counts_into(mut output: Uint32Array) -> Result<()> {
    if output.len() < 4 { return Err(Error::from_reason("Counter arena is too short")); }
    output[0] = CREATED.load(Ordering::SeqCst);
    output[1] = RELEASED.load(Ordering::SeqCst);
    output[2] = HOOKS_CREATED.load(Ordering::SeqCst);
    output[3] = HOOKS_RELEASED.load(Ordering::SeqCst);
    Ok(())
}

#[napi]
impl NativeOwner {
    // The gate is inside the same step boundary as production and then calls
    // the real Context entry, including its Unavailable refusal. The shared
    // signal is rooted for this invocation and never retained by the owner.
    #[napi]
    pub fn probe_step(&self, env: Env, input: JsObject, out: JsObject, signal: Uint32Array, milliseconds: u32) -> Result<u32> {
        if signal.len() < 2 || milliseconds == 0 || milliseconds > 5000 {
            return Err(Error::from_reason("Invalid step gate fixture"));
        }
        self.step_with(env, input, out, || {
            // SAFETY: Uint32Array elements are aligned for AtomicU32. The
            // caller uses Atomics exclusively until this native call returns.
            let started = unsafe {&*(signal.as_ptr() as *const AtomicU32)};
            let release = unsafe {&*(signal.as_ptr().add(1) as *const AtomicU32)};
            started.store(1, Ordering::SeqCst);
            let deadline = Instant::now() + Duration::from_millis(milliseconds as u64);
            while release.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {std::hint::spin_loop();}
            started.store(2, Ordering::SeqCst);
        })
    }
    #[napi]
    pub fn probe_context_panic(&self) -> Result<()> {
        self.invoke(|owner| {
            let result = owner.context.as_ref().unwrap().world_replay_panic();
            super::world::context_result(owner, result)
        })
    }

    // The world smoke admits recorded cold bytes through Context's harness
    // entry. It never installs a second world implementation in the addon.
    #[napi]
    pub fn probe_world_admit(&self, env: Env, admission: String, out: napi::JsObject) -> Result<u32> {
        self.invoke(|owner| {
            owner.arenas.idle()?;
            let output = super::blocks::arena(env, &out)?;
            use bfe_sim::gen::WorldTickSetup;
            if output.length < WorldTickSetup::WORDS { return Ok(WorldTickSetup::WORDS as u32); }
            let value = admission.parse().map_err(|_| Error::from_reason("Invalid replay admission JSON"))?;
            let result = owner.context.as_ref().unwrap().world_replay_admit(&value);
            let setup = super::world::context_result(owner, result)?;
            let mut words = [0.; WorldTickSetup::WORDS];
            setup.put(&mut words, &mut Vec::new());
            Ok(output.write(&words))
        })
    }

    #[napi]
    pub fn probe_world_prepare(&self, frame: String) -> Result<()> {
        self.invoke(|owner| {
            owner.arenas.idle()?;
            let value = frame.parse().map_err(|_| Error::from_reason("Invalid replay frame JSON"))?;
            let result = owner.context.as_ref().unwrap().world_replay_prepare(&value);
            super::world::context_result(owner, result)
        })
    }

    // Only observations produced by Context leave this entry. The smoke feeds
    // expected fingerprints to its comparator, never to the native world.
    #[napi]
    pub fn probe_world_observe(&self, env: Env, frame: String, outputs: napi::JsObject) -> Result<String> {
        self.invoke(|owner| {
            owner.arenas.idle()?;
            let value = frame.parse().map_err(|_| Error::from_reason("Invalid replay frame JSON"))?;
            let input = super::blocks::arena(env, &outputs)?;
            use bfe_sim::gen::WorldTickOutput;
            input.read(|words| {
                if words.len() % WorldTickOutput::WORDS != 0 {
                    return Err(Error::from_reason("Invalid replay output rows"));
                }
                let rows: Vec<_> = words.chunks_exact(WorldTickOutput::WORDS)
                    .map(|row| WorldTickOutput::take(row, &[])).collect();
                let result = owner.context.as_ref().unwrap().world_replay_publish(&value, &rows);
                super::world::context_result(owner, result).map(|observation| observation.to_string())
            })
        })
    }
}
