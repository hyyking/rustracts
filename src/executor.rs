use std::sync::mpsc::{channel, Sender};
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

/// Message for a WaitThread
pub enum WaitMessage {
    WakeIn {
        waker: futures::task::Waker,
        duration: Duration,
    },
    End,
}

/// Thread that will wake an underlying waker after parking for a duration
pub struct WaitThread {
    sender: Sender<WaitMessage>,
    handle: Option<JoinHandle<()>>,
}

impl WaitThread {
    pub fn new() -> Self {
        let (sender, receiver) = channel();
        Self {
            sender,
            handle: Some(thread::spawn(move || loop {
                match receiver.recv() {
                    Ok(WaitMessage::End) => {
                        break;
                    }
                    Ok(WaitMessage::WakeIn { waker, duration }) => {
                        thread::sleep(duration);
                        waker.wake()
                    }
                    Err(e) => eprintln!("Error on WaitProcess channel: {}", e),
                };
            })),
        }
    }
    pub fn sender(&self) -> Sender<WaitMessage> {
        self.sender.clone()
    }
}

impl Drop for WaitThread {
    fn drop(&mut self) {
        self.sender.send(WaitMessage::End).unwrap();
        self.handle
            .take()
            .expect("WaitProcess cannot be killed twice")
            .join()
            .unwrap();
    }
}
