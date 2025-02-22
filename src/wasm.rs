// ... [previous imports remain the same]

impl WasmActor {
    // ... [previous methods remain the same]

    pub fn get_memory_size(&self) -> usize {
        // Get the size of the actor state
        let state_size = self.actor_state.len();
        
        // Get the size of exports table
        let exports_size = self.exports.len() * std::mem::size_of::<ComponentExportIndex>();
        
        // Get the size of the store's data
        let store_size = self.actor_store.get_chain().iter()
            .map(|event| event.data.len())
            .sum::<usize>();

        // Sum up all memory usage
        state_size + exports_size + store_size
    }

    pub fn get_memory_stats(&self) -> MemoryStats {
        MemoryStats {
            state_size: self.actor_state.len(),
            exports_table_size: self.exports.len() * std::mem::size_of::<ComponentExportIndex>(),
            store_size: self.actor_store.get_chain().iter()
                .map(|event| event.data.len())
                .sum::<usize>(),
            num_exports: self.exports.len(),
            num_chain_events: self.actor_store.get_chain().len(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryStats {
    pub state_size: usize,
    pub exports_table_size: usize,
    pub store_size: usize,
    pub num_exports: usize,
    pub num_chain_events: usize,
}