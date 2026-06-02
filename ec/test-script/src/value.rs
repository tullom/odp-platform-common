//! Runtime value model for the text-DSL executor.
//!
//! Each call resolves to a [`Value`] (or `Err`). Dotted projections walk
//! the struct map; verbs operate on the leaf value's kind.
//!
//! SPDX-License-Identifier: MIT

use std::fmt;

#[derive(Debug, Clone)]
pub enum Value {
    /// Setter success.
    Unit,
    Num(f64),
    Bool(bool),
    Struct(Vec<(String, Value)>),
}

impl Value {
    pub fn project(&self, path: &[String]) -> Result<&Value, String> {
        let mut cur = self;
        for seg in path {
            cur = match cur {
                Value::Struct(fields) => fields
                    .iter()
                    .find_map(|(k, v)| (k == seg).then_some(v))
                    .ok_or_else(|| {
                        let avail: Vec<&str> = fields.iter().map(|(k, _)| k.as_str()).collect();
                        format!("no field `{seg}` (have: {})", avail.join(", "))
                    })?,
                other => return Err(format!("cannot project `.{seg}` on {}", other.kind_name())),
            };
        }
        Ok(cur)
    }

    pub fn kind_name(&self) -> &'static str {
        match self {
            Value::Unit => "unit",
            Value::Num(_) => "number",
            Value::Bool(_) => "bool",
            Value::Struct(_) => "struct",
        }
    }

    pub fn as_num(&self) -> Result<f64, String> {
        match self {
            Value::Num(n) => Ok(*n),
            other => Err(format!("expected number, got {}", other.kind_name())),
        }
    }

    pub fn as_bool(&self) -> Result<bool, String> {
        match self {
            Value::Bool(b) => Ok(*b),
            other => Err(format!("expected bool, got {}", other.kind_name())),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Unit => f.write_str("()"),
            Value::Num(n) => write!(f, "{n}"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Struct(fields) => {
                f.write_str("{ ")?;
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{k}: {v}")?;
                }
                f.write_str(" }")
            }
        }
    }
}
