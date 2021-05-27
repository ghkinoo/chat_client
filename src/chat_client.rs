use popol::Events;
use popol::Sources;
use std::io;
use std::io::prelude::*;
use std::io::BufReader;
use std::net::TcpStream;
use std::thread;

#[derive(Eq, PartialEq, Clone)]
enum Source {
    Client,
}

pub struct ChatClient {}

impl ChatClient {
    pub fn run(&self) {
        let input_thread = thread::spawn(|| ChatClient::handle_input());
        input_thread.join().unwrap();
    }

    fn handle_input() {
        let stdin = io::stdin();

        let mut sources = Sources::new();
        sources.register(Source::Client, &stdin, popol::interest::READ);

        let mut events = Events::new();

        loop {
            // Wait for something to happen on our sources.
            sources.wait(&mut events).unwrap();

            for (key, _event) in events.iter() {
                match key {
                    Source::Client => loop {
                        let mut one_line = String::new();
                        match stdin.read_line(&mut one_line) {
                            Ok(_) => {
                                let one_line = one_line.trim();
                                match &one_line[..] {
                                    "/quit" => return,
                                    _ => println!("{}", one_line),
                                }
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                            Err(_) => return,
                        };
                    },
                }
            }
        }
    }

    fn handle_output() {
        let stream = TcpStream::connect("127.0.0.1:8080").unwrap();
        let mut reader = BufReader::new(&stream);

        let mut buffer = String::new();
        reader.read_line(&mut buffer).unwrap();

        println!("{}", buffer);
    }
}
