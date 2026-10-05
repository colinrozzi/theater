//! Serialization of an actor's STATIC Pact metadata (the decoded CGRF arena)
//! into a structured dynamic [`Value`] — the payload of the `rpc.describe` verb.
//!
//! The shape is a convention on a dynamic `value` (like `rpc.exports`), so no
//! Pact type graph is needed; consumers decode it with packr's Value tools.
//! It mirrors the full `packr_abi::types::Type` / `TypeDef` system 1:1 so a
//! caller can construct a typed `rpc.call` argument without the actor's source.
//!
//! ## Value shape
//!
//! ```text
//! actor-description = record {
//!   exports: list<function>,     // callable surface
//!   imports: list<function>,     // (classification refined with the live path)
//!   types:   list<type-def>,     // referenced named types (arena + fn-local)
//! }
//! function = record { name: string (FQ), interface: string,
//!                     params: list<param>, results: list<type-ref>,
//!                     local-types: list<type-def> }
//! param    = record { name: string, type: type-ref }
//! type-ref = variant {                      // mirrors packr_abi Type, case order = tags here
//!   scalar(string),   // bool,u8..u64,s8..s64,f32,f64,char,string
//!   unit, value,                            // explicit no-value / dynamic
//!   list(type-ref), option(type-ref), set(type-ref),
//!   result(type-ref, type-ref), map(type-ref, type-ref), tuple(list<type-ref>),
//!   ref(type-path), app(type-path, list<type-ref>),   // named ref / generic application
//! }
//! type-path = record { segments: list<string>, absolute: bool }
//! type-def  = record { name: string, type-params: list<string>, def: type-def-body }
//! type-def-body = variant { alias(type-ref), record(list<field>),
//!                           variant(list<case>), enum(list<string>), flags(list<string>) }
//! field = record { name: string, type: type-ref }
//! case  = record { name: string, payload: type-ref }   // payload=unit when none
//! ```

// TEMPORARY: these are exercised by the unit tests (below) now, and wired into
// the `describe` host fn in the follow-up chunk that adds the GetActorMetadata
// plumbing. Remove this allow when `describe_metadata` is called from lib.rs.
#![allow(dead_code)]

use theater::pack_bridge::{
    Arena, Case, Field, Function, MetadataWithHashes, Param, Type, TypeDef, TypePath, Value,
    ValueType,
};

fn vstr(s: &str) -> Value {
    Value::String(s.to_string())
}

fn vrec(type_name: &str, fields: Vec<(&str, Value)>) -> Value {
    Value::Record {
        type_name: type_name.to_string(),
        fields: fields
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    }
}

fn vvar(case_name: &str, tag: usize, payload: Vec<Value>) -> Value {
    Value::Variant {
        type_name: "type-ref".to_string(),
        case_name: case_name.to_string(),
        tag,
        payload,
    }
}

/// A `list<_>` whose `elem_type` is inferred from the first item (so decoders
/// get a valid element annotation), falling back to `default_elem` when empty.
fn vlist(items: Vec<Value>, default_elem: ValueType) -> Value {
    let elem_type = items
        .first()
        .map(|v| v.infer_type())
        .unwrap_or(default_elem);
    Value::List { elem_type, items }
}

fn list_of_strings(ss: &[String]) -> Value {
    vlist(ss.iter().map(|s| vstr(s)).collect(), ValueType::String)
}

fn type_ref_list(ts: &[Type]) -> Value {
    vlist(
        ts.iter().map(type_to_value).collect(),
        ValueType::Variant("type-ref".to_string()),
    )
}

