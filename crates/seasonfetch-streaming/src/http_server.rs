use crate::TorrentStreamingEngine;
use axum::body::Body;
use axum::extract::{Path as AxumPath, State};
use axum::http::{header, HeaderMap, HeaderValue, Response, StatusCode};
use axum::routing::get;
use axum::Router;
use seasonfetch_core::streaming::{content_type_for_filename, parse_http_byte_range};
use std::io::SeekFrom;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeekExt, ReadBuf};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio_util::io::ReaderStream;

/// RAII wrapper around an async reader that:
/// 1. Tracks active HTTP stream count (`active_http_streams`),
/// 2. Logs time-to-first-byte (TTFB) and errors to the engine log sink, and
/// 3. Immediately aborts if the session's cancellation signal fires (e.g., on player close),
///    releasing `FileStream` and Windows file handles in milliseconds so cache deletion succeeds cleanly.
struct TrackedReader<R> {
    inner: R,
    counter: Arc<AtomicU32>,
    cancel_rx: watch::Receiver<bool>,
    engine: TorrentStreamingEngine,
    range_label: String,
    started_at: Instant,
    first_byte_logged: bool,
    bytes_sent: u64,
}

impl<R> TrackedReader<R> {
    fn new(
        inner: R,
        counter: Arc<AtomicU32>,
        cancel_rx: watch::Receiver<bool>,
        engine: TorrentStreamingEngine,
        range_label: String,
    ) -> Self {
        counter.fetch_add(1, Ordering::Relaxed);
        Self {
            inner,
            counter,
            cancel_rx,
            engine,
            range_label,
            started_at: Instant::now(),
            first_byte_logged: false,
            bytes_sent: 0,
        }
    }
}

impl<R> Drop for TrackedReader<R> {
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

impl<R: AsyncRead + Unpin> AsyncRead for TrackedReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if *self.cancel_rx.borrow() {
            return Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "streaming session stopped",
            )));
        }
        let before = buf.filled().len();
        let res = Pin::new(&mut self.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &res {
            let delta = buf.filled().len().saturating_sub(before) as u64;
            if delta > 0 {
                self.bytes_sent = self.bytes_sent.saturating_add(delta);
                if !self.first_byte_logged {
                    self.first_byte_logged = true;
                    self.engine.log_event(&format!(
                        "HTTP first byte delivered ({}) in {}ms",
                        self.range_label,
                        self.started_at.elapsed().as_millis()
                    ));
                }
            }
        } else if let Poll::Ready(Err(err)) = &res {
            self.engine.log_event(&format!(
                "HTTP read error ({}) after {} bytes: {err}",
                self.range_label, self.bytes_sent
            ));
        }
        res
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
            serde_json::json!({ "ok": true, "engine": "seasonfetch-streaming" }).to_string(),
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
    let Some((_handle, _idx, file_name, file_len, _counter, _cancel_rx)) =
        engine.get_stream_target(session_id, token, file_idx).await
    else {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap();
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type_for_filename(&file_name))
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, file_len.to_string())
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
    let Some((handle, idx, file_name, file_len, active_counter, cancel_rx)) =
        engine.get_stream_target(session_id, token, file_idx).await
    else {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Streaming session not found or unauthorized"))
            .unwrap();
    };

    if file_len == 0 {
        return Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .body(Body::from("Torrent file length not yet available"))
            .unwrap();
    }

    let range_header = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    let (status_code, start, end) = if let Some(ref rh) = range_header {
        match parse_http_byte_range(rh, file_len) {
            Some((s, e)) => (StatusCode::PARTIAL_CONTENT, s, e),
            None => {
                return Response::builder()
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .header(header::CONTENT_RANGE, format!("bytes */{file_len}"))
                    .header(header::ACCEPT_RANGES, "bytes")
                    .body(Body::empty())
                    .unwrap();
            }
        }
    } else {
        (StatusCode::OK, 0, file_len - 1)
    };

    // Retry briefly if the torrent is still completing Live state transition
    let mut last_stream_err = None;
    let mut opened_stream = None;
    for _ in 0..20 {
        match handle.clone().stream(idx) {
            Ok(s) => {
                opened_stream = Some(s);
                break;
            }
            Err(err) => {
                last_stream_err = Some(err);
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            }
        }
    }

    let mut file_stream = match opened_stream {
        Some(s) => s,
        None => {
            let msg = last_stream_err
                .map(|e| format!("Unable to open torrent file stream: {e:#}"))
                .unwrap_or_else(|| "Unable to open torrent file stream".to_string());
            return Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .body(Body::from(msg))
                .unwrap();
        }
    };

    if start > 0 {
        if let Err(err) = file_stream.seek(SeekFrom::Start(start)).await {
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from(format!("Failed to seek torrent stream to byte {start}: {err}")))
                .unwrap();
        }
    }

    let content_length = end - start + 1;
    let range_label = format!("fileIdx={idx} bytes={start}-{end}/{file_len}");
    engine.log_event(&format!(
        "HTTP GET {} ({status_code}, {content_length} bytes)",
        range_label
    ));
    let bounded = file_stream.take(content_length);
    let tracked = TrackedReader::new(bounded, active_counter, cancel_rx, engine, range_label);
    let stream = ReaderStream::with_capacity(tracked, 64 * 1024);

    let mut builder = Response::builder()
        .status(status_code)
        .header(header::CONTENT_TYPE, content_type_for_filename(&file_name))
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, content_length.to_string())
        .header(header::CACHE_CONTROL, "no-store");

    if status_code == StatusCode::PARTIAL_CONTENT {
        if let Ok(val) = HeaderValue::from_str(&format!("bytes {start}-{end}/{file_len}")) {
            builder = builder.header(header::CONTENT_RANGE, val);
        }
    }

    builder.body(Body::from_stream(stream)).unwrap()
}
