use tokio::net::TcpListener;
use server::accept_connection;
use std::sync::Arc;

mod server;
mod channels;
mod dependencies;

pub mod storage {
    pub mod locks;
}
use crate::dependencies::{initialize_dependencies};


#[tokio::main]
async fn main() {
    let try_socket = TcpListener::bind("127.0.0.1:9001").await;
    let listener = try_socket.expect("Failed to bind");
    println!("Listening on: {}", "127.0.0.1:9001");

    let dependencies = Arc::new(initialize_dependencies());

    while let Ok((stream, _)) = listener.accept().await {
        let dependencies_clone = Arc::clone(&dependencies);
        tokio::spawn(accept_connection(stream, dependencies_clone));
    }
}
