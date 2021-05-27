mod chat_client;
mod chat_server;
mod thread_pool;
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    println!("{:?}", args);

    if args.len() != 2 {
        println!("You must specify client or server");
        return ();
    }

    match &args[1][..] {
        "server" => {
            let server = chat_server::ChatServer {};
            server.run()
        }
        "client" => {
            let client = chat_client::ChatClient {};
            client.run();
        }
        _ => println!("You must specify client or server"),
    }
}
