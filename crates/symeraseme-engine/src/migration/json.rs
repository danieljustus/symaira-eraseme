//! The two migration documents use Go encoding/json's struct/map semantics.
//! Validate the entire byte stream before assigning fields (syntax wins over
//! type errors). A flat token tape retains duplicates and skips unknown values
//! without recursive allocation or Serde's smaller nesting limit. This is private
//! to migration: no general JSON API, floating point conversion, or path I/O.
use super::State;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Object,
    Array,
    String,
    Number,
    Bool,
    Null,
}
struct Token {
    kind: Kind,
    start: usize,
    end: usize,
    next: usize,
}
impl Token {
    fn name(&self) -> &'static str {
        match self.kind {
            Kind::Object => "object",
            Kind::Array => "array",
            Kind::String => "string",
            Kind::Number => "number",
            Kind::Bool => "bool",
            Kind::Null => "null",
        }
    }
}
#[derive(Clone, Copy)]
enum Expect {
    Value,
    ArrayFirst,
    KeyFirst,
    Key,
    Colon,
    After,
}
fn invalid(c: u8, context: &str) -> String {
    // encoding/json.quoteChar quotes a byte as a Latin-1 rune, not UTF-8.
    let quoted = match c {
        b'\'' => "\\'".into(),
        b'\\' => "\\\\".into(),
        7 => "\\a".into(),
        8 => "\\b".into(),
        12 => "\\f".into(),
        b'\n' => "\\n".into(),
        b'\r' => "\\r".into(),
        b'\t' => "\\t".into(),
        11 => "\\v".into(),
        0..=31 | 127 => format!("\\x{c:02x}"),
        128..=160 | 173 => format!("\\u{c:04x}"),
        _ => char::from(c).to_string(),
    };
    format!("invalid character '{quoted}' {context}")
}
const EOF: &str = "unexpected end of JSON input";
struct Scanner<'a> {
    data: &'a [u8],
    pos: usize,
    tokens: Vec<Token>,
}
impl Scanner<'_> {
    fn byte(&self) -> u8 {
        // Go's scanner feeds one synthetic space at EOF, which matters for
        // truncated numbers, keywords, and escapes. It is never consumed.
        self.data.get(self.pos).copied().unwrap_or(b' ')
    }
    fn push(&mut self, kind: Kind, start: usize) {
        self.tokens.push(Token {
            kind,
            start,
            end: self.pos,
            next: self.tokens.len() + 1,
        });
    }
    fn string(&mut self) -> Result<(), String> {
        self.pos += 1;
        while self.pos < self.data.len() {
            match self.byte() {
                b'"' => {
                    self.pos += 1;
                    return Ok(());
                }
                b'\\' => {
                    self.pos += 1;
                    match self.byte() {
                        b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {}
                        b'u' => {
                            for _ in 0..4 {
                                self.pos += 1;
                                if !self.byte().is_ascii_hexdigit() {
                                    return Err(invalid(
                                        self.byte(),
                                        "in \\u hexadecimal character escape",
                                    ));
                                }
                            }
                        }
                        c => return Err(invalid(c, "in string escape code")),
                    }
                }
                c if c < 32 => return Err(invalid(c, "in string literal")),
                _ => {}
            }
            self.pos += 1;
        }
        Err(EOF.into())
    }
    fn digits(&mut self, context: &str) -> Result<(), String> {
        if !self.byte().is_ascii_digit() {
            return Err(invalid(self.byte(), context));
        }
        while self.byte().is_ascii_digit() {
            self.pos += 1;
        }
        Ok(())
    }
    fn number(&mut self) -> Result<(), String> {
        if self.byte() == b'-' {
            self.pos += 1;
        }
        if self.byte() == b'0' {
            self.pos += 1;
        } else {
            self.digits("in numeric literal")?;
        }
        if self.byte() == b'.' {
            self.pos += 1;
            self.digits("after decimal point in numeric literal")?;
        }
        if matches!(self.byte(), b'e' | b'E') {
            self.pos += 1;
            if matches!(self.byte(), b'+' | b'-') {
                self.pos += 1;
            }
            self.digits("in exponent of numeric literal")?;
        }
        Ok(())
    }
    fn literal(&mut self, word: &str) -> Result<(), String> {
        for c in word.bytes() {
            if self.byte() != c {
                return Err(invalid(
                    self.byte(),
                    &format!("in literal {word} (expecting '{}')", char::from(c)),
                ));
            }
            self.pos += 1;
        }
        Ok(())
    }
    fn scan(mut self) -> Result<Vec<Token>, String> {
        let mut stack: Vec<usize> = Vec::new();
        let mut expect = Expect::Value;
        loop {
            while self.pos < self.data.len() && matches!(self.byte(), b' ' | b'\t' | b'\r' | b'\n')
            {
                self.pos += 1;
            }
            if self.pos == self.data.len() {
                return if matches!(expect, Expect::After) && stack.is_empty() {
                    Ok(self.tokens)
                } else {
                    Err(EOF.into())
                };
            }
            let c = self.byte();
            let start = self.pos;
            let close = match expect {
                Expect::KeyFirst if c == b'}' => true,
                Expect::ArrayFirst if c == b']' => true,
                Expect::KeyFirst | Expect::Key => {
                    if c != b'"' {
                        return Err(invalid(c, "looking for beginning of object key string"));
                    }
                    self.string()?;
                    self.push(Kind::String, start);
                    expect = Expect::Colon;
                    false
                }
                Expect::Colon => {
                    if c != b':' {
                        return Err(invalid(c, "after object key"));
                    }
                    self.pos += 1;
                    expect = Expect::Value;
                    false
                }
                Expect::Value | Expect::ArrayFirst => {
                    let kind = match c {
                        b'{' => Kind::Object,
                        b'[' => Kind::Array,
                        b'"' => {
                            self.string()?;
                            Kind::String
                        }
                        b'-' | b'0'..=b'9' => {
                            self.number()?;
                            Kind::Number
                        }
                        b't' => {
                            self.literal("true")?;
                            Kind::Bool
                        }
                        b'f' => {
                            self.literal("false")?;
                            Kind::Bool
                        }
                        b'n' => {
                            self.literal("null")?;
                            Kind::Null
                        }
                        _ => return Err(invalid(c, "looking for beginning of value")),
                    };
                    if matches!(kind, Kind::Object | Kind::Array) {
                        stack.push(self.tokens.len());
                        if stack.len() > 10000 {
                            return Err(invalid(c, "exceeded max depth"));
                        }
                        self.pos += 1;
                        expect = if kind == Kind::Object {
                            Expect::KeyFirst
                        } else {
                            Expect::ArrayFirst
                        };
                    } else {
                        expect = Expect::After;
                    }
                    self.push(kind, start);
                    false
                }
                Expect::After => {
                    let Some(&parent) = stack.last() else {
                        return Err(invalid(c, "after top-level value"));
                    };
                    let object = self.tokens[parent].kind == Kind::Object;
                    if c == b',' {
                        self.pos += 1;
                        expect = if object { Expect::Key } else { Expect::Value };
                        false
                    } else if c == if object { b'}' } else { b']' } {
                        true
                    } else {
                        return Err(invalid(
                            c,
                            if object {
                                "after object key:value pair"
                            } else {
                                "after array element"
                            },
                        ));
                    }
                }
            };
            if close {
                self.pos += 1;
                let index = stack.pop().expect("container being closed");
                self.tokens[index].end = self.pos;
                self.tokens[index].next = self.tokens.len();
                expect = Expect::After;
            }
        }
    }
}

