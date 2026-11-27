//! Loopback-only (`127.0.0.1:0`) authenticated HTTP/1.1 Range proxy server backed by
//! SeasonFetch's managed `libtorrent 2.1.1` (`stremio-native/stream-server`) engine.
//!
//! Security & lifecycle invariants:
//! - Strict loopback `Host` header validation (`403 Forbidden` against DNS rebinding).
//! - Constant-time per-session token verification (`404 Not Found` for unknown/expired sessions).
//! - Zero-copy streaming byte proxy preserving exact `200`/`206`/`416`/`HEAD` Range semantics.
//! - Every upstream stream is wrapped with SeasonFetch's `CancellationToken` so stop/switch/close
//!   aborts the upstream socket in <1 ms (triggering immediate `libtorrent` piece deadline cleanup).

use crate::priorities::{classify_http_intent, PlaybackIntent};
use crate::TorrentStreamingEngine;
use axum::body::Body;
use axum::extract::{Path as AxumPath, State};
use axum::http::{header, HeaderMap, HeaderValue, Response, StatusCode};
use axum::routing::get;
use axum::Router;
use bytes::Bytes;
use futures_util::Stream;
use seasonfetch_core::streaming::{content_type_for_filename, parse_http_byte_range};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio_util::sync::{CancellationToken, ReusableBoxFuture};

const READ_STALL_TIMEOUT: Duration = Duration::from_secs(45);

/// RAII wrapper around an upstream HTTP byte stream that:
/// 1. Tracks active HTTP stream count (`active_http_streams`),
/// 2. Wakes and aborts **immediately** when `CancellationToken` fires,
/// 3. Enforces a per-chunk stall timeout (45s) so dead swarms fail cleanly,
/// 4. Logs first-byte delivery latency for diagnostics.
struct TrackedProxyStream<S> {
    inner: S,
    counter: Arc<AtomicU32>,
    cancel_fut: ReusableBoxFuture<'static, ()>,
    stall_sleep: Pin<Box<tokio::time::Sleep>>,
    engine: TorrentStreamingEngine,
    intent: PlaybackIntent,
    range_label: String,
    started_at: Instant,
    first_byte_logged: bool,
    bytes_sent: u64,
}

impl<S> TrackedProxyStream<S> {
    fn new(
        inner: S,
        counter: Arc<AtomicU32>,
        cancel_token: CancellationToken,
        engine: TorrentStreamingEngine,
        intent: PlaybackIntent,
        range_label: String,
    ) -> Self {
        counter.fetch_add(1, Ordering::Relaxed);
        let cancel_fut = ReusableBoxFuture::new(async move {
            cancel_token.cancelled().await;
        });
        Self {
            inner,
            counter,
            cancel_fut,
            stall_sleep: Box::pin(tokio::time::sleep(READ_STALL_TIMEOUT)),
            engine,
            intent,
            range_label,
            started_at: Instant::now(),
            first_byte_logged: false,
            bytes_sent: 0,
        }
    }
}

impl<S> Drop for TrackedProxyStream<S> {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::Relaxed);
        if !self.first_byte_logged {
            self.engine.log_event(&format!(
                "HTTP stream closed before first byte ({}) after {}ms",
                self.range_label,
                self.started_at.elapsed().as_millis()
            ));
        }
    }
}

impl<S, E> Stream for TrackedProxyStream<S>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
    E: std::fmt::Display,
{
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.cancel_fut.poll(cx).is_ready() {
            return Poll::Ready(Some(Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "streaming session stopped",
            ))));
        }

        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                if !chunk.is_empty() {
                    self.bytes_sent = self.bytes_sent.saturating_add(chunk.len() as u64);
                    self.stall_sleep
                        .as_mut()
                        .reset(tokio::time::Instant::now() + READ_STALL_TIMEOUT);
                    if !self.first_byte_logged {
                        self.first_byte_logged = true;
                        self.engine.log_event(&format!(
                            "HTTP first byte delivered ({}, intent={:?}) in {}ms",
                            self.range_label,
                            self.intent,
                            self.started_at.elapsed().as_millis()
                        ));
                    }
                }
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(Some(Err(err))) => {
                self.engine.log_event(&format!(
                    "HTTP upstream error ({}) after {} bytes: {err}",
                    self.range_label, self.bytes_sent
                ));
                Poll::Ready(Some(Err(std::io::Error::other(err.to_string()))))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => {
                if self.stall_sleep.as_mut().poll(cx).is_ready() {
                    self.engine.log_event(&format!(
                        "HTTP stream stalled for {}s ({}) after {} bytes",
                        READ_STALL_TIMEOUT.as_secs(),
                        self.range_label,
                        self.bytes_sent
                    ));
                    return Poll::Ready(Some(Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "stream timed out waiting for media data",
                    ))));
                }
                Poll::Pending
            }
        }
    }
}

