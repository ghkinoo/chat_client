use bus::Bus;
use bus::BusReader;
use core::time;
use popol::Events;
use popol::Sources;
use std::io;
use std::io::prelude::*;
use std::net::TcpListener;
use std::net::TcpStream;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;

use crate::thread_pool::ThreadPool;

#[derive(Eq, PartialEq, Clone)]
enum Source {
    Listener,
    Client,
}

pub struct ChatServer {}

impl ChatServer {
    pub fn run(&self) {
        let listener = TcpListener::bind("127.0.0.1:8080").unwrap();
        listener.set_nonblocking(true).unwrap();

        let mut sources = Sources::new();
        sources.register(Source::Listener, &listener, popol::interest::READ);

        let running = Arc::new(AtomicBool::new(true));

        let running_handler = running.clone();
        ctrlc::set_handler(move || {
            running_handler.clone().store(false, Ordering::SeqCst);
        })
        .unwrap();

        let mut events = Events::new();
        let pool = ThreadPool::new(4);

        let room_sender = Arc::new(Mutex::new(Bus::new(4)));
        let (message_sender, message_receiver) = mpsc::channel();

        let running_copy = running.clone();
        let message_receiver_ref = Arc::new(Mutex::new(message_receiver));
        let room_sender_ref = room_sender.clone();
        pool.execute(|| {
            ChatServer::handle_room(running_copy, message_receiver_ref, room_sender_ref)
        });

        let message_sender_ref = Arc::new(Mutex::new(message_sender));

        while running.load(Ordering::SeqCst) {
            // Wait for something to happen on our sources.
            sources.wait(&mut events).unwrap();

            for (key, _event) in events.iter() {
                match key {
                    Source::Listener => loop {
                        let (conn, _addr) = match listener.accept() {
                            Ok((conn, addr)) => (conn, addr),
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                            Err(_) => return,
                        };

                        let running = running.clone();
                        let room_receiver = room_sender.clone().lock().unwrap().add_rx();
                        let message_sender_ref = message_sender_ref.clone();

                        pool.execute(|| {
                            ChatServer::handle_client(
                                conn,
                                running,
                                room_receiver,
                                message_sender_ref,
                            );
                        });
                    },
                    _ => {}
                }
            }
        }
    }

    fn handle_room(
        running: Arc<AtomicBool>,
        message_receiver: Arc<Mutex<mpsc::Receiver<String>>>,
        room_sender: Arc<Mutex<Bus<String>>>,
    ) {
        println!("Room started");

        while running.load(Ordering::SeqCst) {
            match message_receiver.lock().unwrap().try_recv() {
                Ok(message) => {
                    room_sender.lock().unwrap().broadcast(message);
                }
                Err(_) => {}
            }
            thread::sleep(time::Duration::from_millis(10));
        }
    }

    fn handle_client(
        mut stream: TcpStream,
        running: Arc<AtomicBool>,
        mut room_receiver: BusReader<String>,
        message_sender: Arc<Mutex<mpsc::Sender<String>>>,
    ) {
        println!("Client connected");

        let mut user = String::from("Unknown");
        let mut buffer = [0; 1024];

        let mut sources = Sources::new();
        sources.register(Source::Client, &stream, popol::interest::ALL);
        let mut events = Events::new();

        while running.load(Ordering::SeqCst) {
            // Wait for something to happen on our sources.
            sources.wait(&mut events).unwrap();

            for (key, event) in events.iter() {
                match key {
                    Source::Client if event.readable => match stream.read(&mut buffer) {
                        Ok(bytes_read) => {
                            if bytes_read == 0 {
                                return;
                            }

                            let message = String::from_utf8(buffer[..bytes_read].to_vec()).unwrap();
                            let message = message.trim();

                            if message.starts_with("/user") {
                                user = String::from(message["/user".len()..].trim());
                            } else {
                                message_sender
                                    .lock()
                                    .unwrap()
                                    .send(format!("{}: {}", user, message.to_string()))
                                    .unwrap();
                            }
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                        Err(_) => return,
                    },
                    Source::Client if event.writable => match room_receiver.try_recv() {
                        Ok(message) => {
                            stream.write(message.as_bytes()).unwrap();
                            stream.flush().unwrap();
                        }
                        Err(_) => {}
                    },
                    _ => {}
                }
            }
        }

        // stream.set_nonblocking(false).unwrap();
        // let mut reader = BufReader::new(&stream);

        // let get = "GET / HTTP/1.1\r\n";
        // let sleep = "GET /sleep HTTP/1.1\r\n";

        // let mut buffer = String::new();
        // reader.read_line(&mut buffer).unwrap();

        // let (status_line, filename) = if buffer.starts_with(get) {
        //     ("HTTP/1.1 200 OK", "hello.html")
        // } else if buffer.starts_with(sleep) {
        //     thread::sleep(Duration::from_secs(5));
        //     ("HTTP/1.1 200 OK", "hello.html")
        // } else {
        //     ("HTTP/1.1 404 NOT FOUND", "404.html")
        // };

        // let contents = fs::read_to_string(filename).unwrap();
        // let response = format!(
        //     "{}\r\nContent-Length: {}\r\n\r\n{}",
        //     status_line,
        //     contents.len(),
        //     contents
        // );

        // stream.write(response.as_bytes()).unwrap();
        // stream.flush().unwrap();
    }
}
