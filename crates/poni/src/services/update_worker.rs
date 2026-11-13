//! A sleeping HTTP worker for update checks and downloads.

use crate::services::http::{Client, MtaGuard};
use std::io;
use std::sync::{Arc, Condvar, Mutex};

const USER_AGENT: &str = concat!("Island/", env!("CARGO_PKG_VERSION"));

pub type Job = Box<dyn FnOnce(Result<&Client, &str>) + Send>;

pub struct UpdateWorker {
    shared: Arc<Shared>,
}

struct Shared {
    queue: Mutex<Queue>,
    ready: Condvar,
}

struct Queue {
    jobs: Vec<Job>,
    shutdown: bool,
}

impl UpdateWorker {
    pub fn start() -> io::Result<Self> {
        let shared = Arc::new(Shared { queue: Mutex::new(Queue { jobs: Vec::new(), shutdown: false }), ready: Condvar::new() });
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("poni-updates".into())
            .spawn(move || run(worker))
            .map_err(|e| io::Error::other(format!("could not start the update thread: {e}")))?;
        Ok(Self { shared })
    }

    pub fn run(&self, job: Job) {
        let mut queue = self.shared.queue.lock().unwrap_or_else(|p| p.into_inner());
        queue.jobs.push(job);
        self.shared.ready.notify_one();
    }
}

impl Drop for UpdateWorker {
    fn drop(&mut self) {
        let mut queue = self.shared.queue.lock().unwrap_or_else(|p| p.into_inner());
        queue.shutdown = true;
        self.shared.ready.notify_one();
    }
}

fn run(shared: Arc<Shared>) {
    let _com = MtaGuard::new();
    let mut client: Option<Client> = None;
    loop {
        let jobs = {
            let mut queue = shared.queue.lock().unwrap_or_else(|p| p.into_inner());
            while queue.jobs.is_empty() && !queue.shutdown {
                queue = shared.ready.wait(queue).unwrap_or_else(|p| p.into_inner());
            }
            if queue.shutdown {
                return;
            }
            std::mem::take(&mut queue.jobs)
        };
        for job in jobs {
            let mut why = String::new();
            if client.is_none() {
                match Client::new(USER_AGENT) {
                    Ok(ready) => client = Some(ready),
                    Err(e) => {
                        crate::pwarn!("updates: no HTTP client ({e})");
                        why = e.to_string();
                    }
                }
            }
            job(client.as_ref().ok_or(why.as_str()));
        }
    }
}