/// `packr_abi::types::Type` → `type-ref` value. Full 1:1 coverage; case order
/// is stable so consumers can key on `case_name`.
pub fn type_to_value(t: &Type) -> Value {
    let scalar = |name: &str| vvar("scalar", 0, vec![vstr(name)]);
    match t {
        Type::Unit => vvar("unit", 1, vec![]),
        Type::Value => vvar("value", 2, vec![]),
        Type::Bool => scalar("bool"),
        Type::U8 => scalar("u8"),
        Type::U16 => scalar("u16"),
        Type::U32 => scalar("u32"),
        Type::U64 => scalar("u64"),
        Type::S8 => scalar("s8"),
        Type::S16 => scalar("s16"),
        Type::S32 => scalar("s32"),
        Type::S64 => scalar("s64"),
        Type::F32 => scalar("f32"),
        Type::F64 => scalar("f64"),
        Type::Char => scalar("char"),
        Type::String => scalar("string"),
        Type::List(inner) => vvar("list", 3, vec![type_to_value(inner)]),
        Type::Option(inner) => vvar("option", 4, vec![type_to_value(inner)]),
        Type::Result { ok, err } => vvar("result", 5, vec![type_to_value(ok), type_to_value(err)]),
        Type::Tuple(items) => vvar("tuple", 6, vec![type_ref_list(items)]),
        Type::Map { key, value } => vvar("map", 7, vec![type_to_value(key), type_to_value(value)]),
        Type::Set(inner) => vvar("set", 8, vec![type_to_value(inner)]),
        Type::Ref(path) => vvar("ref", 9, vec![typepath_to_value(path)]),
        Type::App { path, args } => vvar(
            "app",
            10,
            vec![typepath_to_value(path), type_ref_list(args)],
        ),
    }
}

fn typepath_to_value(p: &TypePath) -> Value {
    vrec(
        "type-path",
        vec![
            ("segments", list_of_strings(&p.segments)),
            ("absolute", Value::Bool(p.absolute)),
        ],
    )
}

fn field_to_value(f: &Field) -> Value {
    vrec(
        "field",
        vec![("name", vstr(&f.name)), ("type", type_to_value(&f.ty))],
    )
}

fn case_to_value(c: &Case) -> Value {
    // `payload` is `Type::Unit` when the case carries no value — preserved as
    // the `unit` type-ref so order (= tag) and arity are unambiguous.
    vrec(
        "case",
        vec![
            ("name", vstr(&c.name)),
            ("payload", type_to_value(&c.payload)),
        ],
    )
}

fn param_to_value(p: &Param) -> Value {
    vrec(
        "param",
        vec![("name", vstr(&p.name)), ("type", type_to_value(&p.ty))],
    )
}

/// `packr_abi::types::TypeDef` → `type-def` value. Preserves `type-params`
/// (generics) and variant/enum/flags member ORDER (the only tag source).
pub fn typedef_to_value(td: &TypeDef) -> Value {
    let body_var = |case_name: &str, tag: usize, payload: Vec<Value>| Value::Variant {
        type_name: "type-def-body".to_string(),
        case_name: case_name.to_string(),
        tag,
        payload,
    };
    let (name, type_params, body): (&str, Vec<String>, Value) = match td {
        TypeDef::Alias {
            name,
            type_params,
            ty,
        } => (
            name,
            type_params.clone(),
            body_var("alias", 0, vec![type_to_value(ty)]),
        ),
        TypeDef::Record {
            name,
            type_params,
            fields,
        } => (
            name,
            type_params.clone(),
            body_var(
                "record",
                1,
                vec![vlist(
                    fields.iter().map(field_to_value).collect(),
                    ValueType::Record("field".to_string()),
                )],
            ),
        ),
        TypeDef::Variant {
            name,
            type_params,
            cases,
        } => (
            name,
            type_params.clone(),
            body_var(
                "variant",
                2,
                vec![vlist(
                    cases.iter().map(case_to_value).collect(),
                    ValueType::Record("case".to_string()),
                )],
            ),
        ),
        TypeDef::Enum { name, cases } => (
            name,
            Vec::new(),
            body_var("enum", 3, vec![list_of_strings(cases)]),
        ),
        TypeDef::Flags { name, flags } => (
            name,
            Vec::new(),
            body_var("flags", 4, vec![list_of_strings(flags)]),
        ),
    };
    vrec(
        "type-def",
        vec![
            ("name", vstr(name)),
            ("type-params", list_of_strings(&type_params)),
            ("def", body),
        ],
    )
}

