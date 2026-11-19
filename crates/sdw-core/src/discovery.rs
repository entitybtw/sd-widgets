//! LAN discovery: the Deck broadcasts a probe, daemons answer with their address.

use crate::model::{DiscoveryResponse, DISCOVERY_MAGIC, DISCOVERY_PORT};
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn probe_socket() -> std::io::Result<UdpSocket> {
    let s = UdpSocket::bind("0.0.0.0:0")?;
    s.set_broadcast(true)?;
    s.set_read_timeout(Some(Duration::from_millis(250)))?;
    Ok(s)
}

/// Broadcast on the local network and on loopback for a short while.
pub fn scan(timeout: Duration) -> Result<Vec<DiscoveryResponse>, String> {
    let sock = probe_socket().map_err(|e| e.to_string())?;
    let payload = DISCOVERY_MAGIC.as_bytes();
    let mut targets: Vec<SocketAddr> = vec![
        SocketAddr::from((Ipv4Addr::BROADCAST, DISCOVERY_PORT)),
        SocketAddr::from((Ipv4Addr::LOCALHOST, DISCOVERY_PORT)),
    ];
    if let Some(local) = crate::util::local_ipv4() {
        if let std::net::IpAddr::V4(v4) = local {
            let o = v4.octets();
            targets.push(SocketAddr::from((
                Ipv4Addr::new(o[0], o[1], o[2], 255),
                DISCOVERY_PORT,
            )));
        }
    }
    for target in &targets {
        let _ = sock.send_to(payload, target);
    }
    let deadline = Instant::now() + timeout;
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    let mut buf = [0u8; 2048];
    while Instant::now() < deadline {
        match sock.recv_from(&mut buf) {
            Ok((n, from)) => {
                if let Ok(mut v) = serde_json::from_slice::<DiscoveryResponse>(&buf[..n]) {
                    if v.magic == DISCOVERY_MAGIC && seen.insert(v.daemon_id.clone()) {
                        if v.address.is_empty() {
                            v.address = from.ip().to_string();
                        }
                        out.push(v);
                    }
                }
            }
            Err(_) => {}
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Answer probes until `stop` is set. Bind failures are reported once and retried.
pub fn responder(
    name: String,
    daemon_id: String,
    port: u16,
    setup_done: bool,
    stop: Arc<AtomicBool>,
) -> std::io::Result<()> {
    let sock = UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT))?;
    sock.set_read_timeout(Some(Duration::from_millis(500)))?;
    let mut buf = [0u8; 512];
    while !stop.load(Ordering::Relaxed) {
        match sock.recv_from(&mut buf) {
            Ok((n, from)) => {
                if n < DISCOVERY_MAGIC.len()
                    || &buf[..DISCOVERY_MAGIC.len()] != DISCOVERY_MAGIC.as_bytes()
                {
                    continue;
                }
                let reply = DiscoveryResponse {
                    magic: DISCOVERY_MAGIC.to_string(),
                    name: name.clone(),
                    daemon_id: daemon_id.clone(),
                    version: crate::model::VERSION.to_string(),
                    port,
                    setup_done,
                    address: from.ip().to_string(),
                };
                if let Ok(json) = serde_json::to_vec(&reply) {
                    let _ = sock.send_to(&json, from);
                }
            }
            Err(_) => {}
        }
    }
    Ok(())
}
