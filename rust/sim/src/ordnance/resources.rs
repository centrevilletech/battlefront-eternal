//! Browser resource transport for the ordnance owners. Resources are opaque;
//! selection and all arithmetic remain in the enclosing Rust operation.
use crate::gen::{
    OrdnanceHostArgument as Argument, OrdnanceHostAuthored as Authored, OrdnanceHostExtra as Extra,
    OrdnanceHostField as Field, OrdnanceHostKind as Kind, OrdnanceHostWord as Word,
    ORDNANCE_HOST_AUTHORED_BASE, ORDNANCE_HOST_EXTRA_BASE, ORDNANCE_HOST_WORD_BASE,
};
use three::{Matrix4, Vector3};

#[derive(Clone, Copy, Debug)]
pub struct Key(pub u32);
impl From<Field> for Key {
    fn from(k: Field) -> Self {
        Self(k as u32)
    }
}
impl From<Authored> for Key {
    fn from(k: Authored) -> Self {
        Self(ORDNANCE_HOST_AUTHORED_BASE + k as u32)
    }
}
impl From<Extra> for Key {
    fn from(k: Extra) -> Self {
        Self(ORDNANCE_HOST_EXTRA_BASE + k as u32)
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Value {
    pub kind: u8,
    pub value: f64,
}
impl Value {
    pub const UNDEFINED: Self = Self { kind: 0, value: 0. };
    pub const NULL: Self = Self { kind: 1, value: 0. };
    pub fn number(n: f64) -> Self {
        Self {
            kind: Kind::Number as u8,
            value: n,
        }
    }
    pub fn reference(id: u32) -> Self {
        if id == 0 {
            Self::UNDEFINED
        } else if id == 1 {
            Self::NULL
        } else {
            Self {
                kind: Kind::Reference as u8,
                value: id as f64,
            }
        }
    }
    pub fn boolean(v: bool) -> Self {
        Self {
            kind: Kind::Boolean as u8,
            value: if v { 1. } else { 0. },
        }
    }
    pub fn id(self) -> u32 {
        if self.kind == Kind::Reference as u8 {
            self.value as u32
        } else if self.kind == Kind::Null as u8 {
            1
        } else {
            0
        }
    }
    pub fn number_or(self, default: f64) -> f64 {
        if self.kind == Kind::Number as u8 {
            self.value
        } else {
            default
        }
    }
    pub fn absent(self) -> bool {
        self.kind <= Kind::Null as u8
    }
    pub fn take(v: &crate::gen::OrdnanceHostValue) -> Self {
        Self {
            kind: v.kind as u8,
            value: v.value,
        }
    }
    pub fn put(self) -> crate::gen::OrdnanceHostValue {
        crate::gen::OrdnanceHostValue {
            kind: Kind::from_u8(self.kind).unwrap_or_default(),
            value: self.value,
        }
    }
}
pub fn word(w: Word) -> u32 {
    ORDNANCE_HOST_WORD_BASE + w as u32
}
pub fn present(id: u32) -> bool {
    id > 1
}
pub fn truth(n: f64) -> bool {
    n != 0. && !n.is_nan()
}
pub fn or(n: f64, f: f64) -> f64 {
    if truth(n) {
        n
    } else {
        f
    }
}
pub fn array(v: Vector3) -> [f64; 3] {
    [v.x, v.y, v.z]
}
pub fn vector(v: [f64; 3]) -> Vector3 {
    Vector3::new(v[0], v[1], v[2])
}

// These are primitive property and I/O operations, not game method adapters.
// One native phase may observe resources without returning control decisions
// to JavaScript. No string or argument array is constructed by a warm call.
#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "ordnance_resources")]
extern "C" {
    fn resource_kind(object: u32) -> u32;
    fn reference(object: u32, key: u32) -> u32;
    fn number(object: u32, key: u32) -> f64;
    fn boolean(object: u32, key: u32) -> u32;
    fn index(object: u32, index: u32) -> u32;
    fn numeric_index(object: u32, index: u32) -> f64;
    fn dynamic(object: u32, key: u32) -> u32;
    fn read_vector(object: u32, out: *mut f64);
    fn read_matrix(object: u32, out: *mut f64);
    fn write_vector(object: u32, values: *const f64);
    fn property(object: u32, key: u32, kind: u32, value: f64);
    fn invoke(object: u32, key: u32, args: *const f64, count: u32, numeric: u32) -> f64;
    fn construct(kind: u32, values: *const f64) -> u32;
    fn text(object: u32, output: *mut u16, capacity: u32) -> u32;
    fn intern(input: *const u16, count: u32) -> u32;
    fn publish(root: u32, input: *const f64, count: u32);
    fn clock() -> f64;
    fn stable(id: u32) -> u32;
    fn scratch_vector(slot: u32, values: *const f64) -> u32;
    fn scratch_hit(slot: u32) -> u32;
    fn refresh(node: u32);
    fn unit(id: u32, values: *mut f64);
}
// Native clients bind resources to their own scene/device service. A headless
// world never enters this browser-only resource interface.
#[cfg(not(target_arch = "wasm32"))]
mod unavailable {
    macro_rules! stub {($name:ident($($arg:ident:$ty:ty),*)->$out:ty)=>{pub unsafe fn $name($($arg:$ty),*)->$out {$(let _=$arg;)*panic!("browser ordnance resource service required")}}}
    stub!(resource_kind(object: u32) -> u32);
    stub!(reference(object: u32, key: u32) -> u32);
    stub!(number(object: u32, key: u32) -> f64);
    stub!(boolean(object: u32, key: u32) -> u32);
    stub!(index(object: u32, index: u32) -> u32);
    stub!(numeric_index(object: u32, index: u32) -> f64);
    stub!(dynamic(object: u32, key: u32) -> u32);
    stub!(read_vector(object: u32, out: *mut f64) -> ());
    stub!(read_matrix(object: u32, out: *mut f64) -> ());
    stub!(write_vector(object: u32, values: *const f64) -> ());
    stub!(property(object: u32, key: u32, kind: u32, value: f64) -> ());
    stub!(invoke(object: u32, key: u32, args: *const f64, count: u32, numeric: u32) -> f64);
    stub!(construct(kind: u32, values: *const f64) -> u32);
    stub!(text(object: u32, output: *mut u16, capacity: u32) -> u32);
    stub!(intern(input: *const u16, count: u32) -> u32);
    stub!(publish(root: u32, input: *const f64, count: u32) -> ());
    stub!(clock()->f64);
    stub!(stable(id: u32) -> u32);
    stub!(scratch_vector(slot: u32, values: *const f64) -> u32);
    stub!(scratch_hit(slot: u32) -> u32);
    stub!(refresh(node: u32) -> ());
    stub!(unit(id: u32, values: *mut f64) -> ());
}
#[cfg(not(target_arch = "wasm32"))]
use unavailable::*;

pub struct Env;
impl Env {
    pub fn kind(&self, id: u32) -> crate::gen::OrdnanceHostResourceKind {
        crate::gen::OrdnanceHostResourceKind::from_u8(unsafe { resource_kind(id) } as u8)
            .unwrap_or_default()
    }
    pub fn string_value(&self, id: u32) -> String {
        let mut units = [0u16; 512];
        let n = unsafe { text(id, units.as_mut_ptr(), 512) }.min(512) as usize;
        String::from_utf16_lossy(&units[..n])
    }
    pub fn r(&self, obj: u32, key: impl Into<Key>) -> u32 {
        unsafe { reference(obj, key.into().0) }
    }
    pub fn n(&self, obj: u32, key: impl Into<Key>) -> f64 {
        unsafe { number(obj, key.into().0) }
    }
    pub fn b(&self, obj: u32, key: impl Into<Key>) -> bool {
        unsafe { boolean(obj, key.into().0) != 0 }
    }
    pub fn at(&self, obj: u32, i: usize) -> u32 {
        unsafe { index(obj, i.min(u32::MAX as usize) as u32) }
    }
    pub fn num_at(&self, obj: u32, i: usize) -> f64 {
        unsafe { numeric_index(obj, i.min(u32::MAX as usize) as u32) }
    }
    pub fn dynamic(&self, obj: u32, key: u32) -> u32 {
        unsafe { dynamic(obj, key) }
    }
    pub fn v(&self, obj: u32) -> Vector3 {
        let mut out = [0.; 3];
        unsafe { read_vector(obj, out.as_mut_ptr()) };
        vector(out)
    }
    pub fn m(&self, obj: u32) -> Matrix4 {
        let mut out = Matrix4::default();
        unsafe { read_matrix(obj, out.elements.as_mut_ptr()) };
        out
    }
    pub fn write_v(&self, obj: u32, v: Vector3) {
        unsafe { write_vector(obj, array(v).as_ptr()) }
    }
    pub fn set(&self, obj: u32, key: impl Into<Key>, v: Value) {
        unsafe { property(obj, key.into().0, v.kind as u32, v.value) }
    }
    pub fn call(&self, obj: u32, key: impl Into<Key>, args: &[Value]) -> u32 {
        self.invoke(obj, key.into(), args, false) as u32
    }
    pub fn call_n(&self, obj: u32, key: impl Into<Key>, args: &[Value]) -> f64 {
        self.invoke(obj, key.into(), args, true)
    }
    fn invoke(&self, obj: u32, key: Key, args: &[Value], numeric: bool) -> f64 {
        let mut words = [0.; Argument::WORDS * 12];
        let mut heap = Vec::new();
        let count = args.len().min(12);
        for (i, v) in args.iter().take(count).enumerate() {
            Argument {
                kind: Kind::from_u8(v.kind).unwrap_or_default(),
                value: v.value,
            }
            .put(
                &mut words[i * Argument::WORDS..(i + 1) * Argument::WORDS],
                &mut heap,
            );
        }
        unsafe { invoke(obj, key.0, words.as_ptr(), count as u32, u32::from(numeric)) }
    }
    pub fn object(&self) -> u32 {
        unsafe { construct(0, std::ptr::null()) }
    }
    pub fn new_v(&self, v: Vector3) -> u32 {
        unsafe { construct(1, array(v).as_ptr()) }
    }
    pub fn new_array(&self) -> u32 {
        unsafe { construct(2, std::ptr::null()) }
    }
    pub fn string(&self, v: &str) -> u32 {
        let units: Vec<u16> = v.encode_utf16().collect();
        unsafe { intern(units.as_ptr(), units.len() as u32) }
    }
    pub fn lower(&self, id: u32) -> u32 {
        let mut units = [0u16; 512];
        let n = unsafe { text(id, units.as_mut_ptr(), 512) }.min(512) as usize;
        self.string(&String::from_utf16_lossy(&units[..n]).to_lowercase())
    }
    pub fn ascii(&self, id: u32, wanted: &[u8]) -> bool {
        let mut units = [0u16; 128];
        let n = unsafe { text(id, units.as_mut_ptr(), 128) } as usize;
        n == wanted.len()
            && n <= 128
            && units[..n]
                .iter()
                .zip(wanted)
                .all(|(&a, &b)| a == b as u16 || a == b.to_ascii_uppercase() as u16)
    }
    pub fn publish(&self, root: u32, words: &[f64], count: usize) {
        unsafe { publish(root, words.as_ptr(), count as u32) }
    }
    pub fn now(&self, root: u32) -> f64 {
        if present(self.r(root, Field::Now)) {
            self.call_n(root, Field::Now, &[])
        } else {
            (unsafe { clock() }) / 1000.
        }
    }
    pub fn stable(&self, id: u32) -> u32 {
        unsafe { stable(id) }
    }
    pub fn temp_v(&self, slot: u32, v: Vector3) -> u32 {
        unsafe { scratch_vector(slot.min(31), array(v).as_ptr()) }
    }
    pub fn hit(&self, slot: u32) -> u32 {
        unsafe { scratch_hit(slot.min(7)) }
    }
    pub fn refresh(&self, node: u32) {
        unsafe { refresh(node) }
    }
    pub fn unit(&self, id: u32) -> crate::gen::OrdnanceHostUnit {
        let mut words = [0.; crate::gen::OrdnanceHostUnit::WORDS];
        unsafe { unit(id, words.as_mut_ptr()) };
        crate::gen::OrdnanceHostUnit::take(&words, &[])
    }
    pub fn len(&self, id: u32) -> usize {
        let n = self.n(id, Field::Length);
        if n.is_finite() && n > 0. {
            n.min(65535.) as usize
        } else {
            0
        }
    }
}
impl Key {
    pub const fn named(name: &str) -> Self {
        const fn equal(a: &str, b: &str) -> bool {
            let a = a.as_bytes();
            let b = b.as_bytes();
            if a.len() != b.len() {
                return false;
            }
            let mut i = 0;
            while i < a.len() {
                if a[i] != b[i] {
                    return false;
                }
                i += 1;
            }
            true
        }
        let mut i = 0;
        while i < Field::NAMES.len() {
            if equal(name, Field::NAMES[i]) {
                return Self(i as u32);
            }
            i += 1;
        }
        i = 0;
        while i < Extra::NAMES.len() {
            if equal(name, Extra::NAMES[i]) {
                return Self(ORDNANCE_HOST_EXTRA_BASE + i as u32);
            }
            i += 1;
        }
        i = 0;
        while i < Authored::NAMES.len() {
            if equal(name, Authored::NAMES[i]) {
                return Self(ORDNANCE_HOST_AUTHORED_BASE + i as u32);
            }
            i += 1;
        }
        panic!("unknown ordnance resource property")
    }
}
#[macro_export]
macro_rules! ordnance_key {
    ($name:ident) => {
        const { $crate::ordnance::resources::Key::named(stringify!($name)) }
    };
}
