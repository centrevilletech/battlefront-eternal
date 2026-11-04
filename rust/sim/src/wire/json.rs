//! JSON values used by the wire's state tags. Strings remain UTF-16, so
//! well-formed JSON containing an unpaired surrogate retains its identity.
#[derive(Debug, Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(Vec<u16>),
    Array(Vec<Value>),
    Object(Vec<(Vec<u16>, Value)>),
}
pub fn parse(bytes: &[u8]) -> Option<Value> {
    let text = String::from_utf8_lossy(bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let mut p = Parser { text, at: 0 };
    let v = p.value(0)?;
    p.space();
    (p.at == text.len()).then_some(v)
}
struct Parser<'a> {
    text: &'a str,
    at: usize,
}
impl Parser<'_> {
    fn byte(&self) -> Option<u8> {
        self.text.as_bytes().get(self.at).copied()
    }
    fn space(&mut self) {
        while matches!(self.byte(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.at += 1;
        }
    }
    fn literal(&mut self, s: &str) -> Option<()> {
        if self.text[self.at..].starts_with(s) {
            self.at += s.len();
            Some(())
        } else {
            None
        }
    }
    fn string(&mut self) -> Option<Vec<u16>> {
        if self.byte() != Some(b'"') {
            return None;
        }
        self.at += 1;
        let mut out = Vec::new();
        loop {
            match self.byte()? {
                b'"' => {
                    self.at += 1;
                    return Some(out);
                }
                0..=31 => return None,
                b'\\' => {
                    self.at += 1;
                    let c = self.byte()?;
                    self.at += 1;
                    out.push(match c {
                        b'"' => 34,
                        b'\\' => 92,
                        b'/' => 47,
                        b'b' => 8,
                        b'f' => 12,
                        b'n' => 10,
                        b'r' => 13,
                        b't' => 9,
                        b'u' => {
                            let part = self.text.as_bytes().get(self.at..self.at + 4)?;
                            let mut n = 0;
                            for &d in part {
                                n = n * 16 + (d as char).to_digit(16)? as u16;
                            }
                            self.at += 4;
                            n
                        }
                        _ => return None,
                    });
                }
                _ => {
                    let ch = self.text[self.at..].chars().next()?;
                    self.at += ch.len_utf8();
                    let mut units = [0; 2];
                    out.extend_from_slice(ch.encode_utf16(&mut units));
                }
            }
        }
    }
    fn number(&mut self) -> Option<Value> {
        let start = self.at;
        if self.byte() == Some(b'-') {
            self.at += 1;
        }
        match self.byte()? {
            b'0' => self.at += 1,
            b'1'..=b'9' => {
                self.at += 1;
                while matches!(self.byte(), Some(b'0'..=b'9')) {
                    self.at += 1;
                }
            }
            _ => return None,
        }
        if self.byte() == Some(b'.') {
            self.at += 1;
            let n = self.at;
            while matches!(self.byte(), Some(b'0'..=b'9')) {
                self.at += 1;
            }
            if n == self.at {
                return None;
            }
        }
        if matches!(self.byte(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.byte(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            let n = self.at;
            while matches!(self.byte(), Some(b'0'..=b'9')) {
                self.at += 1;
            }
            if n == self.at {
                return None;
            }
        }
        Some(Value::Number(self.text[start..self.at].parse().ok()?))
    }
    fn value(&mut self, depth: usize) -> Option<Value> {
        if depth > 512 {
            return None;
        }
        self.space();
        match self.byte()? {
            b'n' => {
                self.literal("null")?;
                Some(Value::Null)
            }
            b't' => {
                self.literal("true")?;
                Some(Value::Bool(true))
            }
            b'f' => {
                self.literal("false")?;
                Some(Value::Bool(false))
            }
            b'"' => self.string().map(Value::String),
            b'[' => {
                self.at += 1;
                let mut a = Vec::new();
                self.space();
                if self.byte() == Some(b']') {
                    self.at += 1;
                    return Some(Value::Array(a));
                }
                loop {
                    a.push(self.value(depth + 1)?);
                    self.space();
                    match self.byte()? {
                        b']' => {
                            self.at += 1;
                            break;
                        }
                        b',' => self.at += 1,
                        _ => return None,
                    }
                }
                Some(Value::Array(a))
            }
            b'{' => {
                self.at += 1;
                let mut a = Vec::new();
                self.space();
                if self.byte() == Some(b'}') {
                    self.at += 1;
                    return Some(Value::Object(a));
                }
                loop {
                    self.space();
                    let key = self.string()?;
                    self.space();
                    if self.byte() != Some(b':') {
                        return None;
                    }
                    self.at += 1;
                    let value = self.value(depth + 1)?;
                    a.push((key, value));
                    self.space();
                    match self.byte()? {
                        b'}' => {
                            self.at += 1;
                            break;
                        }
                        b',' => self.at += 1,
                        _ => return None,
                    }
                }
                Some(Value::Object(a))
            }
            _ => self.number(),
        }
    }
}
pub fn text(v: Option<&Value>) -> Option<Vec<u16>> {
    Some(match v {
        None => "undefined".encode_utf16().collect(),
        Some(Value::Null) => "null".encode_utf16().collect(),
        Some(Value::Bool(b)) => (if *b { "true" } else { "false" }).encode_utf16().collect(),
        Some(Value::Number(n)) => jsrt::numfmt::number_to_string(*n).encode_utf16().collect(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => {
            let mut out = Vec::new();
            for (i, v) in a.iter().enumerate() {
                if i > 0 {
                    out.push(44);
                }
                if !matches!(v, Value::Null) {
                    out.extend(text(Some(v))?);
                }
            }
            out
        }
        Some(Value::Object(a)) => {
            if a.iter()
                .any(|(k, _)| k.iter().copied().eq("toString".encode_utf16()))
            {
                return None;
            }
            "[object Object]".encode_utf16().collect()
        }
    })
}
pub fn building_state_team(payload: &[u8]) -> Option<Vec<u16>> {
    let Value::Array(a) = parse(payload)? else {
        return None;
    };
    let mut out = text(a.get(1))?;
    out.push(58);
    out.extend(text(a.get(3))?);
    Some(out)
}
