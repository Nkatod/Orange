use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;

use channels::{LOCK_EVENT_CHANNEL, LockEvent};

use crate::channels;
use crate::dependencies::Dependencies;

pub const NUM_SHARDS: usize = 64;

type Shard = Arc<RwLock<HashMap<String, ()>>>;
pub type Shards = Vec<Shard>;


fn get_shard<'a>(key: &str, dependencies: &'a Dependencies) -> &'a Shard {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    let hash = hasher.finish();
    &dependencies.locks[(hash % (NUM_SHARDS as u64)) as usize]
}

pub async fn acquire_lock(lock_name: String, ttl: u64, dependencies: Arc<Dependencies>) -> bool {
    let mut locks = get_shard(&lock_name, &dependencies).write().await;
    if locks.contains_key(&lock_name) {
        return false;
    }
    locks.insert(lock_name.clone(), ());
    drop(locks);

    tokio::spawn(async move {
        release_lock_after_delay(lock_name.clone(), ttl, Arc::clone(&dependencies)).await;
    });

    return true;
}

pub async fn has_lock(lock_name: String, dependencies: Arc<Dependencies>) -> bool {
    let locks = get_shard(&lock_name, &dependencies).read().await;
    return locks.contains_key(&lock_name);
}

async fn release_lock_after_delay(lock_name: String, ttl: u64, dependencies: Arc<Dependencies>) {
    tokio::time::sleep(Duration::from_millis(ttl)).await;
    let mut locks = get_shard(&lock_name, &dependencies).write().await;
    locks.remove(&lock_name);
    drop(locks);
    let (sender, _receiver) = &*LOCK_EVENT_CHANNEL;
    let _ = sender.send(LockEvent { lock_name: lock_name.clone(), status: "Released".to_string() });
}


#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::runtime::Runtime;
    use tokio_tungstenite::tungstenite::Message;

    use crate::initialize_dependencies;

    use super::*;

    #[test]
    fn test_acquire_lock() {
        let lock_name = "test_lock1".to_string();
        let ttl = 1000;
        let expected_message = Message::text(format!("Locked key {} for {} miliseconds", lock_name, ttl));

        let rt = Runtime::new().unwrap();
        let dependencies = Arc::new(initialize_dependencies());
        let actual_message = rt.block_on(acquire_lock(lock_name, ttl, Arc::clone(&dependencies)));

        assert_eq!(expected_message, actual_message);
    }

    #[test]
    fn test_acquire_already_locked() {
        let lock_name = "test_lock".to_string();
        let ttl = 1000;
        let expected_message = Message::text(format!("Key {} is already locked", lock_name));

        let rt = Runtime::new().unwrap();
        let dependencies = Arc::new(initialize_dependencies());
        rt.block_on(acquire_lock(lock_name.clone(), ttl, Arc::clone(&dependencies)));
        let actual_message = rt.block_on(acquire_lock(lock_name, ttl, Arc::clone(&dependencies)));

        assert_eq!(expected_message, actual_message);
    }

    #[tokio::test]
    async fn test_release_lock_after_delay() {
        let lock_name = "test_lock2".to_string();
        let ttl = 300;
        let dependencies = Arc::new(initialize_dependencies());

        acquire_lock(lock_name.clone(), ttl, Arc::clone(&dependencies)).await;
        tokio::time::sleep(Duration::from_millis(400)).await;
        let actual_message = acquire_lock(lock_name.clone(), ttl, Arc::clone(&dependencies)).await;
        let expected_message = Message::text(format!("Locked key {} for {} miliseconds", lock_name.clone(), ttl));

        assert_eq!(expected_message, actual_message);
    }
}