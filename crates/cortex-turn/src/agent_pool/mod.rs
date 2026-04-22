pub mod delegation;
pub mod orchestration;
pub mod planner;

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

const DEFAULT_CHANNEL_SIZE: usize = 32;

#[derive(Debug)]
pub enum AgentPoolError {
    DuplicateWorker(String),
    WorkerNotFound(String),
    SendFailed(String),
}

impl std::fmt::Display for AgentPoolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateWorker(name) => {
                write!(f, "worker '{name}' already exists")
            }
            Self::WorkerNotFound(name) => write!(f, "worker '{name}' not found"),
            Self::SendFailed(name) => {
                write!(f, "failed to send message to worker '{name}'")
            }
        }
    }
}

impl std::error::Error for AgentPoolError {}

/// Result from a completed worker.
#[derive(Debug)]
pub struct WorkerResult {
    pub name: String,
    pub output: String,
}

/// Manages concurrent agent workers with message routing.
///
/// Each worker is a tokio task with its own mpsc channel for receiving messages.
/// Messages are routed by worker name.
pub struct AgentPool {
    senders: HashMap<String, mpsc::Sender<String>>,
    handles: Vec<(String, JoinHandle<String>)>,
}

impl Default for AgentPool {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentPool {
    #[must_use]
    pub fn new() -> Self {
        Self {
            senders: HashMap::new(),
            handles: Vec::new(),
        }
    }

    /// Spawn a named worker with a task function.
    ///
    /// The task function receives the worker name and a message receiver.
    /// It should process messages and return a result string.
    /// # Errors
    /// Returns `AgentPoolError::DuplicateWorker` if a worker with the same name already exists.
    pub fn spawn_worker<F, Fut>(
        &mut self,
        name: impl Into<String>,
        task_fn: F,
    ) -> Result<(), AgentPoolError>
    where
        F: FnOnce(String, mpsc::Receiver<String>) -> Fut + Send + 'static,
        Fut: Future<Output = String> + Send + 'static,
    {
        let name = name.into();
        if self.senders.contains_key(&name) {
            return Err(AgentPoolError::DuplicateWorker(name));
        }

        let (tx, rx) = mpsc::channel(DEFAULT_CHANNEL_SIZE);
        let worker_name = name.clone();
        let handle = tokio::spawn(async move { task_fn(worker_name, rx).await });

        self.senders.insert(name.clone(), tx);
        self.handles.push((name, handle));
        Ok(())
    }

    /// Route a message to a named worker.
    ///
    /// # Errors
    /// Returns `AgentPoolError` if the worker is not found or the channel send fails.
    pub async fn route_message(&self, to: &str, content: String) -> Result<(), AgentPoolError> {
        let sender = self
            .senders
            .get(to)
            .ok_or_else(|| AgentPoolError::WorkerNotFound(to.into()))?;

        sender
            .send(content)
            .await
            .map_err(|_| AgentPoolError::SendFailed(to.into()))
    }

    /// Wait for all workers to complete and collect results.
    ///
    /// Drops all senders first to signal workers that no more messages are coming.
    pub async fn wait_all(mut self) -> Vec<WorkerResult> {
        // Drop senders to close channels
        self.senders.clear();

        let mut results = Vec::new();
        for (name, handle) in self.handles {
            match handle.await {
                Ok(output) => results.push(WorkerResult { name, output }),
                Err(e) => results.push(WorkerResult {
                    name,
                    output: format!("worker panicked: {e}"),
                }),
            }
        }
        results
    }

    /// Number of registered workers.
    #[must_use]
    pub fn worker_count(&self) -> usize {
        self.senders.len()
    }
}

/// Helper to create a simple worker that collects all messages.
pub fn collecting_worker()
-> impl FnOnce(String, mpsc::Receiver<String>) -> Pin<Box<dyn Future<Output = String> + Send>> {
    |_name, mut rx| {
        Box::pin(async move {
            let mut messages = Vec::new();
            while let Some(msg) = rx.recv().await {
                messages.push(msg);
            }
            messages.join("\n")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn spawn_and_wait() {
        let mut pool = AgentPool::new();
        pool.spawn_worker("w1", |name, mut rx| async move {
            let mut out = format!("{name}: ");
            while let Some(msg) = rx.recv().await {
                out.push_str(&msg);
            }
            out
        })
        .unwrap();

        pool.route_message("w1", "hello".into()).await.unwrap();

        let results = pool.wait_all().await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "w1");
        assert!(results[0].output.contains("hello"));
    }

    #[tokio::test]
    async fn multiple_workers_parallel() {
        let mut pool = AgentPool::new();

        for i in 0..3 {
            let name = format!("worker-{i}");
            pool.spawn_worker(name, |name, _rx| async move { format!("{name} done") })
                .unwrap();
        }

        let results = pool.wait_all().await;
        assert_eq!(results.len(), 3);
        for r in &results {
            assert!(r.output.contains("done"));
        }
    }

    #[tokio::test]
    async fn duplicate_worker_rejected() {
        let mut pool = AgentPool::new();
        pool.spawn_worker("w1", |_, _| async { String::new() })
            .unwrap();
        let err = pool
            .spawn_worker("w1", |_, _| async { String::new() })
            .unwrap_err();
        assert!(matches!(err, AgentPoolError::DuplicateWorker(_)));
    }

    #[tokio::test]
    async fn route_to_nonexistent_worker() {
        let pool = AgentPool::new();
        let err = pool
            .route_message("ghost", "hello".into())
            .await
            .unwrap_err();
        assert!(matches!(err, AgentPoolError::WorkerNotFound(_)));
    }

    #[tokio::test]
    async fn collecting_worker_gathers_messages() {
        let mut pool = AgentPool::new();
        pool.spawn_worker("collector", collecting_worker()).unwrap();

        pool.route_message("collector", "msg1".into())
            .await
            .unwrap();
        pool.route_message("collector", "msg2".into())
            .await
            .unwrap();

        let results = pool.wait_all().await;
        assert_eq!(results[0].output, "msg1\nmsg2");
    }

    #[test]
    fn worker_count() {
        let pool = AgentPool::new();
        assert_eq!(pool.worker_count(), 0);
    }
}
