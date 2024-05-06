use futures_util::stream::{SplitSink,SplitStream};
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use crate::channels::{LOCK_EVENT_CHANNEL, LockEvent};
use crate::dependencies::Dependencies;
use crate::storage::locks::acquire_lock;
use tokio::sync::Mutex;

#[derive(Clone)]
struct Connection {
    read_mutex: Arc<Mutex<SplitStream<WebSocketStream<TcpStream>>>>,
    write_mutex: Arc<Mutex<SplitSink<WebSocketStream<TcpStream>, Message>>>,
}


pub async fn accept_connection(stream: TcpStream, dependencies: Arc<Dependencies>) {
    let addr = stream
        .peer_addr()
        .expect("connected streams should have a peer address");
    println!("Peer address: {}", addr);

    let ws_stream = accept_async(stream)
        .await
        .expect("Error during the websocket handshake occurred");

    println!("New WebSocket connection: {}", addr);

    // Разделяем поток TcpStream на чтение и запись
    let (write_half, read_half) = ws_stream.split();
    // Создаем Arc<Mutex<WebSocketStream>> для чтения
    let read_mutex = Arc::new(Mutex::new(read_half));
    // Создаем Arc<Mutex<WebSocketStream>> для записи
    let write_mutex = Arc::new(Mutex::new(write_half));

    let connection = Connection {
        read_mutex: Arc::clone(&read_mutex),
        write_mutex: Arc::clone(&write_mutex),
    };

    // Передаем структуру Connection в функцию обработки соединения
    let _ = handle_connection(connection, Arc::clone(&dependencies)).await;
}

async fn handle_connection(connection: Connection,
                           dependencies: Arc<Dependencies>) -> Result<(), Box<dyn std::error::Error>>
{
    // Захватываем мьютекс чтения, чтобы получить доступ к WebSocketStream
    let mut read_stream = connection.read_mutex.lock().await;

    // Запускаем цикл обработки входящих сообщений из WebSocketStream
    while let Some(msg) = read_stream.next().await {
        match msg {
            // Если сообщение успешно получено и является текстовым или бинарным
            Ok(msg) if msg.is_text() || msg.is_binary() => {
                // Преобразуем сообщение в текст
                let text = msg.to_text()?;
                // Разбиваем текст на части по пробелам
                let parts: Vec<&str> = text.split_whitespace().collect();

                // Обрабатываем команду на основе входящих частей сообщения,
                // передавая в качестве аргументов части сообщения, объект Connection
                // и клонированный Arc зависимостей
                process_command(parts, connection.clone(), Arc::clone(&dependencies)).await?;
                println!("New message: {}", text);

            }
            // Если произошла ошибка при чтении сообщения
            Err(e) => {
                // Выводим ошибку чтения сообщения
                eprintln!("Error reading message: {:?}", e);
            }
            // Игнорируем другие типы сообщений (например, пинги и т. д.)
            _ => (),
        }
    }
    // Возвращаем Ok, если обработка завершена успешно
    Ok(())
}


async fn process_command(
    parts: Vec<&str>,               // Входной вектор частей команды
    connection: Connection,         // Структура Connection, представляющая соединение
    dependencies: Arc<Dependencies>// Ссылка на зависимости, обернутые в Arc
) -> Result<(), Box<dyn std::error::Error>> // Возвращаемый тип Result, который может быть ошибкой или успехом
{
    // Проверяем, есть ли команда в первой части входного вектора parts
    if let Some(command) = parts.get(0) {
        // Если команда "lock"
        if *command == "lock" {
            // Обрабатываем команду "lock" с помощью функции handle_lock_command
            handle_lock_command(&parts[1..], connection.clone(), Arc::clone(&dependencies)).await?;
        } else {
            // Если команда неизвестна, обрабатываем неизвестную команду с помощью функции handle_unknown_command
            handle_unknown_command(connection.clone()).await?;
        }
    } else {
        // Если первая часть команды отсутствует, обрабатываем неизвестное сообщение с помощью функции handle_unknown_message
        handle_unknown_message(connection.clone()).await?;
    }

    // Возвращаем Ok, если обработка завершена успешно
    Ok(())
}


async fn handle_lock_command(args: &[&str],
                             connection: Connection,
                             dependencies: Arc<Dependencies>) -> Result<(), Box<dyn std::error::Error>>
{
    if args.len() == 2 {
        let lock_name = args[0].to_string();
        let ttl = args[1].parse().unwrap();
        let locked = acquire_lock(lock_name.clone(), ttl, Arc::clone(&dependencies)).await;
        if locked {
            let message = Message::text(format!("Locked key {} for {} miliseconds", lock_name.clone(), ttl.clone()));
            send_message(connection.clone(), message).await?;
        } else {
            let message = Message::text(format!("Key {} is already locked", lock_name.clone()));
            send_message(connection.clone(), message).await?;

            tokio::spawn(async move {
                let mut receiver = LOCK_EVENT_CHANNEL.0.subscribe();
                while let Ok(LockEvent { lock_name: event_lock_name, status }) = receiver.recv().await {
                    if event_lock_name == lock_name.clone() && status == "Released" {
                        break;
                    }
                }

                let message = Message::text(format!("Key {} is now unlocked", lock_name.clone()));
                let _ = send_message(connection.clone(), message).await;
            });
        }
    } else {
        send_message(connection.clone(), Message::text("Invalid lock command")).await?;
    }
    Ok(())
}

async fn handle_unknown_command(connection: Connection) -> Result<(), Box<dyn std::error::Error>> {
    let response = Message::text("Unknown command");
    return send_message(connection.clone(), response).await;
}

async fn handle_unknown_message(connection: Connection) -> Result<(), Box<dyn std::error::Error>> {
    let response = Message::text("Unknown message");
    return send_message(connection.clone(), response).await;
}


async fn send_message(
    connection: Connection,
    message: Message,
) -> Result<(), Box<dyn std::error::Error>> {
    // Захватываем мьютекс записи, чтобы получить доступ к WebSocketStream для отправки сообщения
    let mut write_stream = connection.write_mutex.lock().await;

    // Отправляем сообщение через WebSocketStream
    let _ = write_stream.send(message).await?; // Ожидаем отправку сообщения, обрабатывая возможные ошибки

    // Возвращаем Ok, если сообщение успешно отправлено
    return Ok(());
    // Освобождение мьютекса, захваченного с помощью метода lock().await, происходит автоматически,
    // когда объект MutexGuard, представляющий захват мьютекса, выходит из области видимости или уничтожается.
}


