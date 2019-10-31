use std::sync::mpsc::{channel, Sender};
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

/// Wait thread can either be scheduled a park or be ended.
pub enum WaitMessage {
    WakeIn {
        waker: futures::task::Waker,
        duration: Duration,
    },
    End,
}

/// Thread that will wake an underlying Waker after parking for a duration. Or can be killed to
/// never wake a Waker again.
pub struct WaitThread {
    sender: Sender<WaitMessage>,
    handle: Option<JoinHandle<()>>,
}

impl WaitThread {
    /// Create a new WaitThread
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
    /// Create a copy of the sender
    pub fn sender(&self) -> Sender<WaitMessage> {
        self.sender.clone()
    }
}

impl Default for WaitThread {
    fn default() -> Self {
        Self::new()
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
