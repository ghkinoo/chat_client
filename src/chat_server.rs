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
use std::time::Duration;

use crate::thread_pool::ThreadPool;

// Derive tells the compiler to add these traits automatically for us.  Enums are a composite type, so this
// works as long as the variants within the enum also define these types (or can derive them).
#[derive(Eq, PartialEq, Clone)]
enum Source {
    Listener,
    Client,
}

// Our public struct, with no fields
pub struct ChatServer {}

impl ChatServer {
    // A typical method definition, takes self first, a string, and a couple objects that implement certain traits
    pub fn run(&self) {
        let listener = TcpListener::bind("127.0.0.1:8080").unwrap();
        listener.set_nonblocking(true).unwrap();

        // Sources and Events are part of popol which is a polling library.  Very similar (if not identical) to c
        // style polling of file descriptors.
        let mut sources = Sources::new();
        sources.register(Source::Listener, &listener, popol::interest::READ);

        // This is an atomic reference counted atomic bool.  The reference counting is so that we can point at the same
        // value among our threads.  The atomic bool is so we can read and write the value safely across threads.
        let running = Arc::new(AtomicBool::new(true));

        // ctrlc is actually a library to help us catch ctrlc.  This lets us setup a closure to change our boolean
        // that tells us if we're running or not
        let running_handler = running.clone();
        ctrlc::set_handler(move || {
            running_handler.store(false, Ordering::SeqCst);
        })
        .unwrap();

        let mut events = Events::new();
        let pool = ThreadPool::new(10);

        // We'll see a lot of wrapping in Arc and Mutex as we are sharing a lot things among our threads.  This wraps
        // our message broadcaster for updating our room chat.
        let room_sender = Arc::new(Mutex::new(Bus::new(4)));
        // This is a multiple producer, single consumer, channel for each of our clients to send incoming messages
        // to our room (to be broadcasted to everyone).
        let (message_sender, message_receiver) = mpsc::channel();

        // More wrapping and cloning as we spawn our room thread.  The thread pool is setup to automatically shut
        // things down when we exit, so we don't do any joins or any special handling other than exiting the threads
        let running_copy = running.clone();
        let message_receiver_ref = Arc::new(Mutex::new(message_receiver));
        let room_sender_ref = room_sender.clone();
        pool.execute(|| {
            ChatServer::handle_room(running_copy, message_receiver_ref, room_sender_ref)
        });

        // Wrapping
        let message_sender_ref = Arc::new(Mutex::new(message_sender));
        while running.load(Ordering::SeqCst) {
            // Wait for something to happen on our socket, just waiting for an attempted connection
            sources.wait(&mut events).unwrap();

            for (key, _event) in events.iter() {
                match key {
                    Source::Listener => loop {
                        let stream = match listener.accept() {
                            Ok((stream, _addr)) => stream,
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                            Err(_) => return,
                        };

                        // Clone our values again for threading
                        let running = running.clone();
                        let room_receiver = room_sender.clone().lock().unwrap().add_rx();
                        let message_sender_ref = message_sender_ref.clone();

                        // This will take our stream and process any messages until they disconnect
                        pool.execute(|| {
                            ChatServer::handle_client(
                                stream,
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

        // Room handling is pretty simple: we take any messages that we receive and simply broadcast them to all of our
        // clients (including the one who sent it).
        while running.load(Ordering::SeqCst) {
            match message_receiver.lock().unwrap().try_recv() {
                Ok(message) => {
                    room_sender.lock().unwrap().broadcast(message);
                }
                Err(_) => {
                    thread::sleep(time::Duration::from_millis(10));
                }
            }
        }
    }

    fn handle_client(
        mut stream: TcpStream,
        running: Arc<AtomicBool>,
        mut room_receiver: BusReader<String>,
        message_sender: Arc<Mutex<mpsc::Sender<String>>>,
    ) {
        println!("Client connected");

        let mut user = String::from("");
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
                            // Once again, a zero byte read is a disconnect
                            if bytes_read == 0 {
                                if user.len() > 0 {
                                    message_sender
                                        .lock()
                                        .unwrap()
                                        .send(format!("{} has left the room.", user))
                                        .unwrap();
                                }
                                return;
                            }

                            let message = String::from_utf8(buffer[..bytes_read].to_vec()).unwrap();
                            let message = message.trim();

                            // We handle a few special events here, and also require the client sets a name when
                            // before we start sending messages
                            if message.starts_with("/user") {
                                user = String::from(message["/user".len()..].trim());
                                message_sender
                                    .lock()
                                    .unwrap()
                                    .send(format!("{} has joined the room.", user))
                                    .unwrap();
                            } else if user.len() > 0 {
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
                        Err(_) => {
                            thread::sleep(Duration::from_millis(10));
                        }
                    },
                    _ => {}
                }
            }
        }
    }
}