fn hex4(data: &[u8]) -> u32 {
    data.iter().fold(0, |n, c| {
        n * 16 + char::from(*c).to_digit(16).expect("validated hex")
    })
}
fn string(data: &[u8], token: &Token) -> String {
    let data = &data[token.start + 1..token.end - 1];
    let mut out = String::new();
    let mut i = 0;
    while i < data.len() {
        if data[i] == b'\\' {
            i += 1;
            let c = match data[i] {
                b'b' => '\u{8}',
                b'f' => '\u{c}',
                b'n' => '\n',
                b'r' => '\r',
                b't' => '\t',
                b'u' => {
                    let mut rune = hex4(&data[i + 1..i + 5]);
                    i += 4;
                    if (0xd800..=0xdbff).contains(&rune)
                        && data.get(i + 1..i + 3) == Some(b"\\u")
                        && i + 7 <= data.len()
                    {
                        let low = hex4(&data[i + 3..i + 7]);
                        if (0xdc00..=0xdfff).contains(&low) {
                            rune = 0x10000 + (rune - 0xd800) * 1024 + low - 0xdc00;
                            i += 6;
                        }
                    }
                    char::from_u32(rune).unwrap_or('\u{fffd}')
                }
                c => char::from(c),
            };
            out.push(c);
            i += 1;
        } else {
            let size = match data[i] {
                0..=127 => 1,
                194..=223 => 2,
                224..=239 => 3,
                240..=244 => 4,
                _ => 0,
            };
            if size > 0
                && let Some(text) = data
                    .get(i..i + size)
                    .and_then(|b| std::str::from_utf8(b).ok())
            {
                out.push_str(text);
                i += size;
            } else {
                // utf8.DecodeRune consumes exactly one byte on an error.
                out.push('\u{fffd}');
                i += 1;
            }
        }
    }
    out
}
fn field_name(name: &str) -> String {
    // The only non-ASCII SimpleFold classes that meet these ASCII JSON tags
    // are long s (S) and Kelvin sign (K). No Unicode normalization is applied.
    name.chars()
        .map(|c| match c {
            'ſ' => 's',
            'K' => 'k',
            _ => c.to_ascii_lowercase(),
        })
        .collect()
}
fn type_error(token: &Token, field: &str, ty: &str) -> String {
    format!(
        "json: cannot unmarshal {} into Go struct field state.{field} of type {ty}",
        token.name()
    )
}
fn assign_string(data: &[u8], token: &Token, field: &str, out: &mut String) -> Result<(), String> {
    match token.kind {
        Kind::Null => Ok(()),
        Kind::String => {
            *out = string(data, token);
            Ok(())
        }
        _ => Err(type_error(token, field, "string")),
    }
}
fn assign_int(data: &[u8], token: &Token, out: &mut isize) -> Result<(), String> {
    if token.kind == Kind::Null {
        return Ok(());
    }
    if token.kind != Kind::Number {
        return Err(type_error(token, "version", "int"));
    }
    let number = std::str::from_utf8(&data[token.start..token.end]).expect("ASCII number");
    *out = number.parse().map_err(|_| {
        format!(
            "json: cannot unmarshal number {number} into Go struct field state.version of type int"
        )
    })?;
    Ok(())
}
fn decode(data: &[u8], marker: bool) -> Result<State, String> {
    let tokens = Scanner {
        data,
        pos: 0,
        tokens: Vec::new(),
    }
    .scan()?;
    let mut state = State::default();
    if tokens[0].kind == Kind::Null {
        return Ok(state);
    }
    if tokens[0].kind != Kind::Object {
        return Err(format!(
            "json: cannot unmarshal {} into Go value of type migration.state",
            tokens[0].name()
        ));
    }
    let mut i = 1;
    while i < tokens[0].next {
        let field = field_name(&string(data, &tokens[i]));
        let value = &tokens[i + 1];
        match field.as_str() {
            "version" => assign_int(data, value, &mut state.version)?,
            "source" if marker => assign_string(data, value, "source", &mut state.source_root)?,
            "source_root" if !marker => {
                assign_string(data, value, "source_root", &mut state.source_root)?
            }
            "destination_root" if !marker => {
                assign_string(data, value, "destination_root", &mut state.destination_root)?
            }
            "backup_dir" if !marker => {
                assign_string(data, value, "backup_dir", &mut state.backup_dir)?
            }
            "items" if !marker => match value.kind {
                Kind::Null => state.items = None,
                Kind::Object => {
                    let items = state.items.get_or_insert_default();
                    let mut j = i + 2;
                    while j < value.next {
                        let key = string(data, &tokens[j]);
                        let mut item = String::new(); // Map elements start at zero, even for duplicate keys.
                        assign_string(data, &tokens[j + 1], "items", &mut item)?;
                        items.insert(key, item);
                        j = tokens[j + 1].next;
                    }
                }
                _ => return Err(type_error(value, "items", "map[string]string")),
            },
            _ => {}
        }
        // Type errors may return immediately: Go retains the first error, and
        // loadState/ensureBackup discard the entire result on any error.
        i = value.next;
    }
    Ok(state)
}
pub(super) fn state(data: &[u8]) -> Result<State, String> {
    decode(data, false)
}
pub(super) fn marker_matches(data: &[u8], source: &str) -> bool {
    decode(data, true).is_ok_and(|s| s.version == 1 && s.source_root == source)
}
