use lazy_static::lazy_static;

#[derive(Clone)]
pub struct LockEvent {
    pub lock_name: String,
    pub status: String,
}

lazy_static! {
    pub static ref LOCK_EVENT_CHANNEL: (tokio::sync::broadcast::Sender<LockEvent>, tokio::sync::broadcast::Receiver<LockEvent>) = tokio::sync::broadcast::channel(1000);
}
