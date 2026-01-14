//! # val-serde
//!
//! Serializable wrapper for wasmtime's component model `Val` type.
//!
//! This crate provides `SerializableVal`, a serde-compatible enum that mirrors
//! `wasmtime::component::Val`. This enables serializing component values to JSON
//! or other formats while preserving full type information.
//!
//! ## Example
//!
//! ```rust
//! use val_serde::SerializableVal;
//! use wasmtime::component::Val;
//!
//! // Convert Val to SerializableVal
//! let val = Val::U64(42);
//! let sv: SerializableVal = (&val).into();
//!
//! // Serialize to JSON (type is preserved!)
//! let json = serde_json::to_string(&sv).unwrap();
//! assert_eq!(json, r#"{"U64":42}"#);
//!
//! // Deserialize back
//! let sv2: SerializableVal = serde_json::from_str(&json).unwrap();
//! let val2: Val = sv2.into();
//! ```

use serde::{Deserialize, Serialize};
use wasmtime::component::Val;

/// A serializable representation of wasmtime's `Val` type.
///
/// This enum mirrors `wasmtime::component::Val` but implements `Serialize` and
/// `Deserialize`, making it suitable for recording/replaying component interactions
/// or storing component values.
///
/// The JSON representation preserves type information:
/// - `Val::U64(42)` becomes `{"U64": 42}`
/// - `Val::S32(-1)` becomes `{"S32": -1}`
/// - `Val::String("hi")` becomes `{"String": "hi"}`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SerializableVal {
    Bool(bool),
    S8(i8),
    U8(u8),
    S16(i16),
    U16(u16),
    S32(i32),
    U32(u32),
    S64(i64),
    U64(u64),
    Float32(f32),
    Float64(f64),
    Char(char),
    String(String),
    List(Vec<SerializableVal>),
    Record(Vec<(String, SerializableVal)>),
    Tuple(Vec<SerializableVal>),
    Variant(String, Option<Box<SerializableVal>>),
    Enum(String),
    Option(Option<Box<SerializableVal>>),
    Result(Result<Option<Box<SerializableVal>>, Option<Box<SerializableVal>>>),
    Flags(Vec<String>),
    /// Resources cannot be meaningfully serialized - they are store-specific handles.
    /// We store whether it was owned or borrowed for informational purposes.
    Resource { owned: bool },
}

impl From<&Val> for SerializableVal {
    fn from(val: &Val) -> Self {
        match val {
            Val::Bool(b) => SerializableVal::Bool(*b),
            Val::S8(n) => SerializableVal::S8(*n),
            Val::U8(n) => SerializableVal::U8(*n),
            Val::S16(n) => SerializableVal::S16(*n),
            Val::U16(n) => SerializableVal::U16(*n),
            Val::S32(n) => SerializableVal::S32(*n),
            Val::U32(n) => SerializableVal::U32(*n),
            Val::S64(n) => SerializableVal::S64(*n),
            Val::U64(n) => SerializableVal::U64(*n),
            Val::Float32(f) => SerializableVal::Float32(*f),
            Val::Float64(f) => SerializableVal::Float64(*f),
            Val::Char(c) => SerializableVal::Char(*c),
            Val::String(s) => SerializableVal::String(s.clone()),
            Val::List(items) => {
                SerializableVal::List(items.iter().map(SerializableVal::from).collect())
            }
            Val::Record(fields) => SerializableVal::Record(
                fields
                    .iter()
                    .map(|(k, v)| (k.clone(), SerializableVal::from(v)))
                    .collect(),
            ),
            Val::Tuple(items) => {
                SerializableVal::Tuple(items.iter().map(SerializableVal::from).collect())
            }
            Val::Variant(name, payload) => SerializableVal::Variant(
                name.clone(),
                payload.as_ref().map(|v| Box::new(SerializableVal::from(v.as_ref()))),
            ),
            Val::Enum(name) => SerializableVal::Enum(name.clone()),
            Val::Option(opt) => SerializableVal::Option(
                opt.as_ref().map(|v| Box::new(SerializableVal::from(v.as_ref()))),
            ),
            Val::Result(res) => SerializableVal::Result(match res {
                Ok(v) => Ok(v.as_ref().map(|v| Box::new(SerializableVal::from(v.as_ref())))),
                Err(v) => Err(v.as_ref().map(|v| Box::new(SerializableVal::from(v.as_ref())))),
            }),
            Val::Flags(flags) => SerializableVal::Flags(flags.clone()),
            Val::Resource(r) => SerializableVal::Resource { owned: r.owned() },
        }
    }
}

