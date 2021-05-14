use chat_server::ThreadPool;
use popol::{Events, Sources};
use std::net::TcpListener;
use std::net::TcpStream;
use std::thread;
use std::time::Duration;
use std::{fs, io};
use std::{
    io::prelude::*,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

#[derive(Eq, PartialEq, Clone)]
enum Source {
    Listener,
}

fn main() {
    let listener = TcpListener::bind("127.0.0.1:8080").unwrap();
    listener.set_nonblocking(true).unwrap();

    let pool = ThreadPool::new(4);
    let mut sources = Sources::new();
    let mut events = Events::new();

    sources.register(Source::Listener, &listener, popol::interest::READ);

    let running = Arc::new(AtomicBool::new(true));

    let running_handler = running.clone();
    ctrlc::set_handler(move || {
        running_handler.store(false, Ordering::SeqCst);
    })
    .unwrap();

    while running.load(Ordering::SeqCst) {
        // Wait for something to happen on our sources.
        sources.wait(&mut events).unwrap();

        for (key, _event) in events.iter() {
            match key {
                Source::Listener => loop {
                    let (conn, _addr) = match listener.accept() {
                        Ok((conn, addr)) => (conn, addr),
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(_) => return,
                    };

                    pool.execute(|| {
                        handle_connection(conn);
                    });
                },
            }
        }
    }
}

fn handle_connection(mut stream: TcpStream) {
    let mut buffer = [0; 1024];

    stream.set_nonblocking(false).unwrap();
    stream.read(&mut buffer).unwrap();

    let get = b"GET / HTTP/1.1\r\n";
    let sleep = b"GET /sleep HTTP/1.1\r\n";

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
