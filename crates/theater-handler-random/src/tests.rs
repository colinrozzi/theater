use super::*;

#[test]
fn pact_parses_and_hashes() {
    let iface = random_interface();
    assert_eq!(iface.name(), "theater:simple/random");
    // hash is deterministic for the embedded pact
    assert_eq!(iface.hash(), random_interface().hash());
}

#[test]
fn config_default_is_one_mib() {
    assert_eq!(RandomHandlerConfig::default().max_bytes, 1024 * 1024);
}

#[test]
fn handler_name_and_no_exports() {
    let h = RandomHandler::new(RandomHandlerConfig::default());
    assert_eq!(h.name(), "random");
    assert_eq!(h.exports(), None);
    assert_eq!(h.imports(), Some(vec!["theater:simple/random".to_string()]));
}

#[test]
fn as_u32_accepts_bare_and_tupled() {
    assert_eq!(as_u32(&Value::U32(7)).unwrap(), 7);
    assert_eq!(as_u32(&Value::Tuple(vec![Value::U32(9)])).unwrap(), 9);
    assert_eq!(as_u32(&Value::U64(5)).unwrap(), 5);
    assert!(as_u32(&Value::String("x".into())).is_err());
}

#[test]
fn two_u64_parses_pair() {
    let v = Value::Tuple(vec![Value::U64(3), Value::U64(10)]);
    assert_eq!(two_u64(&v).unwrap(), (3, 10));
    assert!(two_u64(&Value::U64(1)).is_err());
}

#[test]
fn uniform_in_is_in_bounds() {
    // Many draws from real OS entropy must all land in [min, max).
    let (min, max) = (100u64, 137u64);
    let mut seen_min = false;
    let mut seen_hi = false;
    for _ in 0..10_000 {
        let r = uniform_in(min, max);
        assert!(r >= min && r < max, "r={r} out of [{min},{max})");
        if r == min {
            seen_min = true;
        }
        if r == max - 1 {
            seen_hi = true;
        }
    }
    // With 10k draws over 37 values, both endpoints should appear (coverage,
    // not a strict statistical test).
    assert!(seen_min && seen_hi, "endpoints not covered");
}

#[test]
fn uniform_in_singleton_span() {
    // span == 1 => always min.
    for _ in 0..100 {
        assert_eq!(uniform_in(42, 43), 42);
    }
}

#[test]
fn secure_u64_varies() {
    let a = secure_u64();
    let b = secure_u64();
    // Astronomically unlikely to collide; guards against a stuck source.
    assert_ne!(a, b);
}

#[test]
fn random_bytes_value_shape() {
    let v = bytes_value(vec![1, 2, 3]);
    match v {
        Value::List { elem_type, items } => {
            assert_eq!(elem_type, ValueType::U8);
            assert_eq!(items.len(), 3);
        }
        _ => panic!("expected a list"),
    }
}
