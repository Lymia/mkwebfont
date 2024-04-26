use anyhow::Result;
use std::{future::Future, sync::Arc};
use tokio::task::JoinHandle;
use tracing::{debug, Instrument};

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
impl<T: Send + Sync + 'static> JoinSet<Vec<T>> {
    pub fn map_vec<A: Clone + Send + Sync + 'static>(
        &mut self,
        what: &str,
        vec: &[A],
        chunk_size: usize,
        func: impl Fn(&A) -> Result<T> + Send + Sync + 'static,
    ) {
        let func = Arc::new(func);
        let len = vec.len();
        for (i, chunk) in vec.chunks(chunk_size).enumerate() {
            let chunk = chunk.to_vec();
            let what = what.to_string();
            let func = func.clone();
            self.spawn(async move {
                let start = i * chunk_size;
                let end = start + chunk.len();
                debug!("Process chunks for '{what}': {start}..={end}/{len}");

                let mut result = Vec::new();
                for v in chunk {
                    result.push(func(&v)?);
                }
                Ok(result)
            });
        }
    }

    pub async fn join_vec(self) -> Result<Vec<T>> {
        let mut result = Vec::new();
        for join in self.joins {
            result.extend(join.await??)
        }
        Ok(result)
    }
}
