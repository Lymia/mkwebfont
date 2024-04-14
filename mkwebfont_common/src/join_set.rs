use anyhow::Result;
use std::future::Future;
use tokio::task::JoinHandle;
use tracing::Instrument;

pub struct JoinSet<T> {
    joins: Vec<JoinHandle<Result<T>>>,
}
impl<T: Send + Sync + 'static> JoinSet<T> {
    pub fn new() -> Self {
        JoinSet { joins: Vec::new() }
    }

    pub fn spawn(&mut self, fut: impl Future<Output = Result<T>> + Send + Sync + 'static) {
        self.joins.push(tokio::spawn(fut.in_current_span()));
    }

    pub async fn join(self) -> Result<Vec<T>> {
        let mut result = Vec::new();
        for join in self.joins {
            result.push(join.await??)
        }
        Ok(result)
    }
}
