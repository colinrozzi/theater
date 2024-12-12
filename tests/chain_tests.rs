use pretty_assertions::assert_eq;
use serde_json::json;
use theater::chain::HashChain;

#[test]
fn test_chain_basic_operations() {
    let mut chain = HashChain::new();
    
    // Test initial state
    assert_eq!(chain.get_head(), None);
    
    // Add first entry
    let hash1 = chain.add(json!({"count": 1}));
    assert_eq!(chain.get_head(), Some(&hash1));
    
    // Add second entry
    let hash2 = chain.add(json!({"count": 2}));
    assert_eq!(chain.get_head(), Some(&hash2));
    
    // Verify chain contents
    let full_chain = chain.get_full_chain();
    assert_eq!(full_chain.len(), 2);
    assert_eq!(full_chain[0].0, hash2);
    assert_eq!(full_chain[1].0, hash1);
}