/// Fully-qualified callable name: `<interface>.<name>` when the interface is
/// known, else the bare name.
fn fq_name(f: &Function) -> String {
    if f.interface.is_empty() {
        f.name.clone()
    } else {
        format!("{}.{}", f.interface, f.name)
    }
}

pub fn function_to_value(f: &Function) -> Value {
    vrec(
        "function",
        vec![
            ("name", vstr(&fq_name(f))),
            ("interface", vstr(&f.interface)),
            (
                "params",
                vlist(
                    f.params.iter().map(param_to_value).collect(),
                    ValueType::Record("param".to_string()),
                ),
            ),
            ("results", type_ref_list(&f.results)),
            (
                "local-types",
                vlist(
                    f.types.iter().map(typedef_to_value).collect(),
                    ValueType::Record("type-def".to_string()),
                ),
            ),
        ],
    )
}

/// Walk an arena tree, pushing every function and every named type (incl.
/// function-local defs) into the accumulators.
fn collect_arena(arena: &Arena, functions: &mut Vec<Value>, types: &mut Vec<Value>) {
    for td in &arena.types {
        types.push(typedef_to_value(td));
    }
    for f in &arena.functions {
        functions.push(function_to_value(f));
        for td in &f.types {
            types.push(typedef_to_value(td));
        }
    }
    for child in &arena.children {
        collect_arena(child, functions, types);
    }
}

