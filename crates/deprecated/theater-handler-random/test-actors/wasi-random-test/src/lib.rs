mod bindings;

use bindings::wasi::random::random;

struct Component;

impl bindings::exports::theater::simple::actor::Guest for Component {
    fn init(_state: Option<Vec<u8>>) -> Result<(Option<Vec<u8>>,), String> {
        // Test 1: get-random-bytes with small size
        let bytes_10 = random::get_random_bytes(10);
        assert_eq!(bytes_10.len(), 10, "Should generate exactly 10 bytes");

        // Test 2: get-random-bytes with different size
        let bytes_32 = random::get_random_bytes(32);
        assert_eq!(bytes_32.len(), 32, "Should generate exactly 32 bytes");

        // Test 3: get-random-u64
        let _rand_u64_1 = random::get_random_u64();
        let _rand_u64_2 = random::get_random_u64();
        // Just verify it returns without error (any u64 is valid)
        // Statistically should be different (though not guaranteed)

        // Test 4: Verify randomness (bytes should not all be zeros)
        let bytes_check = random::get_random_bytes(100);
        let all_zeros = bytes_check.iter().all(|&b| b == 0);
        assert!(!all_zeros, "Random bytes should not all be zero");

        Ok((Some(b"WASI random tests passed!".to_vec()),))
    }
}

bindings::export!(Component with_types_in bindings);
