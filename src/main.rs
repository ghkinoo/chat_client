mod chat_server;
mod thread_pool;

fn main() {
    let server = chat_server::ChatServer {};
    server.run();
}