impl From<Val> for SerializableVal {
    fn from(val: Val) -> Self {
        SerializableVal::from(&val)
    }
}

impl From<SerializableVal> for Val {
    fn from(sv: SerializableVal) -> Self {
        match sv {
            SerializableVal::Bool(b) => Val::Bool(b),
            SerializableVal::S8(n) => Val::S8(n),
            SerializableVal::U8(n) => Val::U8(n),
            SerializableVal::S16(n) => Val::S16(n),
            SerializableVal::U16(n) => Val::U16(n),
            SerializableVal::S32(n) => Val::S32(n),
            SerializableVal::U32(n) => Val::U32(n),
            SerializableVal::S64(n) => Val::S64(n),
            SerializableVal::U64(n) => Val::U64(n),
            SerializableVal::Float32(f) => Val::Float32(f),
            SerializableVal::Float64(f) => Val::Float64(f),
            SerializableVal::Char(c) => Val::Char(c),
            SerializableVal::String(s) => Val::String(s),
            SerializableVal::List(items) => {
                Val::List(items.into_iter().map(Val::from).collect())
            }
            SerializableVal::Record(fields) => {
                Val::Record(fields.into_iter().map(|(k, v)| (k, Val::from(v))).collect())
            }
            SerializableVal::Tuple(items) => {
                Val::Tuple(items.into_iter().map(Val::from).collect())
            }
            SerializableVal::Variant(name, payload) => {
                Val::Variant(name, payload.map(|v| Box::new(Val::from(*v))))
            }
            SerializableVal::Enum(name) => Val::Enum(name),
            SerializableVal::Option(opt) => {
                Val::Option(opt.map(|v| Box::new(Val::from(*v))))
            }
            SerializableVal::Result(res) => Val::Result(match res {
                Ok(v) => Ok(v.map(|v| Box::new(Val::from(*v)))),
                Err(v) => Err(v.map(|v| Box::new(Val::from(*v)))),
            }),
            SerializableVal::Flags(flags) => Val::Flags(flags),
            SerializableVal::Resource { .. } => {
                // Resources cannot be reconstructed from serialized form.
                // This would need a store context and the original resource.
                // In practice, replay handlers should handle resources specially.
                panic!("Cannot convert SerializableVal::Resource back to Val without a store context")
            }
        }
    }
}

/// Trait for converting Rust primitives directly to `SerializableVal`.
///
/// This provides a convenient way for handlers to convert their typed results
/// to `SerializableVal` without first constructing a `Val`.
///
/// ## Example
///
/// ```rust
/// use val_serde::IntoSerializableVal;
///
/// let sv = 42u64.into_serializable_val();
/// let json = serde_json::to_string(&sv).unwrap();
/// assert_eq!(json, r#"{"U64":42}"#);
/// ```
pub trait IntoSerializableVal {
    fn into_serializable_val(self) -> SerializableVal;
}

impl IntoSerializableVal for bool {
    fn into_serializable_val(self) -> SerializableVal {
        SerializableVal::Bool(self)
    }
}

impl IntoSerializableVal for i8 {
    fn into_serializable_val(self) -> SerializableVal {
        SerializableVal::S8(self)
    }
}

impl IntoSerializableVal for u8 {
    fn into_serializable_val(self) -> SerializableVal {
        SerializableVal::U8(self)
    }
}

impl IntoSerializableVal for i16 {
    fn into_serializable_val(self) -> SerializableVal {
        SerializableVal::S16(self)
    }
}

impl IntoSerializableVal for u16 {
    fn into_serializable_val(self) -> SerializableVal {
        SerializableVal::U16(self)
    }
}

