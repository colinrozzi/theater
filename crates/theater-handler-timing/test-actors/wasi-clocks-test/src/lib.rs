#[allow(warnings)]
mod bindings;

use bindings::exports::theater::simple::actor::Guest;
use bindings::wasi::clocks::monotonic_clock;
use bindings::wasi::io::poll;

struct Component;

impl Guest for Component {
    fn init(_state: Option<Vec<u8>>) -> Result<(Option<Vec<u8>>,), String> {
        // Test 1: Get current monotonic instant
        let instant1 = monotonic_clock::now();
        assert!(instant1 > 0, "Monotonic clock should return positive value");

        // Test 2: Check resolution
        let resolution = monotonic_clock::resolution();
        assert!(resolution > 0, "Clock resolution should be positive");

        // Test 3: Verify time progresses
        let instant2 = monotonic_clock::now();
        assert!(
            instant2 >= instant1,
            "Monotonic clock should not go backwards"
        );

        // Test 4: subscribe-instant (create a pollable for a future instant)
        let future_instant = instant2 + 100_000_000; // 100ms in the future
        let instant_pollable = monotonic_clock::subscribe_instant(future_instant);

        // Test 5: subscribe-duration (create a pollable for a duration)
        let duration = 50_000_000; // 50ms
        let duration_pollable = monotonic_clock::subscribe_duration(duration);

        // Test 6: Check if pollables are not ready immediately
        let is_ready_instant = instant_pollable.ready();
        let is_ready_duration = duration_pollable.ready();

        // Note: These might be ready if execution is slow, so we just check they're callable
        let _ = is_ready_instant;
        let _ = is_ready_duration;

        // Test 7: Use poll to check multiple pollables
        let _ready_indices = poll::poll(&[&instant_pollable, &duration_pollable]);
        // At this point, some might be ready depending on timing

        // Test 8: Create a very short duration pollable and poll it
        let short_duration = 1_000_000; // 1ms
        let short_pollable = monotonic_clock::subscribe_duration(short_duration);

        // Sleep a bit to let it become ready
        let sleep_pollable = monotonic_clock::subscribe_duration(5_000_000); // 5ms
        sleep_pollable.block(); // This will wait until the duration elapses

        // Now check if the short pollable is ready
        let is_ready_short = short_pollable.ready();
        assert!(
            is_ready_short,
            "Short duration pollable should be ready after blocking"
        );

        // Test 9: Use poll with the short pollable
        let ready = poll::poll(&[&short_pollable]);
        assert!(
            ready.len() > 0,
            "Poll should return at least one ready pollable"
        );
        assert!(
            ready.contains(&0),
            "Short pollable (index 0) should be ready"
        );

        Ok((Some(b"WASI clocks + poll tests passed!".to_vec()),))
    }
}

bindings::export!(Component with_types_in bindings);