/// Top-level: an actor's decoded metadata → the `actor-description` value.
///
/// NOTE: export/import classification (matching each interface's hash against
/// `export_hashes`/`import_hashes`) is refined on the live describe path; here
/// every function is surfaced under `exports` with its `interface` tag so the
/// shape + the full type coverage are exercised. Callers filter by `interface`.
pub fn describe_metadata(md: &MetadataWithHashes) -> Value {
    let mut functions = Vec::new();
    let mut types = Vec::new();
    collect_arena(&md.arena, &mut functions, &mut types);
    vrec(
        "actor-description",
        vec![
            (
                "exports",
                vlist(functions, ValueType::Record("function".to_string())),
            ),
            (
                "imports",
                vlist(Vec::new(), ValueType::Record("function".to_string())),
            ),
            (
                "types",
                vlist(types, ValueType::Record("type-def".to_string())),
            ),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case_name(v: &Value) -> &str {
        match v {
            Value::Variant { case_name, .. } => case_name,
            _ => "<not-variant>",
        }
    }

    fn rec_field<'a>(v: &'a Value, key: &str) -> &'a Value {
        match v {
            Value::Record { fields, .. } => fields
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, val)| val)
                .unwrap_or_else(|| panic!("no field {}", key)),
            _ => panic!("not a record"),
        }
    }

    #[test]
    fn scalars_and_specials_map_by_case() {
        assert_eq!(case_name(&type_to_value(&Type::Unit)), "unit");
        assert_eq!(case_name(&type_to_value(&Type::Value)), "value");
        for t in [
            Type::Bool,
            Type::U64,
            Type::S8,
            Type::F32,
            Type::Char,
            Type::String,
        ] {
            assert_eq!(case_name(&type_to_value(&t)), "scalar");
        }
        // scalar carries its name
        if let Value::Variant { payload, .. } = type_to_value(&Type::U32) {
            assert_eq!(payload, vec![Value::String("u32".to_string())]);
        } else {
            panic!("scalar must be a variant");
        }
    }

    #[test]
    fn compounds_map_including_map_set() {
        assert_eq!(
            case_name(&type_to_value(&Type::List(Box::new(Type::U8)))),
            "list"
        );
        assert_eq!(
            case_name(&type_to_value(&Type::Option(Box::new(Type::String)))),
            "option"
        );
        assert_eq!(
            case_name(&type_to_value(&Type::Set(Box::new(Type::U8)))),
            "set"
        );
        assert_eq!(
            case_name(&type_to_value(&Type::Map {
                key: Box::new(Type::String),
                value: Box::new(Type::U64)
            })),
            "map"
        );
        assert_eq!(
            case_name(&type_to_value(&Type::Result {
                ok: Box::new(Type::Unit),
                err: Box::new(Type::String)
            })),
            "result"
        );
        assert_eq!(
            case_name(&type_to_value(&Type::Tuple(vec![Type::U8, Type::Bool]))),
            "tuple"
        );
    }

    #[test]
    fn ref_preserves_typepath_scope() {
        let p = TypePath {
            segments: vec!["foo".to_string(), "bar".to_string()],
            absolute: true,
        };
        let v = type_to_value(&Type::Ref(p));
        assert_eq!(case_name(&v), "ref");
        if let Value::Variant { payload, .. } = v {
            let tp = &payload[0];
            assert_eq!(rec_field(tp, "absolute"), &Value::Bool(true));
            if let Value::List { items, .. } = rec_field(tp, "segments") {
                assert_eq!(items.len(), 2, "both path segments preserved");
            } else {
                panic!("segments must be a list");
            }
        }
    }

    #[test]
    fn app_generic_application_preserved() {
        let v = type_to_value(&Type::App {
            path: TypePath {
                segments: vec!["list".to_string()],
                absolute: false,
            },
            args: vec![Type::U8],
        });
        assert_eq!(case_name(&v), "app");
        if let Value::Variant { payload, .. } = v {
            assert_eq!(payload.len(), 2, "app carries path + args");
        }
    }

    #[test]
    fn variant_case_order_preserved_for_tags() {
        let td = TypeDef::Variant {
            name: "shape".to_string(),
            type_params: vec![],
            cases: vec![
                Case {
                    name: "circle".to_string(),
                    payload: Type::F64,
                },
                Case {
                    name: "none".to_string(),
                    payload: Type::Unit,
                },
            ],
        };
        let v = typedef_to_value(&td);
        assert_eq!(rec_field(&v, "name"), &Value::String("shape".to_string()));
        let def = rec_field(&v, "def");
        assert_eq!(case_name(def), "variant");
        if let Value::Variant { payload, .. } = def {
            if let Value::List { items, .. } = &payload[0] {
                // ORDER preserved: circle (tag 0) then none (tag 1)
                assert_eq!(
                    rec_field(&items[0], "name"),
                    &Value::String("circle".to_string())
                );
                assert_eq!(
                    rec_field(&items[1], "name"),
                    &Value::String("none".to_string())
                );
                // none's payload is the explicit `unit` type-ref
                assert_eq!(case_name(rec_field(&items[1], "payload")), "unit");
            } else {
                panic!("variant cases must be a list");
            }
        }
    }

    #[test]
    fn enum_and_flags_members_preserved() {
        let e = typedef_to_value(&TypeDef::Enum {
            name: "color".to_string(),
            cases: vec!["red".to_string(), "green".to_string()],
        });
        assert_eq!(case_name(rec_field(&e, "def")), "enum");
        let fl = typedef_to_value(&TypeDef::Flags {
            name: "perms".to_string(),
            flags: vec!["read".to_string(), "write".to_string()],
        });
        assert_eq!(case_name(rec_field(&fl, "def")), "flags");
    }

    #[test]
    fn function_fq_name_and_params_ordered() {
        let f = Function {
            name: "init".to_string(),
            interface: "theater:simple/actor".to_string(),
            types: vec![],
            params: vec![
                Param {
                    name: "config".to_string(),
                    ty: Type::Value,
                },
                Param {
                    name: "count".to_string(),
                    ty: Type::U32,
                },
            ],
            results: vec![Type::Result {
                ok: Box::new(Type::Unit),
                err: Box::new(Type::String),
            }],
        };
        let v = function_to_value(&f);
        assert_eq!(
            rec_field(&v, "name"),
            &Value::String("theater:simple/actor.init".to_string())
        );
        if let Value::List { items, .. } = rec_field(&v, "params") {
            assert_eq!(items.len(), 2);
            assert_eq!(
                rec_field(&items[0], "name"),
                &Value::String("config".to_string())
            );
            assert_eq!(
                rec_field(&items[1], "name"),
                &Value::String("count".to_string())
            );
        } else {
            panic!("params must be a list");
        }
    }
}