impl IntoSerializableVal for i32 {
    fn into_serializable_val(self) -> SerializableVal {
        SerializableVal::S32(self)
    }
}

impl IntoSerializableVal for u32 {
    fn into_serializable_val(self) -> SerializableVal {
        SerializableVal::U32(self)
    }
}

impl IntoSerializableVal for i64 {
    fn into_serializable_val(self) -> SerializableVal {
        SerializableVal::S64(self)
    }
}

impl IntoSerializableVal for u64 {
    fn into_serializable_val(self) -> SerializableVal {
        SerializableVal::U64(self)
    }
}

impl IntoSerializableVal for f32 {
    fn into_serializable_val(self) -> SerializableVal {
        SerializableVal::Float32(self)
    }
}

impl IntoSerializableVal for f64 {
    fn into_serializable_val(self) -> SerializableVal {
        SerializableVal::Float64(self)
    }
}

impl IntoSerializableVal for char {
    fn into_serializable_val(self) -> SerializableVal {
        SerializableVal::Char(self)
    }
}

impl IntoSerializableVal for String {
    fn into_serializable_val(self) -> SerializableVal {
        SerializableVal::String(self)
    }
}

impl IntoSerializableVal for &str {
    fn into_serializable_val(self) -> SerializableVal {
        SerializableVal::String(self.to_string())
    }
}

impl<T: IntoSerializableVal> IntoSerializableVal for Vec<T> {
    fn into_serializable_val(self) -> SerializableVal {
        SerializableVal::List(self.into_iter().map(|v| v.into_serializable_val()).collect())
    }
}

impl<T: IntoSerializableVal> IntoSerializableVal for Option<T> {
    fn into_serializable_val(self) -> SerializableVal {
        SerializableVal::Option(self.map(|v| Box::new(v.into_serializable_val())))
    }
}

impl<T: IntoSerializableVal, E: IntoSerializableVal> IntoSerializableVal for Result<T, E> {
    fn into_serializable_val(self) -> SerializableVal {
        SerializableVal::Result(match self {
            Ok(v) => Ok(Some(Box::new(v.into_serializable_val()))),
            Err(e) => Err(Some(Box::new(e.into_serializable_val()))),
        })
    }
}

/// Unit type serializes as an empty tuple
impl IntoSerializableVal for () {
    fn into_serializable_val(self) -> SerializableVal {
        SerializableVal::Tuple(vec![])
    }
}

/// 2-tuple implementation
impl<A: IntoSerializableVal, B: IntoSerializableVal> IntoSerializableVal for (A, B) {
    fn into_serializable_val(self) -> SerializableVal {
        SerializableVal::Tuple(vec![
            self.0.into_serializable_val(),
            self.1.into_serializable_val(),
        ])
    }
}

/// 3-tuple implementation
impl<A: IntoSerializableVal, B: IntoSerializableVal, C: IntoSerializableVal> IntoSerializableVal for (A, B, C) {
    fn into_serializable_val(self) -> SerializableVal {
        SerializableVal::Tuple(vec![
            self.0.into_serializable_val(),
            self.1.into_serializable_val(),
            self.2.into_serializable_val(),
        ])
    }
}