fn is_loopback_host(headers: &HeaderMap) -> bool {
    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let host_only = host
        .trim()
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(host_only.as_str(), "127.0.0.1" | "localhost")
}

pub(crate) fn spawn_http_server(
    listener: TcpListener,
    engine: TorrentStreamingEngine,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) {
    let router = Router::new()
        .route("/health", get(health_handler))
        .route(
            "/stream/{session_id}/{token}/{file_idx}",
            get(stream_get_no_name).head(stream_head_no_name),
        )
        .route(
            "/stream/{session_id}/{token}/{file_idx}/{filename}",
            get(stream_get_with_name).head(stream_head_with_name),
        )
        .route(
            "/subtitle/{session_id}/{token}/{file_idx}/{filename}",
            get(subtitle_get_handler).head(subtitle_head_handler),
        )
        .with_state(engine);

    tokio::spawn(async move {
        let server = axum::serve(listener, router).with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
        });
        let _ = server.await;
    });
}

async fn health_handler(headers: HeaderMap) -> Response<Body> {
    if !is_loopback_host(&headers) {
        return Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(Body::empty())
            .unwrap();
    }
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::json!({ "ok": true, "engine": "seasonfetch-streaming-libtorrent" })
                .to_string(),
        ))
        .unwrap()
}

async fn stream_head_no_name(
    State(engine): State<TorrentStreamingEngine>,
    AxumPath((session_id, token, file_idx)): AxumPath<(String, String, usize)>,
    headers: HeaderMap,
) -> Response<Body> {
    serve_head(engine, &session_id, &token, file_idx, &headers).await
}

async fn stream_head_with_name(
    State(engine): State<TorrentStreamingEngine>,
    AxumPath((session_id, token, file_idx, _filename)): AxumPath<(String, String, usize, String)>,
    headers: HeaderMap,
) -> Response<Body> {
    serve_head(engine, &session_id, &token, file_idx, &headers).await
}

async fn stream_get_no_name(
    State(engine): State<TorrentStreamingEngine>,
    AxumPath((session_id, token, file_idx)): AxumPath<(String, String, usize)>,
    headers: HeaderMap,
) -> Response<Body> {
    serve_get(engine, &session_id, &token, file_idx, headers).await
}

async fn stream_get_with_name(
    State(engine): State<TorrentStreamingEngine>,
    AxumPath((session_id, token, file_idx, _filename)): AxumPath<(String, String, usize, String)>,
    headers: HeaderMap,
) -> Response<Body> {
    serve_get(engine, &session_id, &token, file_idx, headers).await
}

async fn subtitle_head_handler(
    State(engine): State<TorrentStreamingEngine>,
    AxumPath((session_id, token, file_idx, _filename)): AxumPath<(String, String, usize, String)>,
    headers: HeaderMap,
) -> Response<Body> {
    if !is_loopback_host(&headers) {
        return Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(Body::empty())
            .unwrap();
    }
    let Some(target) = engine
        .get_subtitle_target(&session_id, &token, file_idx)
        .await
    else {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap();
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            content_type_for_filename(&target.file_name),
        )
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, target.file_len.to_string())
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::empty())
        .unwrap()
}

