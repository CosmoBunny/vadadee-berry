//! Blind WebSocket relay: `GET /ws/{room_id}` → broadcast binary to all peers in the room.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::{accept_hdr_async, tungstenite::Message};

type PeerTx = mpsc::UnboundedSender<Message>;

static NEXT_PEER_ID: AtomicU64 = AtomicU64::new(1);

struct Room {
    peers: Vec<(u64, PeerTx)>,
}

pub async fn run_relay_until_stopped(
    bind_addr: &str,
    mut stop: tokio::sync::watch::Receiver<bool>,
) -> Result<(), String> {
    let rooms: Arc<Mutex<HashMap<String, Room>>> = Arc::new(Mutex::new(HashMap::new()));
    let listener = TcpListener::bind(bind_addr)
        .await
        .map_err(|e| format!("relay bind {bind_addr}: {e}"))?;
    log::info!("Collab relay hosting on ws://{bind_addr}/ws/{{room_id}}");

    loop {
        tokio::select! {
            changed = stop.changed() => {
                if changed.is_ok() && *stop.borrow() {
                    break;
                }
            }
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else { continue };
                let rooms2 = rooms.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_peer(stream, rooms2).await {
                        log::debug!("relay peer ended: {e}");
                    }
                });
            }
        }
    }
    Ok(())
}

async fn handle_peer(stream: TcpStream, rooms: Arc<Mutex<HashMap<String, Room>>>) -> Result<(), String> {
    let room_slot = Arc::new(std::sync::Mutex::new("default".to_string()));
    let room_slot_cb = room_slot.clone();

    let ws = accept_hdr_async(stream, move |req: &Request, response: Response| {
        let path = req.uri().path();
        let id = path
            .strip_prefix("/ws/")
            .or_else(|| path.strip_prefix("/ws"))
            .map(|s| s.trim_matches('/'))
            .filter(|s| !s.is_empty())
            .unwrap_or("default")
            .to_string();
        *room_slot_cb.lock().unwrap() = id;
        Ok(response)
    })
    .await
    .map_err(|e| format!("websocket handshake: {e}"))?;

    let rid = room_slot.lock().unwrap().clone();
    let (mut write, mut read) = ws.split();

    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
    let my_id = NEXT_PEER_ID.fetch_add(1, Ordering::Relaxed);
    {
        let mut map = rooms.lock().await;
        let room = map.entry(rid.clone()).or_insert_with(|| Room { peers: vec![] });
        room.peers.push((my_id, tx));
    }

    let rooms_fwd = rooms.clone();
    let rid_fwd = rid.clone();
    let forward = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if write.send(msg).await.is_err() {
                break;
            }
        }
    });

    while let Some(incoming) = read.next().await {
        let msg = match incoming {
            Ok(m) => m,
            Err(_) => break,
        };
        if matches!(msg, Message::Close(_)) {
            break;
        }
        let bin = match msg {
            Message::Binary(b) => Message::Binary(b),
            Message::Text(t) => Message::Text(t),
            _ => continue,
        };
        let mut map = rooms_fwd.lock().await;
        if let Some(room) = map.get_mut(&rid_fwd) {
            for (peer_id, peer) in &room.peers {
                if *peer_id != my_id {
                    let _ = peer.send(bin.clone());
                }
            }
        }
    }

    forward.abort();
    let mut map = rooms.lock().await;
    if let Some(room) = map.get_mut(&rid) {
        room.peers.retain(|(id, p)| *id != my_id && !p.is_closed());
        if room.peers.is_empty() {
            map.remove(&rid);
        }
    }
    Ok(())
}