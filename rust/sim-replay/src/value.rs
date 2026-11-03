//! JavaScript values as a capture holds them: numbers with all their 64
//! bits, strings as their UTF-16 code units, objects with their class name,
//! key order and identity. A record's composite values (arrays, objects,
//! three.js values, typed arrays, Maps and Sets) live in its arena of
//! nodes, numbered as the capture first met them, so an object met twice is
//! one node and cycles are possible.

/// A value of a record. Composite values are `Node`s of the record's arena.
#[derive(Clone, Debug)]
pub enum Value {
    Undefined,
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    /// A string that is not well-formed UTF-16 (a lone surrogate): its code units.
    Str16(Vec<u16>),
    /// An array, object, three.js value, typed array, Map or Set: its number in the arena.
    Node(u32),
    /// A function, by its name (a capture never holds code).
    Function(String),
    /// What a record names without holding: a unit, a hull, a collision body, a
    /// definition, a system of the game, another class's instance (kind, key).
    Handle(String, Box<Value>),
    /// What a capture left out, and why (a getter, too deep, a WeakMap).
    Omitted(String),
}

/// A composite value in a record's arena.
#[derive(Clone, Debug)]
pub enum Node {
    Array(Vec<Value>),
    /// An object: its class ("" for a plain object) and its own enumerable keys in order.
    Object { class: String, fields: Vec<(String, Value)> },
    Vec3([f64; 3]),
    /// x, y, z, w.
    Quat([f64; 4]),
    /// three.js's column-major elements.
    Mat4([f64; 16]),
    Mat3([f64; 9]),
    /// x, y, z and the order.
    Euler([f64; 3], String),
    /// min x, y, z, max x, y, z.
    Box3([f64; 6]),
    Typed(Typed),
    Map(Vec<(Value, Value)>),
    Set(Vec<Value>),
}

/// A typed array's elements.
#[derive(Clone, Debug, PartialEq)]
pub enum Typed {
    F32(Vec<f32>),
    F64(Vec<f64>),
    I8(Vec<i8>),
    U8(Vec<u8>),
    U8Clamped(Vec<u8>),
    I16(Vec<i16>),
    U16(Vec<u16>),
    I32(Vec<i32>),
    U32(Vec<u32>),
    I64(Vec<i64>),
    U64(Vec<u64>),
}