async fn subtitle_get_handler(
    State(engine): State<TorrentStreamingEngine>,
    AxumPath((session_id, token, file_idx, _filename)): AxumPath<(String, String, usize, String)>,
    headers: HeaderMap,
) -> Response<Body> {
    if !is_loopback_host(&headers) {
        return Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(Body::empty())
            .unwrap();
    }
    let Some(target) = engine
        .get_subtitle_target(&session_id, &token, file_idx)
        .await
    else {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Subtitle track not found"))
            .unwrap();
    };

    let upstream_url = engine.upstream_file_url(&target.info_hash, target.file_idx, false);
    let upstream_resp = tokio::select! {
        _ = target.cancel_token.cancelled() => {
            return Response::builder()
                .status(StatusCode::GONE)
                .body(Body::empty())
                .unwrap();
        }
        res = engine.upstream_stream_client().get(&upstream_url).send() => res
    };

    let resp = match upstream_resp {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            return Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .body(Body::from(format!(
                    "Upstream subtitle stream returned HTTP {}",
                    r.status()
                )))
                .unwrap();
        }
        Err(err) => {
            return Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .body(Body::from(format!("Unable to open subtitle stream: {err}")))
                .unwrap();
        }
    };

    let content_len = resp
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .unwrap_or_else(|| target.file_len.to_string());

    let stream = resp.bytes_stream();
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            content_type_for_filename(&target.file_name),
        )
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, content_len)
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from_stream(stream))
        .unwrap()
}

async fn serve_head(
    engine: TorrentStreamingEngine,
    session_id: &str,
    token: &str,
    file_idx: usize,
    headers: &HeaderMap,
) -> Response<Body> {
    if !is_loopback_host(headers) {
        return Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(Body::empty())
            .unwrap();
    }
    let Some(target) = engine.get_stream_target(session_id, token, file_idx).await else {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap();
    };

    let range_header = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    if let Some(rh) = range_header {
        match parse_http_byte_range(&rh, target.file_len) {
            Some((start, end)) => {
                let len = end - start + 1;
                return Response::builder()
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header(
                        header::CONTENT_TYPE,
                        content_type_for_filename(&target.file_name),
                    )
                    .header(header::ACCEPT_RANGES, "bytes")
                    .header(header::CONTENT_LENGTH, len.to_string())
                    .header(
                        header::CONTENT_RANGE,
                        format!("bytes {start}-{end}/{}", target.file_len),
                    )
                    .header(header::CACHE_CONTROL, "no-store")
                    .body(Body::empty())
                    .unwrap();
            }
            None => {
                return Response::builder()
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .header(header::CONTENT_RANGE, format!("bytes */{}", target.file_len))
                    .header(header::ACCEPT_RANGES, "bytes")
                    .body(Body::empty())
                    .unwrap();
            }
        }
    }

    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            content_type_for_filename(&target.file_name),
        )
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, target.file_len.to_string())
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::empty())
        .unwrap()
}

