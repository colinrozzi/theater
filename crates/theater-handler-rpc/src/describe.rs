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
//!   exports: list<function>,     // functions under the `exports` grouping arena
//!   imports: list<function>,     // functions under the `imports` grouping arena
//!   types:   list<type-def>,     // referenced named types (arena + fn-local)
//! }
//! function = record { name: string (FQ = <interface-arena>.<fn>), interface: string,
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
//! type-def  = record { name: string, scope: string, type-params: list<string>, def: type-def-body }
//!   // scope = owning path: <interface-arena> (interface-level) or <interface>.<fn>
//!   // (function-local) — disambiguates same-named defs in the flat `types` table.
//! type-def-body = variant { alias(type-ref), record(list<field>),
//!                           variant(list<case>), enum(list<string>), flags(list<string>) }
//! field = record { name: string, type: type-ref }
//! case  = record { name: string, payload: type-ref }   // payload=unit when none
//! ```

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
///
/// `scope` is the owning path of this definition — the interface-arena name for
/// an interface-level def, or `<interface>.<fn>` for a function-local def. It
/// disambiguates same-named defs across interfaces/functions in the flat global
/// `types` table, so a scoped `ref`/TypePath resolves to the right definition.
pub fn typedef_to_value(td: &TypeDef, scope: &str) -> Value {
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
            ("scope", vstr(scope)),
            ("type-params", list_of_strings(&type_params)),
            ("def", body),
        ],
    )
}

/// `interface` is the NAME OF THE ARENA that contains the function, not
/// `Function.interface` — the embedded/decoded metadata leaves `Function.
/// interface` empty and carries the interface identity as the leaf arena's
/// name (e.g. `theater:simple/actor`). So the FQ callable name is
/// `<arena-name>.<fn-name>`.
pub fn function_to_value(f: &Function, interface: &str) -> Value {
    let fq = if interface.is_empty() {
        f.name.clone()
    } else {
        format!("{}.{}", interface, f.name)
    };
    // Function-local type defs are scoped to this function's FQ name.
    let local_scope = fq.clone();
    vrec(
        "function",
        vec![
            ("name", vstr(&fq)),
            ("interface", vstr(interface)),
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
                    f.types
                        .iter()
                        .map(|td| typedef_to_value(td, &local_scope))
                        .collect(),
                    ValueType::Record("type-def".to_string()),
                ),
            ),
        ],
    )
}

/// Direction of an interface in the arena tree (under the `exports`/`imports`
/// grouping arenas). Defaults to export if no such ancestor is seen.
#[derive(Clone, Copy, PartialEq)]
enum Dir {
    Export,
    Import,
}

/// Walk an arena node, qualifying each function by THIS arena's name and
/// classifying it by the nearest `exports`/`imports` ancestor. Named types
/// (arena-level and function-local) are collected into `types`.
fn walk_arena(
    arena: &Arena,
    dir: Dir,
    exports: &mut Vec<Value>,
    imports: &mut Vec<Value>,
    types: &mut Vec<Value>,
) {
    // The grouping arenas re-key the direction for their subtree.
    let dir = match arena.name.as_str() {
        "exports" => Dir::Export,
        "imports" => Dir::Import,
        _ => dir,
    };
    for td in &arena.types {
        // Interface-level def: scoped to this arena (interface) name.
        types.push(typedef_to_value(td, &arena.name));
    }
    for f in &arena.functions {
        let v = function_to_value(f, &arena.name);
        match dir {
            Dir::Export => exports.push(v),
            Dir::Import => imports.push(v),
        }
        // Function-local def: scoped to <interface>.<fn>.
        let fscope = if arena.name.is_empty() {
            f.name.clone()
        } else {
            format!("{}.{}", arena.name, f.name)
        };
        for td in &f.types {
            types.push(typedef_to_value(td, &fscope));
        }
    }
    for child in &arena.children {
        walk_arena(child, dir, exports, imports, types);
    }
}

/// Top-level: an actor's decoded metadata → the `actor-description` value.
///
/// The decoded arena is `package → {exports, imports} → <interface> →
/// functions`; each function is qualified by its containing interface-arena's
/// name and placed under exports/imports by that grouping. Named types live on
/// the functions (function-local in the embedded form) and in arena `types`.
pub fn describe_metadata(md: &MetadataWithHashes) -> Value {
    let mut exports = Vec::new();
    let mut imports = Vec::new();
    let mut types = Vec::new();
    // The top arena is a neutral container; its `exports`/`imports` children set
    // the direction. Default to export for any functions outside that grouping.
    walk_arena(
        &md.arena,
        Dir::Export,
        &mut exports,
        &mut imports,
        &mut types,
    );
    vrec(
        "actor-description",
        vec![
            (
                "exports",
                vlist(exports, ValueType::Record("function".to_string())),
            ),
            (
                "imports",
                vlist(imports, ValueType::Record("function".to_string())),
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
        let v = typedef_to_value(&td, "theater:simple/shapes");
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
        let e = typedef_to_value(
            &TypeDef::Enum {
                name: "color".to_string(),
                cases: vec!["red".to_string(), "green".to_string()],
            },
            "test",
        );
        assert_eq!(case_name(rec_field(&e, "def")), "enum");
        let fl = typedef_to_value(
            &TypeDef::Flags {
                name: "perms".to_string(),
                flags: vec!["read".to_string(), "write".to_string()],
            },
            "test",
        );
        assert_eq!(case_name(rec_field(&fl, "def")), "flags");
    }

    #[test]
    fn same_name_defs_in_different_scopes_stay_distinct() {
        // Two DIFFERENT record shapes sharing the name "thing" in two scopes —
        // the scope field keeps their global-table entries distinguishable so a
        // scoped ref resolves to the right one.
        let a = typedef_to_value(
            &TypeDef::Record {
                name: "thing".to_string(),
                type_params: vec![],
                fields: vec![Field {
                    name: "x".to_string(),
                    ty: Type::U8,
                }],
            },
            "iface/a",
        );
        let b = typedef_to_value(
            &TypeDef::Record {
                name: "thing".to_string(),
                type_params: vec![],
                fields: vec![Field {
                    name: "y".to_string(),
                    ty: Type::String,
                }],
            },
            "iface/b",
        );
        assert_eq!(rec_field(&a, "name"), rec_field(&b, "name"), "same name");
        assert_ne!(
            rec_field(&a, "scope"),
            rec_field(&b, "scope"),
            "scope must distinguish same-named defs"
        );
        assert_eq!(
            rec_field(&a, "scope"),
            &Value::String("iface/a".to_string())
        );
        assert_eq!(
            rec_field(&b, "scope"),
            &Value::String("iface/b".to_string())
        );
    }

    #[test]
    fn function_fq_name_and_params_ordered() {
        let f = Function {
            name: "init".to_string(),
            // Empty, like the decoded/embedded form — the interface identity
            // comes from the containing arena name (the fn arg), not this field.
            interface: String::new(),
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
        let v = function_to_value(&f, "theater:simple/actor");
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
