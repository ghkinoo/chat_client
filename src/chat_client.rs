use popol::Events;
use popol::Sources;
use std::io;
use std::io::prelude::*;
use std::io::BufReader;
use std::net::TcpStream;
use std::os::unix::prelude::AsRawFd;
use std::process;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

// Derive tells the compiler to add these traits automatically for us.  Enums are a composite type, so this
// works as long as the variants within the enum also define these types (or can derive them).
#[derive(Eq, PartialEq, Clone)]
enum Source {
    Input,
    Server,
}

// Our public struct, with no fields
pub struct ChatClient {}

impl ChatClient {
    // A typical method definition, takes self first, a string, and a couple objects that implement certain traits
    pub fn run(
        &self,
        user: String,
        input: impl io::Read + AsRawFd + Send + 'static, // These are passed to closures and require a static lifetime
        output: impl io::Write + Send + 'static,         // Removing this is a compile error
    ) {
        // You'll see a lot of Arc and Mutex whenever we deal with shared values in threading, Arc is atomic reference
        // counting, and mutex is an old friend.
        let (room_sender, room_receiver) = mpsc::channel();
        let room_sender = Arc::new(Mutex::new(room_sender));
        let room_receiver = Arc::new(Mutex::new(room_receiver));

        // Since we pass input and output into these closures, this entire function, and even the application, could
        // finish before they do, which requires the lifetime of input and output be 'static.  The user field is
        // moved into the closure, so doesn't need anything special.
        let room_thread = thread::spawn(|| ChatClient::handle_room(user, output, room_receiver));
        let input_thread = thread::spawn(|| ChatClient::handle_input(input, room_sender));

        // This is a compile error
        // println!("{}", user);

        // If we exit normally we'll expect our input_thread to end first, which will signal the room_thread with
        // a message.  If we don't exit normally none of this will matter.
        input_thread.join().unwrap();
        room_thread.join().unwrap();
    }

    fn handle_room(
        user: String,
        mut output: impl io::Write,
        room_receiver: Arc<Mutex<mpsc::Receiver<String>>>,
    ) {
        // Connect to our server for any chat in our room, with some error handling in case the server isn't there.
        // Take note of the port, which gives you a good indicator of what tutorial I started with.
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

        // An undocumented limit of 1024 characters to our messages
        let mut buffer = [0; 1024];

        // Sources and Events are part of popol which is a polling library.  Very similar (if not identical) to c
        // style polling of file descriptors.
        let mut sources = Sources::new();
        sources.register(Source::Server, &stream, popol::interest::ALL);

        let mut events = Events::new();

        // Going to loop forever, or until an error, or until the server shuts down, or until we explicitly quit
        loop {
            // A timeout waiting for any read or write events on our TcpStream
            match sources.wait_timeout(&mut events, Duration::from_secs(5)) {
                Ok(_) => {}
                Err(err) if err.kind() == io::ErrorKind::TimedOut => {
                    output.write(b"Timed out\n").unwrap();
                    output.flush().unwrap();

                    // Just exiting instead of unwraveling our other thread
                    process::exit(1);
                }
                Err(_) => {}
            }

            // Itererate over our read and write events
            for (key, event) in events.iter() {
                match key {
                    Source::Server if event.readable => match stream.read(&mut buffer) {
                        Ok(bytes_read) => {
                            // Typical streams: if the stream is readable but returns 0 bytes it was closed on us
                            if bytes_read == 0 {
                                output.write(b"Server disconnected\n").unwrap();
                                output.flush().unwrap();

                                // Just exiting instead of unwraveling our other thread
                                process::exit(1);
                            }

                            // Write the message that was read in
                            let message = String::from_utf8(buffer[..bytes_read].to_vec()).unwrap();
                            output.write(message.as_bytes()).unwrap();
                            output.write(b"\n").unwrap();
                            output.flush().unwrap();
                        }
                        Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                        Err(_) => {}
                    },
                    Source::Server if event.writable => {
                        match room_receiver.lock().unwrap().try_recv() {
                            Ok(message) => {
                                let message = message.trim();
                                if message == "/quit" {
                                    return;
                                }

                                stream.write(message.as_bytes()).unwrap();
                                stream.flush().unwrap();
                            }
                            Err(_) => {
                                // Good ol' busy waiting
                                thread::sleep(Duration::from_millis(10));
                            }
                        }
                    },
                    _ => {}
                }
            }
        }
    }

    fn handle_input(input: impl io::Read + AsRawFd, room_sender: Arc<Mutex<mpsc::Sender<String>>>) {
        let mut sources = Sources::new();
        sources.register(Source::Input, &input, popol::interest::READ);

        let mut events = Events::new();
        let mut reader = BufReader::new(input);

        loop {
            // Wait for something to happen on our sources.
            sources.wait(&mut events).unwrap();

            for (key, _event) in events.iter() {
                match key {
                    Source::Input => {
                        let mut one_line = String::new();
                        match reader.read_line(&mut one_line) {
                            Ok(_) => {
                                // Have to do a clone here due to borrowing.  We can't check the
                                // trimmed value of one_line after sending it because mpsc::Sender ends up moving
                                // the String.  We could send a clone of the string instead and then check the original
                                // or do what I'm doing here.

                                // This is a compile error
                                // room_sender.lock().unwrap().send(one_line).unwrap();
                                room_sender.lock().unwrap().send(one_line.clone()).unwrap();
                                if one_line.trim() == "/quit" {
                                    return;
                                }
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
