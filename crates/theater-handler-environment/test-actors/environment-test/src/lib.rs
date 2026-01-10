#[allow(warnings)]
mod bindings;

use bindings::exports::theater::simple::actor::Guest;
use bindings::theater::simple::environment;

struct Component;

impl Guest for Component {
    fn init(_state: Option<Vec<u8>>) -> Result<(Option<Vec<u8>>,), String> {
        // Test 1: Check if PATH exists (should be true on most systems)
        let path_exists = environment::exists("PATH");

        // Test 2: Get the PATH variable
        let path_value = environment::get_var("PATH");

        // Test 3: Check a variable that likely doesn't exist
        let fake_exists = environment::exists("THEATER_TEST_NONEXISTENT_VAR_12345");

        // Test 4: Get a variable that doesn't exist
        let fake_value = environment::get_var("THEATER_TEST_NONEXISTENT_VAR_12345");

        // Test 5: Check HOME variable
        let home_exists = environment::exists("HOME");
        let home_value = environment::get_var("HOME");

        // Build result message
        let mut results = Vec::new();
        results.push(format!("PATH exists: {}", path_exists));
        results.push(format!("PATH value is Some: {}", path_value.is_some()));
        results.push(format!("Fake var exists: {}", fake_exists));
        results.push(format!("Fake var is None: {}", fake_value.is_none()));
        results.push(format!("HOME exists: {}", home_exists));
        results.push(format!("HOME value is Some: {}", home_value.is_some()));

        // Verify expected results
        let all_passed = path_exists
            && path_value.is_some()
            && !fake_exists
            && fake_value.is_none()
            && home_exists
            && home_value.is_some();

        if all_passed {
            results.push("Environment tests passed!".to_string());
        } else {
            results.push("Some environment tests failed!".to_string());
        }

        let result_str = results.join("\n");
        Ok((Some(result_str.into_bytes()),))
    }
}

bindings::export!(Component with_types_in bindings);
