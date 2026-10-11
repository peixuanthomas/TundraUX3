//! Async sockets driven on the existing watchdog worker; cancellation drops the
//! pending HTTP future, including waits for headers and individual body chunks.
use super::*;
use std::{future::Future, sync::Arc};

#[derive(Clone)]
pub(super) struct Client {
    inner: reqwest::Client,
    runtime: Arc<tokio::runtime::Runtime>,
    cancel: UpdateCancellation,
}

impl Client {
    pub(super) fn new(timeout: Duration, cancel: &UpdateCancellation) -> Result<Self, UpdateError> {
        cancel.check()?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let entered = runtime.enter();
        let inner = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(timeout)
            .build()
            .map_err(|error| UpdateError::new(error.to_string()))?;
        drop(entered);
        Ok(Self {
            inner,
            runtime: Arc::new(runtime),
            cancel: cancel.clone(),
        })
    }

    fn wait<T>(
        &self,
        future: impl Future<Output = Result<T, reqwest::Error>>,
    ) -> Result<T, UpdateError> {
        self.runtime.block_on(async {
            tokio::pin!(future);
            loop {
                self.cancel.check()?;
                tokio::select! {
                    biased;
                    _ = tokio::time::sleep(Duration::from_millis(20)) => {},
                    result = &mut future => {
                        self.cancel.check()?;
                        return result.map_err(|error| UpdateError::new(format!("HTTP request failed: {error}")));
                    }
                }
            }
        })
    }

    pub(super) fn get(&self, url: impl reqwest::IntoUrl) -> Request {
        Request {
            client: self.clone(),
            request: self.inner.get(url),
        }
    }
}

pub(super) struct Request {
    client: Client,
    request: reqwest::RequestBuilder,
}
impl Request {
    pub(super) fn send(self) -> Result<Response, UpdateError> {
        let inner = self.client.wait(async { self.request.send().await })?;
        Ok(Response {
            client: self.client,
            inner,
            pending: Vec::new(),
            offset: 0,
        })
    }
}

pub(super) struct Response {
    client: Client,
    inner: reqwest::Response,
    pending: Vec<u8>,
    offset: usize,
}
impl Response {
    pub(super) fn content_length(&self) -> Option<u64> {
        self.inner.content_length()
    }
    pub(super) fn status(&self) -> reqwest::StatusCode {
        self.inner.status()
    }
    pub(super) fn headers(&self) -> &reqwest::header::HeaderMap {
        self.inner.headers()
    }
    pub(super) fn text(mut self) -> Result<String, UpdateError> {
        let mut bytes = Vec::new();
        self.by_ref()
            .take(16 * 1024 * 1024)
            .read_to_end(&mut bytes)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
    pub(super) fn json<T: serde::de::DeserializeOwned>(self) -> Result<T, UpdateError> {
        serde_json::from_str(&self.text()?)
            .map_err(|error| UpdateError::new(format!("Invalid JSON: {error}")))
    }
}
impl Read for Response {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.client.cancel.check().map_err(io::Error::other)?;
        if buffer.is_empty() {
            return Ok(0);
        }
        while self.offset == self.pending.len() {
            match self
                .client
                .wait(self.inner.chunk())
                .map_err(io::Error::other)?
            {
                Some(bytes) => {
                    self.pending = bytes.to_vec();
                    self.offset = 0;
                }
                None => return Ok(0),
            }
        }
        let size = buffer.len().min(self.pending.len() - self.offset);
        buffer[..size].copy_from_slice(&self.pending[self.offset..self.offset + size]);
        self.offset += size;
        Ok(size)
    }
}