/// 4-tuple implementation
impl<A: IntoSerializableVal, B: IntoSerializableVal, C: IntoSerializableVal, D: IntoSerializableVal> IntoSerializableVal for (A, B, C, D) {
    fn into_serializable_val(self) -> SerializableVal {
        SerializableVal::Tuple(vec![
            self.0.into_serializable_val(),
            self.1.into_serializable_val(),
            self.2.into_serializable_val(),
            self.3.into_serializable_val(),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_primitives_roundtrip() {
        let cases: Vec<Val> = vec![
            Val::Bool(true),
            Val::Bool(false),
            Val::S8(-42),
            Val::U8(255),
            Val::S16(-1000),
            Val::U16(65535),
            Val::S32(-100000),
            Val::U32(100000),
            Val::S64(-9999999999),
            Val::U64(9999999999),
            Val::Float32(3.14),
            Val::Float64(2.71828),
            Val::Char('x'),
            Val::String("hello world".to_string()),
        ];

        for val in cases {
            let sv: SerializableVal = (&val).into();
            let json = serde_json::to_string(&sv).unwrap();
            let sv2: SerializableVal = serde_json::from_str(&json).unwrap();
            let val2: Val = sv2.into();

            // Compare debug representations since Val doesn't implement PartialEq
            assert_eq!(format!("{:?}", val), format!("{:?}", val2));
        }
    }

    #[test]
    fn test_list_roundtrip() {
        let val = Val::List(vec![Val::U64(1), Val::U64(2), Val::U64(3)]);
        let sv: SerializableVal = (&val).into();
        let json = serde_json::to_string(&sv).unwrap();

        assert_eq!(json, r#"{"List":[{"U64":1},{"U64":2},{"U64":3}]}"#);

        let sv2: SerializableVal = serde_json::from_str(&json).unwrap();
        let val2: Val = sv2.into();
        assert_eq!(format!("{:?}", val), format!("{:?}", val2));
    }

    #[test]
    fn test_record_roundtrip() {
        let val = Val::Record(vec![
            ("name".to_string(), Val::String("Alice".to_string())),
            ("age".to_string(), Val::U32(30)),
        ]);
        let sv: SerializableVal = (&val).into();
        let json = serde_json::to_string(&sv).unwrap();

        let sv2: SerializableVal = serde_json::from_str(&json).unwrap();
        let val2: Val = sv2.into();
        assert_eq!(format!("{:?}", val), format!("{:?}", val2));
    }

    #[test]
    fn test_option_roundtrip() {
        let some_val = Val::Option(Some(Box::new(Val::U64(42))));
        let none_val = Val::Option(None);

        for val in [some_val, none_val] {
            let sv: SerializableVal = (&val).into();
            let json = serde_json::to_string(&sv).unwrap();
            let sv2: SerializableVal = serde_json::from_str(&json).unwrap();
            let val2: Val = sv2.into();
            assert_eq!(format!("{:?}", val), format!("{:?}", val2));
        }
    }

    #[test]
    fn test_result_roundtrip() {
        let ok_val = Val::Result(Ok(Some(Box::new(Val::String("success".to_string())))));
        let err_val = Val::Result(Err(Some(Box::new(Val::String("error".to_string())))));

        for val in [ok_val, err_val] {
            let sv: SerializableVal = (&val).into();
            let json = serde_json::to_string(&sv).unwrap();
            let sv2: SerializableVal = serde_json::from_str(&json).unwrap();
            let val2: Val = sv2.into();
            assert_eq!(format!("{:?}", val), format!("{:?}", val2));
        }
    }

    #[test]
    fn test_variant_roundtrip() {
        let val = Val::Variant("some-case".to_string(), Some(Box::new(Val::U32(123))));
        let sv: SerializableVal = (&val).into();
        let json = serde_json::to_string(&sv).unwrap();
        let sv2: SerializableVal = serde_json::from_str(&json).unwrap();
        let val2: Val = sv2.into();
        assert_eq!(format!("{:?}", val), format!("{:?}", val2));
    }

    #[test]
    fn test_into_serializable_val_trait() {
        assert_eq!(
            42u64.into_serializable_val(),
            SerializableVal::U64(42)
        );
        assert_eq!(
            "hello".into_serializable_val(),
            SerializableVal::String("hello".to_string())
        );
        assert_eq!(
            vec![1u32, 2, 3].into_serializable_val(),
            SerializableVal::List(vec![
                SerializableVal::U32(1),
                SerializableVal::U32(2),
                SerializableVal::U32(3),
            ])
        );
    }

    #[test]
    fn test_json_format_is_typed() {
        // The key feature: JSON preserves type information
        assert_eq!(
            serde_json::to_string(&SerializableVal::U64(42)).unwrap(),
            r#"{"U64":42}"#
        );
        assert_eq!(
            serde_json::to_string(&SerializableVal::S32(42)).unwrap(),
            r#"{"S32":42}"#
        );
        assert_eq!(
            serde_json::to_string(&SerializableVal::U32(42)).unwrap(),
            r#"{"U32":42}"#
        );
        // Same value, different types, different JSON!
    }
}
