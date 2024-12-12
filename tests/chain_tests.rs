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
    assert_eq!(chain.get_head(), Some(hash1.as_str()));
    
    // Add second entry
    let hash2 = chain.add(json!({"count": 2}));
    assert_eq!(chain.get_head(), Some(hash2.as_str()));
    
    // Verify chain contents
    let full_chain = chain.get_full_chain();
    assert_eq!(full_chain.len(), 2);
    assert_eq!(full_chain[0].0, hash2);
    assert_eq!(full_chain[1].0, hash1);
}

#[test]
fn test_empty_chain() {
    let chain = HashChain::new();
    assert_eq!(chain.get_head(), None);
    assert!(chain.get_full_chain().is_empty());
}

#[test]
fn test_chain_ordering() {
    let mut chain = HashChain::new();
    
    // Add multiple entries and verify ordering
    let hash1 = chain.add(json!({"step": 1}));
    let hash2 = chain.add(json!({"step": 2}));
    let hash3 = chain.add(json!({"step": 3}));
    
    let full_chain = chain.get_full_chain();
    assert_eq!(full_chain.len(), 3);
    
    // Verify reverse chronological order
    assert_eq!(full_chain[0].0, hash3);
    assert_eq!(full_chain[1].0, hash2);
    assert_eq!(full_chain[2].0, hash1);
    
    // Verify parent relationships
    assert_eq!(full_chain[0].1.parent, Some(hash2));
    assert_eq!(full_chain[1].1.parent, Some(hash1));
    assert_eq!(full_chain[2].1.parent, None);
}