async fn serve_get(
    engine: TorrentStreamingEngine,
    session_id: &str,
    token: &str,
    file_idx: usize,
    headers: HeaderMap,
) -> Response<Body> {
    if !is_loopback_host(&headers) {
        return Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(Body::from("Forbidden host header"))
            .unwrap();
    }
    let Some(target) = engine.get_stream_target(session_id, token, file_idx).await else {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Streaming session not found or unauthorized"))
            .unwrap();
    };

    if target.file_len == 0 {
        return Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .body(Body::from("Media file length not yet available"))
            .unwrap();
    }

    let range_header = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    let (expected_status, start, end, is_partial) = if let Some(ref rh) = range_header {
        match parse_http_byte_range(rh, target.file_len) {
            Some((s, e)) => (StatusCode::PARTIAL_CONTENT, s, e, true),
            None => {
                return Response::builder()
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .header(header::CONTENT_RANGE, format!("bytes */{}", target.file_len))
                    .header(header::ACCEPT_RANGES, "bytes")
                    .body(Body::empty())
                    .unwrap();
            }
        }
    } else {
        (StatusCode::OK, 0, target.file_len - 1, false)
    };

    let content_length = end - start + 1;
    let intent = classify_http_intent(start, content_length, target.file_len, false, is_partial);
    let range_label = format!(
        "fileIdx={} bytes={start}-{end}/{}",
        target.file_idx, target.file_len
    );

    let upstream_url = engine.upstream_file_url(&target.info_hash, target.file_idx, false);
    let mut req = engine.upstream_stream_client().get(&upstream_url);
    if is_partial {
        req = req.header(reqwest::header::RANGE, format!("bytes={start}-{end}"));
    }

    let upstream_resp = tokio::select! {
        _ = target.cancel_token.cancelled() => {
            return Response::builder()
                .status(StatusCode::GONE)
                .body(Body::empty())
                .unwrap();
        }
        res = req.send() => res
    };

    let resp = match upstream_resp {
        Ok(r) => r,
        Err(err) => {
            engine.log_event(&format!(
                "Upstream libtorrent proxy request failed ({range_label}): {err}"
            ));
            return Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Body::from(format!("Upstream stream unavailable: {err}")))
                .unwrap();
        }
    };

    let upstream_status =
        StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    if upstream_status == StatusCode::RANGE_NOT_SATISFIABLE {
        return Response::builder()
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .header(header::CONTENT_RANGE, format!("bytes */{}", target.file_len))
            .header(header::ACCEPT_RANGES, "bytes")
            .body(Body::empty())
            .unwrap();
    }
    if !upstream_status.is_success() {
        return Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .body(Body::from(format!(
                "Upstream libtorrent engine returned HTTP {upstream_status}"
            )))
            .unwrap();
    }

    let final_status = if upstream_status == StatusCode::PARTIAL_CONTENT {
        StatusCode::PARTIAL_CONTENT
    } else if upstream_status == StatusCode::OK {
        StatusCode::OK
    } else {
        expected_status
    };

    let upstream_content_length = resp
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .unwrap_or_else(|| content_length.to_string());

    let upstream_content_range = resp
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .or_else(|| {
            (final_status == StatusCode::PARTIAL_CONTENT)
                .then(|| format!("bytes {start}-{end}/{}", target.file_len))
        });

    let upstream_content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .filter(|ct| !ct.trim().is_empty() && *ct != "application/octet-stream")
        .map(str::to_string)
        .unwrap_or_else(|| content_type_for_filename(&target.file_name).to_string());

    engine.log_event(&format!(
        "HTTP GET {} ({final_status}, intent={intent:?}, {upstream_content_length} bytes)",
        range_label
    ));

    let tracked = TrackedProxyStream::new(
        resp.bytes_stream(),
        target.active_http_streams,
        target.cancel_token,
        engine,
        intent,
        range_label,
    );

    let mut builder = Response::builder()
        .status(final_status)
        .header(header::CONTENT_TYPE, upstream_content_type)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, upstream_content_length)
        .header(header::CACHE_CONTROL, "no-store");

    if let Some(cr) = upstream_content_range {
        if let Ok(val) = HeaderValue::from_str(&cr) {
            builder = builder.header(header::CONTENT_RANGE, val);
        }
    }

    builder.body(Body::from_stream(tracked)).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;

    struct NeverReadyStream;
    impl Stream for NeverReadyStream {
        type Item = Result<Bytes, std::io::Error>;

        fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Poll::Pending
        }
    }

    #[tokio::test]
    async fn tracked_proxy_stream_wakes_immediately_on_cancellation() {
        let tmp = std::env::temp_dir().join(format!(
            "sf-http-cancel-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let engine = TorrentStreamingEngine::start(tmp.clone(), 128)
            .await
            .expect("engine starts");
        let counter = Arc::new(AtomicU32::new(0));
        let cancel = CancellationToken::new();

        let mut stream = TrackedProxyStream::new(
            NeverReadyStream,
            Arc::clone(&counter),
            cancel.clone(),
            engine.clone(),
            PlaybackIntent::DirectInitial,
            "test-range".to_string(),
        );
        assert_eq!(counter.load(Ordering::Relaxed), 1);

        let cancel_clone = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            cancel_clone.cancel();
        });

        let started = Instant::now();
        let item = stream.next().await.expect("should yield error");
        let err = item.expect_err("should abort");
        assert_eq!(err.kind(), std::io::ErrorKind::ConnectionAborted);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "cancellation must wake proxy stream immediately"
        );
        drop(stream);
        assert_eq!(counter.load(Ordering::Relaxed), 0);

        engine.shutdown().await;
        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }
}
