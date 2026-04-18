use std::sync::mpsc;
use std::thread;

use cortex_types::Event;

use crate::journal::{Journal, JournalError};

enum Command {
    Append {
        event: Event,
        reply: mpsc::Sender<Result<u64, JournalError>>,
    },
    Shutdown,
}

/// Background writer that serializes Journal appends through a single thread.
pub struct DbWriter {
    sender: mpsc::Sender<Command>,
    handle: Option<thread::JoinHandle<()>>,
}

impl DbWriter {
    #[must_use]
    pub fn new(journal: Journal) -> Self {
        let (tx, rx) = mpsc::channel::<Command>();
        let handle = thread::spawn(move || {
            for cmd in rx {
                match cmd {
                    Command::Append { event, reply } => {
                        let result = journal.append(&event);
                        let _ = reply.send(result);
                    }
                    Command::Shutdown => break,
                }
            }
        });
        Self {
            sender: tx,
            handle: Some(handle),
        }
    }

    /// Append an event via the background writer thread.
    ///
    /// # Errors
    /// Returns `JournalError` if the append fails or the worker is unreachable.
    pub fn append(&self, event: Event) -> Result<u64, JournalError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.sender
            .send(Command::Append {
                event,
                reply: reply_tx,
            })
            .map_err(|_| JournalError::Serialization("writer thread gone".into()))?;
        reply_rx
            .recv()
            .map_err(|_| JournalError::Serialization("writer reply lost".into()))?
    }

    /// Shut down the background writer, waiting for it to finish.
    pub fn shutdown(mut self) {
        let _ = self.sender.send(Command::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for DbWriter {
    fn drop(&mut self) {
        let _ = self.sender.send(Command::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cortex_types::{CorrelationId, Payload, TurnId};

    fn make_event() -> Event {
        Event::new(TurnId::new(), CorrelationId::new(), Payload::TurnStarted)
    }

    #[test]
    fn append_via_writer() {
        let journal = Journal::open_in_memory().unwrap();
        let writer = DbWriter::new(journal);
        let offset = writer.append(make_event()).unwrap();
        assert_eq!(offset, 1);
        writer.shutdown();
    }

    #[test]
    fn multiple_appends() {
        let journal = Journal::open_in_memory().unwrap();
        let writer = DbWriter::new(journal);
        for _ in 0..5 {
            writer.append(make_event()).unwrap();
        }
        writer.shutdown();
    }
}
