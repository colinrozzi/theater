use crate::chain::HashChain;
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub struct Store {
    pub chain: Arc<RwLock<HashChain>>,
}