impl Typed {
    pub fn len(&self) -> usize {
        match self {
            Typed::F32(v) => v.len(),
            Typed::F64(v) => v.len(),
            Typed::I8(v) => v.len(),
            Typed::U8(v) | Typed::U8Clamped(v) => v.len(),
            Typed::I16(v) => v.len(),
            Typed::U16(v) => v.len(),
            Typed::I32(v) => v.len(),
            Typed::U32(v) => v.len(),
            Typed::I64(v) => v.len(),
            Typed::U64(v) => v.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// The JavaScript class name.
    pub fn class(&self) -> &'static str {
        match self {
            Typed::F32(_) => "Float32Array",
            Typed::F64(_) => "Float64Array",
            Typed::I8(_) => "Int8Array",
            Typed::U8(_) => "Uint8Array",
            Typed::U8Clamped(_) => "Uint8ClampedArray",
            Typed::I16(_) => "Int16Array",
            Typed::U16(_) => "Uint16Array",
            Typed::I32(_) => "Int32Array",
            Typed::U32(_) => "Uint32Array",
            Typed::I64(_) => "BigInt64Array",
            Typed::U64(_) => "BigUint64Array",
        }
    }
    /// Element `i` as JavaScript reads it (a number; BigInt elements as their nearest f64).
    pub fn get(&self, i: usize) -> Option<f64> {
        Some(match self {
            Typed::F32(v) => *v.get(i)? as f64,
            Typed::F64(v) => *v.get(i)?,
            Typed::I8(v) => *v.get(i)? as f64,
            Typed::U8(v) | Typed::U8Clamped(v) => *v.get(i)? as f64,
            Typed::I16(v) => *v.get(i)? as f64,
            Typed::U16(v) => *v.get(i)? as f64,
            Typed::I32(v) => *v.get(i)? as f64,
            Typed::U32(v) => *v.get(i)? as f64,
            Typed::I64(v) => *v.get(i)? as f64,
            Typed::U64(v) => *v.get(i)? as f64,
        })
    }
    /// Every element's bits equal (an f32 or f64 NaN equal to any NaN, as JavaScript reads it).
    pub fn same(&self, other: &Typed) -> Option<usize> {
        let n = self.len();
        if other.len() != n || std::mem::discriminant(self) != std::mem::discriminant(other) {
            return Some(0);
        }
        match (self, other) {
            (Typed::F32(a), Typed::F32(b)) => a.iter().zip(b).position(|(x, y)| !(x.to_bits() == y.to_bits() || (x.is_nan() && y.is_nan()))),
            (Typed::F64(a), Typed::F64(b)) => a.iter().zip(b).position(|(x, y)| !same_number(*x, *y)),
            _ => (0..n).find(|&i| self.get(i).map(f64::to_bits) != other.get(i).map(f64::to_bits)),
        }
    }
}

/// JavaScript's `Object.is` on two numbers: the same bits, or both NaN.
pub fn same_number(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

/// A number as JavaScript would show it, with its bits.
pub fn show_number(x: f64) -> String {
    if x == 0.0 && x.is_sign_negative() {
        return "-0".into();
    }
    format!("{x:?} ({:016x})", x.to_bits())
}

/// A record's arena: the nodes its values number.
#[derive(Clone, Debug, Default)]
pub struct Arena {
    pub nodes: Vec<Node>,
}

impl Arena {
    pub fn node(&self, id: u32) -> &Node {
        &self.nodes[id as usize]
    }
    /// A view of `value`, whose nodes are this arena's.
    pub fn view<'a>(&'a self, value: &'a Value) -> V<'a> {
        V { arena: self, value }
    }
    fn push(&mut self, node: Node) -> Value {
        self.nodes.push(node);
        Value::Node(self.nodes.len() as u32 - 1)
    }
    // Builders, for the state a port computes (compared with a capture's by `diff`).
    pub fn array(&mut self, items: Vec<Value>) -> Value {
        self.push(Node::Array(items))
    }
    pub fn object(&mut self, class: &str, fields: Vec<(&str, Value)>) -> Value {
        self.push(Node::Object { class: class.into(), fields: fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect() })
    }
    pub fn vec3(&mut self, v: [f64; 3]) -> Value {
        self.push(Node::Vec3(v))
    }
    pub fn quat(&mut self, q: [f64; 4]) -> Value {
        self.push(Node::Quat(q))
    }
    pub fn typed(&mut self, t: Typed) -> Value {
        self.push(Node::Typed(t))
    }
    pub fn map(&mut self, entries: Vec<(Value, Value)>) -> Value {
        self.push(Node::Map(entries))
    }
    pub fn set(&mut self, items: Vec<Value>) -> Value {
        self.push(Node::Set(items))
    }
    /// A node already in the arena, met again (the same object).
    pub fn again(&self, value: &Value) -> Value {
        value.clone()
    }
}

impl Value {
    pub fn num(x: f64) -> Value {
        Value::Num(x)
    }
    pub fn str(s: &str) -> Value {
        Value::Str(s.into())
    }
    pub fn handle(kind: &str, key: Value) -> Value {
        Value::Handle(kind.into(), Box::new(key))
    }
}

static UNDEFINED: Value = Value::Undefined;

/// A value with the arena it lives in: what replay code reads a capture through.
/// Reading a key an object lacks gives `undefined`, as in JavaScript.
#[derive(Clone, Copy, Debug)]
pub struct V<'a> {
    pub arena: &'a Arena,
    pub value: &'a Value,
}

impl<'a> V<'a> {
    fn with(&self, value: &'a Value) -> V<'a> {
        V { arena: self.arena, value }
    }
    pub fn node(&self) -> Option<&'a Node> {
        match self.value {
            Value::Node(id) => Some(self.arena.node(*id)),
            _ => None,
        }
    }
    pub fn node_id(&self) -> Option<u32> {
        match self.value {
            Value::Node(id) => Some(*id),
            _ => None,
        }
    }
    /// An object's field (undefined when it has none, or is no object).
    pub fn get(&self, key: &str) -> V<'a> {
        if let Some(Node::Object { fields, .. }) = self.node() {
            if let Some((_, v)) = fields.iter().find(|(k, _)| k == key) {
                return self.with(v);
            }
        }
        self.with(&UNDEFINED)
    }
    /// Whether an object has the key (as `key in object`, own keys only).
    pub fn has(&self, key: &str) -> bool {
        matches!(self.node(), Some(Node::Object { fields, .. }) if fields.iter().any(|(k, _)| k == key))
    }
    /// An array's element (undefined past its end).
    pub fn at(&self, i: usize) -> V<'a> {
        match self.node() {
            Some(Node::Array(items)) | Some(Node::Set(items)) => items.get(i).map(|v| self.with(v)).unwrap_or(self.with(&UNDEFINED)),
            _ => self.with(&UNDEFINED),
        }
    }
    pub fn len(&self) -> usize {
        match self.node() {
            Some(Node::Array(items)) | Some(Node::Set(items)) => items.len(),
            Some(Node::Map(entries)) => entries.len(),
            Some(Node::Object { fields, .. }) => fields.len(),
            Some(Node::Typed(t)) => t.len(),
            _ => 0,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// An array's elements.
    pub fn items(&self) -> impl Iterator<Item = V<'a>> + 'a {
        let me = *self;
        let items: &'a [Value] = match self.node() {
            Some(Node::Array(items)) | Some(Node::Set(items)) => items,
            _ => &[],
        };
        items.iter().map(move |v| me.with(v))
    }
    /// An object's own keys and values, in order.
    pub fn fields(&self) -> impl Iterator<Item = (&'a str, V<'a>)> + 'a {
        let me = *self;
        let fields: &'a [(String, Value)] = match self.node() {
            Some(Node::Object { fields, .. }) => fields,
            _ => &[],
        };
        fields.iter().map(move |(k, v)| (k.as_str(), me.with(v)))
    }
    /// A Map's entries, in order.
    pub fn entries(&self) -> impl Iterator<Item = (V<'a>, V<'a>)> + 'a {
        let me = *self;
        let entries: &'a [(Value, Value)] = match self.node() {
            Some(Node::Map(entries)) => entries,
            _ => &[],
        };
        entries.iter().map(move |(k, v)| (me.with(k), me.with(v)))
    }
    /// An object's class ("" for a plain one).
    pub fn class(&self) -> Option<&'a str> {
        match self.node() {
            Some(Node::Object { class, .. }) => Some(class),
            _ => None,
        }
    }
    pub fn opt_num(&self) -> Option<f64> {
        match self.value {
            Value::Num(x) => Some(*x),
            _ => None,
        }
    }
    /// A number (panics, naming what it found, on anything else: for replay code that knows its data).
    pub fn num(&self) -> f64 {
        self.opt_num().unwrap_or_else(|| panic!("not a number: {}", self.show()))
    }
    pub fn opt_bool(&self) -> Option<bool> {
        match self.value {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn bool(&self) -> bool {
        self.opt_bool().unwrap_or_else(|| panic!("not a boolean: {}", self.show()))
    }
    pub fn str(&self) -> Option<&'a str> {
        match self.value {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn is_undefined(&self) -> bool {
        matches!(self.value, Value::Undefined)
    }
    pub fn is_null(&self) -> bool {
        matches!(self.value, Value::Null)
    }
    /// null or undefined (what `??` and `?.` test).
    pub fn is_nullish(&self) -> bool {
        matches!(self.value, Value::Null | Value::Undefined)
    }
    /// JavaScript's truthiness (what `||`, `&&` and `if` test): false for
    /// undefined, null, false, 0, -0, NaN and "".
    pub fn truthy(&self) -> bool {
        match self.value {
            Value::Undefined | Value::Null => false,
            Value::Bool(b) => *b,
            Value::Num(x) => !(*x == 0.0 || x.is_nan()),
            Value::Str(s) => !s.is_empty(),
            Value::Str16(s) => !s.is_empty(),
            _ => true,
        }
    }
    /// A handle's kind and key.
    pub fn handle(&self) -> Option<(&'a str, V<'a>)> {
        match self.value {
            Value::Handle(kind, key) => Some((kind, self.with(key))),
            _ => None,
        }
    }
    /// A handle of this kind's key as a number (a unit's id, a body's serial).
    pub fn handle_num(&self, kind: &str) -> Option<f64> {
        self.handle().filter(|(k, _)| *k == kind).and_then(|(_, key)| key.opt_num())
    }
    /// A handle of this kind's key as a string (a definition's key).
    pub fn handle_str(&self, kind: &str) -> Option<&'a str> {
        self.handle().filter(|(k, _)| *k == kind).and_then(|(_, key)| key.str())
    }
    pub fn function(&self) -> Option<&'a str> {
        match self.value {
            Value::Function(name) => Some(name),
            _ => None,
        }
    }
    pub fn vec3(&self) -> Option<[f64; 3]> {
        match self.node() {
            Some(Node::Vec3(v)) => Some(*v),
            _ => None,
        }
    }
    pub fn typed(&self) -> Option<&'a Typed> {
        match self.node() {
            Some(Node::Typed(t)) => Some(t),
            _ => None,
        }
    }
    /// A short description, for messages.
    pub fn show(&self) -> String {
        show(*self, 2)
    }
}

