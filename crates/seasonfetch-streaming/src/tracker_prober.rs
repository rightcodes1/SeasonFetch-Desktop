//! Parallel UDP & HTTP tracker RTT prober and resilient default trackers.
//!
//! Adapted from the independent MIT-licensed open-source project
//! `stremio-native/stream-server` (Copyright (c) 2025 perpetus,
//! https://github.com/stremio-native/stream-server).

use std::time::{Duration, Instant};
use tokio::net::{TcpStream, UdpSocket};
use url::Url;

const UDP_CONNECT_TIMEOUT: Duration = Duration::from_millis(900);
const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_millis(1500);

pub const DEFAULT_TRACKERS: &[&str] = &[
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.stealth.si:80/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://exodus.desync.com:6969/announce",
    "udp://tracker.moeking.me:6969/announce",
    "udp://explodie.org:6969/announce",
    "udp://open.demonii.com:1337/announce",
    "udp://tracker1.bt.moack.co.kr:80/announce",
    "udp://tracker.tiny-vps.com:6969/announce",
    "udp://tracker.dler.org:6969/announce",
    "http://tracker.opentrackr.org:1337/announce",
];

pub struct TrackerProber;

impl TrackerProber {
    /// Probe candidate trackers in parallel and return them sorted by fastest live RTT first.
    /// Unreachable trackers are retained at the tail so callers never lose fallback entries.
    pub async fn rank_trackers(trackers: Vec<String>) -> Vec<String> {
        let mut handles = Vec::with_capacity(trackers.len());
        for tracker in trackers {
            handles.push(tokio::spawn(async move {
                let rtt = Self::probe(&tracker).await;
                (tracker, rtt)
            }));
        }

        let mut results = Vec::with_capacity(handles.len());
        for handle in handles {
            if let Ok((tracker, rtt)) = handle.await {
                results.push((tracker, rtt.unwrap_or(Duration::from_secs(3600))));
            }
        }

        results.sort_by_key(|(_, rtt)| *rtt);
        results.into_iter().map(|(url, _)| url).collect()
    }

    pub async fn probe(url_str: &str) -> Option<Duration> {
        let url = Url::parse(url_str).ok()?;
        match url.scheme() {
            "udp" => Self::probe_udp(&url).await,
            "http" | "https" => Self::probe_tcp_http(&url).await,
            _ => None,
        }
    }

    async fn probe_tcp_http(url: &Url) -> Option<Duration> {
        let host = url.host_str()?;
        let port = url.port_or_known_default()?;
        let start = Instant::now();
        match tokio::time::timeout(HTTP_CONNECT_TIMEOUT, TcpStream::connect((host, port))).await {
            Ok(Ok(_stream)) => Some(start.elapsed()),
            _ => None,
        }
    }

    async fn probe_udp(url: &Url) -> Option<Duration> {
        let host = url.host_str()?;
        let port = url.port().unwrap_or(80);
        let addr = format!("{host}:{port}");

        let socket = UdpSocket::bind("0.0.0.0:0").await.ok()?;
        tokio::time::timeout(UDP_CONNECT_TIMEOUT, socket.connect(&addr))
            .await
            .ok()?
            .ok()?;

        // BEP 15 UDP Tracker Connect Request: protocol_id (0x41727101980), action=0, tx_id
        let protocol_id: u64 = 0x41727101980;
        let action: u32 = 0;
        let transaction_id: u32 = 0x5346_1337;

        let mut buf = [0u8; 16];
        buf[0..8].copy_from_slice(&protocol_id.to_be_bytes());
        buf[8..12].copy_from_slice(&action.to_be_bytes());
        buf[12..16].copy_from_slice(&transaction_id.to_be_bytes());

        let start = Instant::now();
        socket.send(&buf).await.ok()?;

        let mut recv_buf = [0u8; 16];
        match tokio::time::timeout(UDP_CONNECT_TIMEOUT, socket.recv(&mut recv_buf)).await {
            Ok(Ok(n)) if n >= 8 => {
                let recv_action = u32::from_be_bytes(recv_buf[0..4].try_into().ok()?);
                let recv_tx = u32::from_be_bytes(recv_buf[4..8].try_into().ok()?);
                if recv_action == 0 && recv_tx == transaction_id {
                    Some(start.elapsed())
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn probes_local_udp_connect_response_and_ranks_live_first() {
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = server.local_addr().unwrap().port();

        let srv_task = tokio::spawn(async move {
            let mut buf = [0u8; 32];
            if let Ok((n, peer)) = server.recv_from(&mut buf).await {
                if n >= 16 {
                    let mut resp = [0u8; 16];
                    resp[0..4].copy_from_slice(&0u32.to_be_bytes()); // action = connect
                    resp[4..8].copy_from_slice(&buf[12..16]); // echo tx_id
                    let _ = server.send_to(&resp, peer).await;
                }
            }
        });

        let live_url = format!("udp://127.0.0.1:{port}/announce");
        let dead_url = "http://127.0.0.1:1/announce".to_string();
        let ranked = TrackerProber::rank_trackers(vec![dead_url.clone(), live_url.clone()]).await;
        let _ = srv_task.await;

        assert_eq!(ranked[0], live_url);
        assert_eq!(ranked[1], dead_url);
    }
}
