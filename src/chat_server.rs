use super::thread_pool::ThreadPool;
use popol::Events;
use popol::Sources;
use std::fs;
use std::io::prelude::*;
use std::io::BufReader;
use std::net::TcpListener;
use std::net::TcpStream;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[derive(Eq, PartialEq, Clone)]
enum Source {
    Listener,
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
            running_handler.store(false, Ordering::SeqCst);
        })
        .unwrap();

        let mut events = Events::new();
        let pool = ThreadPool::new(4);

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

                        pool.execute(|| {
                            ChatServer::handle_connection(conn);
                        });
                    },
                }
            }
        }
    }

    fn handle_connection(mut stream: TcpStream) {
        stream.set_nonblocking(false).unwrap();
        let mut reader = BufReader::new(&stream);

        let get = "GET / HTTP/1.1\r\n";
        let sleep = "GET /sleep HTTP/1.1\r\n";

        let mut buffer = String::new();
        reader.read_line(&mut buffer).unwrap();

        let (status_line, filename) = if buffer.starts_with(get) {
            ("HTTP/1.1 200 OK", "hello.html")
        } else if buffer.starts_with(sleep) {
            thread::sleep(Duration::from_secs(5));
            ("HTTP/1.1 200 OK", "hello.html")
        } else {
            ("HTTP/1.1 404 NOT FOUND", "404.html")
        };

        let contents = fs::read_to_string(filename).unwrap();
        let response = format!(
            "{}\r\nContent-Length: {}\r\n\r\n{}",
            status_line,
            contents.len(),
            contents
        );

        stream.write(response.as_bytes()).unwrap();
        stream.flush().unwrap();
    }
}
