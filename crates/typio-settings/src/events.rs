use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use typio_client::Client;

#[derive(Debug)]
pub enum Event {
    Changed,
    Disconnected(String),
}

pub struct EventWorker {
    events: Receiver<Event>,
    stop: Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl EventWorker {
    pub fn start() -> Self {
        let (event_tx, events) = mpsc::channel();
        let (stop, stop_rx) = mpsc::channel();
        let thread = thread::spawn(move || {
            loop {
                if stop_rx.try_recv().is_ok() {
                    break;
                }
                let subscription = Client::connect().and_then(|client| client.subscribe(&[]));
                let mut subscription = match subscription {
                    Ok(subscription) => subscription,
                    Err(error) => {
                        let _ = event_tx.send(Event::Disconnected(error.to_string()));
                        if stop_rx.recv_timeout(Duration::from_secs(1)).is_ok() {
                            break;
                        }
                        continue;
                    }
                };

                loop {
                    if stop_rx.try_recv().is_ok() {
                        return;
                    }
                    match subscription.recv() {
                        Ok(Some(_)) => {
                            if event_tx.send(Event::Changed).is_err() {
                                return;
                            }
                        }
                        Ok(None) => {}
                        Err(error) => {
                            let _ = event_tx.send(Event::Disconnected(error.to_string()));
                            break;
                        }
                    }
                }
            }
        });

        Self {
            events,
            stop,
            thread: Some(thread),
        }
    }

    pub fn try_recv(&self) -> Result<Event, mpsc::TryRecvError> {
        self.events.try_recv()
    }
}

impl Drop for EventWorker {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
