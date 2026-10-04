use anyhow::{Context as AnyhowContext, Result};
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::{Mutex, broadcast};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use tokio::net::TcpStream;

pub type WsSink = Arc<Mutex<futures::stream::SplitSink<
    WebSocketStream<MaybeTlsStream<TcpStream>>,
    Message,
>>>;

pub fn json_message(value: Value) -> Message {
    Message::Text(value.to_string().into())
}

pub async fn connect_plugin(args: &[String]) -> Result<(WsSink, broadcast::Receiver<Value>, Option<Value>)> {
    // OpenDeck launches native plugins using the normal Elgato-style CLI:
    // -port <n> -pluginUUID <uuid> -registerEvent registerPlugin -info <json>
    // See OpenDeck's native-plugin launcher for the exact argument order.
    fn value_after(args: &[String], key: &str) -> Result<String> {
        let pos = args.iter().position(|arg| arg == key)
            .ok_or_else(|| anyhow::anyhow!("missing required plugin argument {key}"))?;
        args.get(pos + 1)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("missing value after {key}"))
    }

    let port = value_after(args, "-port")?;
    let uuid = value_after(args, "-pluginUUID")?;
    let event = value_after(args, "-registerEvent")?;
    // -info carries OpenDeck's own live theme colors (application.colors.*).
    // Best-effort only: the native management/approval UI reuses this palette
    // so it visually matches OpenDeck's own settings instead of an arbitrary
    // hardcoded theme, but the plugin must still work if this is missing
    // (e.g. `--open-management` standalone launches never set it).
    let info = value_after(args, "-info").ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok());

    let (ws, _) = connect_async(format!("ws://127.0.0.1:{port}"))
        .await
        .with_context(|| format!("failed to connect to OpenDeck websocket on port {port}"))?;

    let (mut tx, mut rx) = ws.split();

    // The very first frame must register the plugin with OpenDeck.
    tx.send(json_message(json!({
        "event": event,
        "uuid": uuid
    }))).await?;

    let sink = Arc::new(Mutex::new(tx));
    let (sender, receiver) = broadcast::channel(256);

    tokio::spawn(async move {
        while let Some(message) = rx.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    match serde_json::from_str::<Value>(&text) {
                        Ok(value) => {
                            log::debug!("[OPENACTION_RX] {}", value);
                            let _ = sender.send(value);
                        }
                        Err(error) => {
                            log::warn!("[OPENACTION] invalid JSON from OpenDeck: {error}: {text}");
                        }
                    }
                }
                Ok(Message::Ping(payload)) => {
                    log::trace!("[OPENACTION] ping from OpenDeck ({} bytes)", payload.len());
                }
                Ok(Message::Close(frame)) => {
                    log::info!("[OPENACTION] OpenDeck websocket closed: {:?}", frame);
                    break;
                }
                Ok(_) => {}
                Err(error) => {
                    log::warn!("[OPENACTION] websocket read failed: {error}");
                    break;
                }
            }
        }
    });

    Ok((sink, receiver, info))
}

pub async fn send(sink: &WsSink, value: Value) -> Result<()> {
    log::debug!("[OPENACTION_TX] {}", value);
    sink.lock().await.send(json_message(value)).await?;
    Ok(())
}

pub fn spawn_openaction_logger(mut rx: broadcast::Receiver<Value>) {
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(value) => {
                    log::debug!("[OPENACTION] event={}", value["event"]);
                }
                Err(broadcast::error::RecvError::Closed) => break,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
            }
        }
    });
}