fn show(v: V, depth: usize) -> String {
    match v.value {
        Value::Undefined => "undefined".into(),
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Num(x) => show_number(*x),
        Value::Str(s) => format!("{s:?}"),
        Value::Str16(s) => format!("{:?} (UTF-16)", String::from_utf16_lossy(s)),
        Value::Function(name) => format!("<function {name}>"),
        Value::Handle(kind, key) => format!("<{kind} {}>", show(v.with(key), depth)),
        Value::Omitted(why) => format!("<left out: {why}>"),
        Value::Node(id) => {
            let node = v.arena.node(*id);
            match node {
                Node::Vec3(a) => format!("Vector3({}, {}, {})", a[0], a[1], a[2]),
                Node::Quat(a) => format!("Quaternion({}, {}, {}, {})", a[0], a[1], a[2], a[3]),
                Node::Typed(t) => format!("{}({})", t.class(), t.len()),
                _ if depth == 0 => format!("<#{id}>"),
                Node::Array(items) => format!("[{}]", items.iter().take(8).map(|x| show(v.with(x), depth - 1)).collect::<Vec<_>>().join(", ")),
                Node::Object { class, fields } => format!(
                    "{}{{{}}}",
                    class,
                    fields.iter().take(12).map(|(k, x)| format!("{k}: {}", show(v.with(x), depth - 1))).collect::<Vec<_>>().join(", ")
                ),
                other => format!("{other:?}").chars().take(120).collect(),
            }
        }
    }
}
