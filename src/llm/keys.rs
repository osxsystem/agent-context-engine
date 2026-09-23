//! Spreading requests across a provider's API keys. Each request starts on the
//! next key round-robin; a failure moves it to the next key. When every key has
//! failed once, a second pass after a short pause retries only the keys that
//! did not report a rate limit, since waiting two seconds will not lift a
//! quota. The LLM client and the Jev reranker both send through this.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use anyhow::{Result, bail};
use tracing::warn;

/// The pause between the first pass over the keys and the retry pass.
const RETRY_PAUSE: Duration = Duration::from_secs(2);

/// How one attempt on one key failed.
pub enum Failure {
    /// Worth trying on another key.
    Retry(anyhow::Error),
    /// Must not be sent again, e.g. output already reached the caller.
    Stop(anyhow::Error),
}

impl From<anyhow::Error> for Failure {
    fn from(e: anyhow::Error) -> Self {
        Self::Retry(e)
    }
}

/// A request that failed on every key while at least one key reported a rate
/// limit. Without a limit the last error is returned as it was.
#[derive(Debug)]
pub struct RateLimited {
    /// Keys that answered with a rate limit.
    pub limited: usize,
    /// Keys configured.
    pub keys: usize,
    /// The final failure, which a key without a limit may have produced.
    pub last: anyhow::Error,
}

impl fmt::Display for RateLimited {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "rate-limited on {} of {} keys; last error: {:#}",
            self.limited, self.keys, self.last
        )
    }
}

impl std::error::Error for RateLimited {}

/// Whether an error is a rate limit (HTTP 429), which rules its key out of the
/// retry pass.
fn is_rate_limited(err: &anyhow::Error) -> bool {
    let msg = err.to_string();
    msg.contains("429")
        && (msg.contains("Too Many Requests")
            || msg.contains("RESOURCE_EXHAUSTED")
            || msg.contains("rate")
            || msg.contains("quota"))
}

/// A provider's keys plus the round-robin cursor that clones share.
#[derive(Clone)]
pub struct KeyRing {
    keys: Vec<String>,
    cursor: Arc<AtomicUsize>,
    pause: Duration,
}

impl KeyRing {
    /// A ring with its own cursor, starting on the first key.
    pub fn new(keys: Vec<String>) -> Self {
        Self {
            keys,
            cursor: Arc::new(AtomicUsize::new(0)),
            pause: RETRY_PAUSE,
        }
    }

    /// A ring whose cursor lives for the whole process, shared by every ring
    /// built from the same keys. Owners rebuilt for each query use this so
    /// rotation carries on across queries instead of restarting on key one.
    pub fn shared(keys: Vec<String>) -> Self {
        static CURSORS: LazyLock<Mutex<HashMap<Vec<String>, Arc<AtomicUsize>>>> =
            LazyLock::new(Mutex::default);
        let cursor = CURSORS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(keys.clone())
            .or_default()
            .clone();
        Self {
            keys,
            cursor,
            pause: RETRY_PAUSE,
        }
    }

