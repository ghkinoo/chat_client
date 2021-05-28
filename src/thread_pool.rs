use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;

type Job = Box<dyn FnOnce() + Send + 'static>;

// A good example of using a fancier enum to allow for additional information to be passed along.  We can pattern
// match to get the value of the Job in NewJob.
enum Message {
    NewJob(Job),
    Terminate,
}

pub struct ThreadPool {
    workers: Vec<Worker>,
    sender: mpsc::Sender<Message>,
}

impl ThreadPool {
    pub fn new(size: usize) -> ThreadPool {
        assert!(size > 0);

        let (sender, receiver) = mpsc::channel();
        let receiver = Arc::new(Mutex::new(receiver));

        let mut workers = Vec::with_capacity(size);
        for id in 0..size {
            workers.push(Worker::new(id, receiver.clone()));
        }

        ThreadPool { workers, sender }
    }

    pub fn execute<T>(&self, func: T)
    where
        T: FnOnce() + Send + 'static,
    {
        let job = Message::NewJob(Box::new(func));

        self.sender.send(job).unwrap()
    }
}

// This is essentially a destructor implementation.  When the threadpool leaves scope it will run the drop function
// and shut everything down.
impl Drop for ThreadPool {
    fn drop(&mut self) {
        println!("Sending terminate to all workers");

        // One of those tricks with concurrency, we can guarantee that the terminate message is the last message any
        // of our workers will get, so we don't need to worry about one thread consuming multiple terminate messages
        for _ in &mut self.workers {
            self.sender.send(Message::Terminate).unwrap();
        }

        // We used an option here, because we must take ownership in order to join the thread.  Option allows us to
        // swap the Some value in our worker with a None value.
        for worker in &mut self.workers {
            // If for whatever reason we had already processed this worker, this pattern match would fail (it would
            // be None instead of Some(thread)).
            if let Some(thread) = worker.thread.take() {
                // Join is actually defined as join(self) instead of join(&self).  This means it will consume the
                // variable it is called on, not allowing us to use it again.
                thread.join().unwrap();

                // This would be a compile error
                // thread.join().unwrap();
            }

            println!("Shutdown worker {}", worker.id);
        }
    }
}

struct Worker {
    id: usize,
    thread: Option<thread::JoinHandle<()>>,
}

impl Worker {
    fn new(id: usize, receiver: Arc<Mutex<mpsc::Receiver<Message>>>) -> Worker {
        // Really simple message loop, a message is either a job to execute or a termination.
        let thread = thread::spawn(move || loop {
            let message = receiver.lock().unwrap().recv().unwrap();

            match message {
                Message::NewJob(job) => {
                    println!("Worker {} got a job; executing.", id);

                    job();

                    println!("Worker {} finished job.", id);
                }
                Message::Terminate => {
                    println!("Worker {} terminating", id);

                    break;
                }
            }
        });

        Worker {
            id,
            thread: Some(thread),
        }
    }
}
