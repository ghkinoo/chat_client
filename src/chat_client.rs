use popol::Events;
use popol::Sources;
use std::io;
use std::io::BufReader;
use std::io::prelude::*;
use std::net::TcpStream;
use std::os::unix::prelude::AsRawFd;
use std::process;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

#[derive(Eq, PartialEq, Clone)]
enum Source {
    StandardIn,
    Server,
}

pub struct ChatClient {}

impl ChatClient {
    pub fn run(
        &self,
        user: String,
        input: impl io::Read + AsRawFd + Send + 'static,
        output: impl io::Write + Send + 'static,
    ) {
        let (room_sender, room_receiver) = mpsc::channel();
        let room_sender = Arc::new(Mutex::new(room_sender));
        let room_receiver = Arc::new(Mutex::new(room_receiver));

        let room_thread = thread::spawn(|| ChatClient::handle_room(user, output, room_receiver));
        let input_thread = thread::spawn(|| ChatClient::handle_input(input, room_sender));

        input_thread.join().unwrap();
        room_thread.join().unwrap();
    }

    fn handle_room(
        user: String,
        mut output: impl io::Write,
        room_receiver: Arc<Mutex<mpsc::Receiver<String>>>,
    ) {
        let mut stream = match TcpStream::connect("127.0.0.1:8080") {
            Ok(stream) => stream,
            Err(err) => {
                print!("{}", err);
                match err.raw_os_error() {
                    Some(code) => process::exit(code),
                    None => process::exit(1),
                }
            }
        };

        // Before we go nonblocking, let's send an intro
        let intro = format!("/user {}", user);
        stream.write(intro.as_bytes()).unwrap();
        stream.set_nonblocking(true).unwrap();

        let mut buffer = [0; 1024];

        let mut sources = Sources::new();
        sources.register(Source::Server, &stream, popol::interest::ALL);

        let mut events = Events::new();

        loop {
            match sources.wait_timeout(&mut events, Duration::from_secs(5)) {
                Ok(_) => {}
                Err(err) if err.kind() == io::ErrorKind::TimedOut => {
                    output.write(b"Timed out\n").unwrap();
                    output.flush().unwrap();

                    process::exit(1);
                }
                Err(_) => {}
            }

            for (key, event) in events.iter() {
                match key {
                    Source::Server if event.readable => match stream.read(&mut buffer) {
                        Ok(bytes_read) => {
                            if bytes_read == 0 {
                                output.write(b"Server disconnected\n").unwrap();
                                output.flush().unwrap();

                                process::exit(1);
                            }

                            let message = String::from_utf8(buffer[..bytes_read].to_vec()).unwrap();
                            output.write(message.as_bytes()).unwrap();
                            output.write(b"\n").unwrap();
                            output.flush().unwrap();
                        }
                        Err(_) => {}
                    },
                    Source::Server if event.writable => {
                        match room_receiver.lock().unwrap().try_recv() {
                            Ok(message) => {
                                let message = message.trim();

                                stream.write(message.as_bytes()).unwrap();
                                stream.flush().unwrap();

                                if message == "/quit" {
                                    return;
                                }
                            }
                            Err(_) => {
                                thread::sleep(Duration::from_millis(10));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn handle_input(input: impl io::Read + AsRawFd, room_sender: Arc<Mutex<mpsc::Sender<String>>>) {
        let mut sources = Sources::new();
        sources.register(Source::StandardIn, &input, popol::interest::READ);

        let mut events = Events::new();
        let mut reader = BufReader::new(input);

        loop {
            // Wait for something to happen on our sources.
            sources.wait(&mut events).unwrap();

            for (key, _event) in events.iter() {
                match key {
                    Source::StandardIn => {
                        let mut one_line = String::new();
                        match reader.read_line(&mut one_line) {
                            Ok(_) => {
                                if one_line.trim() == "/quit" {
                                    room_sender.lock().unwrap().send(one_line).unwrap();
                                    return;
                                }
                                room_sender.lock().unwrap().send(one_line).unwrap();
                            }
                            Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                            Err(_) => return,
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