    /// Replaces [`RETRY_PAUSE`] for this ring.
    pub fn with_pause(mut self, pause: Duration) -> Self {
        self.pause = pause;
        self
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Runs `attempt` with one key at a time until it succeeds, it fails with
    /// [`Failure::Stop`], or both passes are spent. Returns the last error,
    /// wrapped in [`RateLimited`] when any key reported a limit.
    /// `request_label` names the request in the warning logged for each failed
    /// key. The key is passed owned so the returned future borrows nothing from
    /// the ring, which keeps callers' futures `Send`. Any error converts into
    /// [`Failure::Retry`].
    pub async fn send<T, E, Fut>(
        &self,
        request_label: &str,
        mut attempt: impl FnMut(String) -> Fut,
    ) -> Result<T>
    where
        Fut: Future<Output = Result<T, E>>,
        E: Into<Failure>,
    {
        let n = self.keys.len();
        if n == 0 {
            bail!("no API key configured");
        }
        let start = self.cursor.fetch_add(1, Ordering::Relaxed) % n;
        let order = || (0..n).map(move |offset| (start + offset) % n);
        let mut rate_limited = vec![false; n];
        let mut last_err = None;

        for i in order() {
            match attempt(self.keys[i].clone()).await.map_err(Into::into) {
                Ok(v) => return Ok(v),
                Err(Failure::Stop(e)) => return Err(e),
                Err(Failure::Retry(e)) => {
                    rate_limited[i] = is_rate_limited(&e);
                    warn!(key_index = i, error = %e, "{request_label} failed — trying next key");
                    last_err = Some(e);
                }
            }
        }

        if !rate_limited.iter().all(|&limited| limited) {
            tokio::time::sleep(self.pause).await;
            for i in order().filter(|&i| !rate_limited[i]) {
                match attempt(self.keys[i].clone()).await.map_err(Into::into) {
                    Ok(v) => return Ok(v),
                    Err(Failure::Stop(e)) => return Err(e),
                    Err(Failure::Retry(e)) => last_err = Some(e),
                }
            }
        }

        let last = last_err.expect("every key was tried");
        let limited = rate_limited.iter().filter(|&&limited| limited).count();
        if limited == 0 {
            return Err(last);
        }
        Err(RateLimited {
            limited,
            keys: n,
            last,
        }
        .into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    fn ring(keys: &[&str]) -> KeyRing {
        KeyRing::new(keys.iter().map(|k| (*k).to_owned()).collect()).with_pause(Duration::ZERO)
    }

    fn limited() -> Failure {
        Failure::Retry(anyhow!("HTTP 429 Too Many Requests: slow down"))
    }

    /// Sends one request and returns the keys it tried, in order.
    async fn tried(
        ring: &KeyRing,
        mut outcome: impl FnMut(&str, usize) -> Result<(), Failure>,
    ) -> (Result<()>, Vec<String>) {
        let mut keys_tried = Vec::new();
        let result = ring
            .send("test", |key: String| {
                keys_tried.push(key.clone());
                std::future::ready(outcome(&key, keys_tried.len()))
            })
            .await;
        (result, keys_tried)
    }

    fn rate_limited(result: Result<()>) -> RateLimited {
        result
            .unwrap_err()
            .downcast::<RateLimited>()
            .expect("a RateLimited error")
    }

    #[tokio::test]
    async fn requests_start_on_successive_keys() {
        let ring = ring(&["a", "b", "c"]);
        let mut first_keys = Vec::new();
        for _ in 0..4 {
            let (result, tried) = tried(&ring, |_, _| Ok(())).await;
            assert!(result.is_ok());
            first_keys.push(tried[0].clone());
        }
        assert_eq!(first_keys, ["a", "b", "c", "a"]);
    }

    #[tokio::test]
    async fn clones_share_the_cursor() {
        let ring = ring(&["a", "b"]);
        let clone = ring.clone();
        let (_, first) = tried(&ring, |_, _| Ok(())).await;
        let (_, second) = tried(&clone, |_, _| Ok(())).await;
        assert_eq!([&first[0], &second[0]], ["a", "b"]);
    }

    #[tokio::test]
    async fn shared_rings_from_the_same_keys_continue_one_rotation() {
        let keys = || vec!["shared-a".to_owned(), "shared-b".to_owned()];
        let (_, first) = tried(&KeyRing::shared(keys()), |_, _| Ok(())).await;
        let (_, second) = tried(&KeyRing::shared(keys()), |_, _| Ok(())).await;
        let (_, other) = tried(&KeyRing::shared(vec!["shared-c".to_owned()]), |_, _| Ok(())).await;
        assert_eq!([&first[0], &second[0]], ["shared-a", "shared-b"]);
        assert_eq!(other, ["shared-c"], "other keys keep their own rotation");
    }

    #[tokio::test]
    async fn a_failed_key_moves_the_request_to_the_next() {
        let (result, tried) = tried(&ring(&["a", "b"]), |key, _| match key {
            "a" => Err(limited()),
            _ => Ok(()),
        })
        .await;
        assert!(result.is_ok());
        assert_eq!(tried, ["a", "b"]);
    }

    #[tokio::test]
    async fn the_retry_pass_skips_rate_limited_keys() {
        let (result, tried) = tried(&ring(&["a", "b", "c"]), |key, n| match (key, n) {
            ("a", _) => Err(limited()),
            (_, n) if n <= 3 => Err(anyhow!("HTTP 502 Bad Gateway").into()),
            _ => Ok(()),
        })
        .await;
        assert!(result.is_ok());
        assert_eq!(tried, ["a", "b", "c", "b"]);
    }

    #[tokio::test]
    async fn every_key_rate_limited_returns_the_limit_without_a_retry_pass() {
        let (result, tried) = tried(&ring(&["a", "b"]), |_, _| Err(limited())).await;
        let err = rate_limited(result);
        assert_eq!((err.limited, err.keys), (2, 2));
        assert!(is_rate_limited(&err.last));
        assert_eq!(tried, ["a", "b"]);
    }

    #[tokio::test]
    async fn a_limit_on_some_keys_is_reported_even_when_another_error_came_last() {
        let (result, _) = tried(&ring(&["a", "b"]), |key, _| match key {
            "a" => Err(limited()),
            _ => Err(anyhow!("HTTP 502 Bad Gateway").into()),
        })
        .await;
        let err = rate_limited(result);
        assert_eq!((err.limited, err.keys), (1, 2));
        assert_eq!(err.last.to_string(), "HTTP 502 Bad Gateway");
    }

    #[tokio::test]
    async fn exhausted_passes_return_the_last_error() {
        let (result, tried) = tried(&ring(&["a", "b"]), |key, n| {
            Err(anyhow!("{key} failed on attempt {n}").into())
        })
        .await;
        let err = result.unwrap_err();
        assert!(
            err.downcast_ref::<RateLimited>().is_none(),
            "no key was limited"
        );
        assert_eq!(err.to_string(), "b failed on attempt 4");
        assert_eq!(tried, ["a", "b", "a", "b"]);
    }

    #[tokio::test]
    async fn a_stop_failure_is_returned_without_trying_another_key() {
        let (result, tried) = tried(&ring(&["a", "b"]), |_, _| {
            Err(Failure::Stop(anyhow!("mid-stream")))
        })
        .await;
        assert_eq!(result.unwrap_err().to_string(), "mid-stream");
        assert_eq!(tried, ["a"]);
    }

    /// Output that reached the caller on the retry pass must not be sent again.
    #[tokio::test]
    async fn a_stop_failure_on_the_retry_pass_ends_the_request() {
        let (result, tried) = tried(&ring(&["a", "b"]), |_, n| match n {
            1 | 2 => Err(anyhow!("HTTP 502 Bad Gateway").into()),
            _ => Err(Failure::Stop(anyhow!("mid-stream"))),
        })
        .await;
        assert_eq!(result.unwrap_err().to_string(), "mid-stream");
        assert_eq!(tried, ["a", "b", "a"]);
    }

    #[tokio::test]
    async fn no_keys_is_an_error() {
        let (result, tried) = tried(&ring(&[]), |_, _| Ok(())).await;
        assert!(result.is_err());
        assert!(tried.is_empty());
    }
}
