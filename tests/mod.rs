// Main test module
pub mod common;
pub mod unit;
pub mod integration;

#[cfg(test)]
mod tests {
    // Import unit tests
    use crate::unit::chain_tests;
    use crate::unit::actor_store_tests;
    use crate::unit::actor_handle_tests;
    use crate::unit::store_tests;
    use crate::unit::messages_tests;
    
    // Integration tests will be added later
}