use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::storage::locks::{NUM_SHARDS, Shards};

pub struct Dependencies {
    pub locks: Shards,
}

pub fn initialize_dependencies() -> Dependencies
{
    let dependencies = Dependencies {
        locks: (0..NUM_SHARDS).map(|_| Arc::new(RwLock::new(HashMap::new()))).collect(),
    };
    return dependencies;
}