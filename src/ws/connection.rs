use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use prost::Message as ProstMessage;
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};
use tokio::time;
use tracing::{debug, error, info, warn};

use crate::api::error::NanitError;
use crate::api::types::WS_BASE_URL;
use crate::proto;
use crate::ws::codec;

const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(20);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
#[allow(dead_code)]
const RECONNECT_DELAYS: &[Duration] = &[
    Duration::from_secs(30),
    Duration::from_secs(120),
    Duration::from_secs(900),
    Duration::from_secs(3600),
];

type WsSink = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    tokio_tungstenite::tungstenite::Message,
>;

struct PendingRequest {
    sender: oneshot::Sender<proto::Response>,
}

pub struct NanitWebSocket {
    camera_uid: String,
    auth_token: Arc<Mutex<String>>,
    base_url: String,
    #[allow(dead_code)]
    auto_reconnect: bool,
    request_id: AtomicI32,
    pending: Arc<Mutex<HashMap<i32, PendingRequest>>>,
    ws_tx: Option<mpsc::Sender<Vec<u8>>>,
    sensor_data_tx: broadcast::Sender<Vec<proto::SensorData>>,
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl NanitWebSocket {
    pub fn new(camera_uid: &str, auth_token: &str) -> Self {
        let (sensor_data_tx, _) = broadcast::channel(64);
        Self {
            camera_uid: camera_uid.to_string(),
            auth_token: Arc::new(Mutex::new(auth_token.to_string())),
            base_url: WS_BASE_URL.to_string(),
            auto_reconnect: true,
            request_id: AtomicI32::new(0),
            pending: Arc::new(Mutex::new(HashMap::new())),
            ws_tx: None,
            sensor_data_tx,
            shutdown_tx: None,
        }
    }

    #[allow(dead_code)]
    pub fn update_auth_token(&self, token: &str) {
        let auth = self.auth_token.clone();
        let token = token.to_string();
        tokio::spawn(async move {
            *auth.lock().await = token;
        });
    }

    pub fn sensor_data_rx(&self) -> broadcast::Receiver<Vec<proto::SensorData>> {
        self.sensor_data_tx.subscribe()
    }

    /// Connect to the WebSocket endpoint. Spawns reader, writer, and keepalive tasks.
    pub async fn connect(&mut self) -> Result<(), NanitError> {
        let url = format!("{}/{}/user_connect", self.base_url, self.camera_uid);
        let token = self.auth_token.lock().await.clone();

        let uri: tokio_tungstenite::tungstenite::http::Uri = url
            .parse()
            .map_err(|e| NanitError::Other(anyhow::anyhow!("Invalid WS URL: {e}")))?;
        let host = uri.host().unwrap_or("api.nanit.com");

        let request = tokio_tungstenite::tungstenite::http::Request::builder()
            .uri(&url)
            .header("Host", host)
            // WebSocket: Authorization: Bearer {token}
            .header("Authorization", format!("Bearer {token}"))
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header(
                "Sec-WebSocket-Key",
                tokio_tungstenite::tungstenite::handshake::client::generate_key(),
            )
            .body(())
            .map_err(|e| NanitError::Other(anyhow::anyhow!("Failed to build WS request: {e}")))?;

        let (ws_stream, _) = tokio_tungstenite::connect_async(request)
            .await
            .map_err(|e| NanitError::Other(anyhow::anyhow!("WebSocket connect failed: {e}")))?;

        info!("WebSocket connected to camera {}", self.camera_uid);

        let (ws_sink, ws_stream) = ws_stream.split();

        // Channel for outgoing messages
        let (msg_tx, msg_rx) = mpsc::channel::<Vec<u8>>(64);
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>(1);

        self.ws_tx = Some(msg_tx.clone());
        self.shutdown_tx = Some(shutdown_tx);

        // Writer task
        tokio::spawn(Self::writer_task(ws_sink, msg_rx));

        // Reader task
        let pending = self.pending.clone();
        let sensor_tx = self.sensor_data_tx.clone();
        tokio::spawn(Self::reader_task(ws_stream, pending, sensor_tx));

        // Keepalive task
        tokio::spawn(Self::keepalive_task(msg_tx.clone(), shutdown_rx));

        Ok(())
    }

    /// Send a request and wait for the correlated response.
    pub async fn send_request(&self, request: proto::Request) -> Result<proto::Response, NanitError> {
        let ws_tx = self.ws_tx.as_ref().ok_or(NanitError::NotConnected)?;

        let id = request.id;
        let msg = proto::Message {
            r#type: proto::message::Type::Request as i32,
            request: Some(request),
            response: None,
        };
        let data = msg.encode_to_vec();

        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            pending.insert(id, PendingRequest { sender: tx });
        }

        ws_tx
            .send(data)
            .await
            .map_err(|_| NanitError::NotConnected)?;

        match time::timeout(DEFAULT_REQUEST_TIMEOUT, rx).await {
            Ok(Ok(response)) => {
                if response.status_code != 200 {
                    let msg = response
                        .status_message
                        .clone()
                        .unwrap_or_else(|| format!("Status code {}", response.status_code));
                    return Err(NanitError::Other(anyhow::anyhow!(msg)));
                }
                Ok(response)
            }
            Ok(Err(_)) => {
                self.pending.lock().await.remove(&id);
                Err(NanitError::WebSocketClosed("Response channel dropped".into()))
            }
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(NanitError::RequestTimeout)
            }
        }
    }

    pub fn next_request_id(&self) -> i32 {
        self.request_id.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Request sensor data.
    pub async fn get_sensor_data(&self) -> Result<proto::Response, NanitError> {
        let id = self.next_request_id();
        self.send_request(codec::build_get_sensor_data_request(id))
            .await
    }

    /// Tell camera to start/stop streaming.
    pub async fn put_streaming(
        &self,
        rtmp_url: &str,
        status: proto::streaming::Status,
    ) -> Result<proto::Response, NanitError> {
        let id = self.next_request_id();
        self.send_request(codec::build_put_streaming_request(id, rtmp_url, status))
            .await
    }

    /// Disconnect.
    pub async fn disconnect(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }
        self.ws_tx = None;
        // Reject all pending
        let mut pending = self.pending.lock().await;
        pending.clear();
        info!("WebSocket disconnected");
    }

    pub fn is_connected(&self) -> bool {
        self.ws_tx.is_some()
    }

    // --- Internal tasks ---

    async fn writer_task(mut sink: WsSink, mut rx: mpsc::Receiver<Vec<u8>>) {
        while let Some(data) = rx.recv().await {
            let msg = tokio_tungstenite::tungstenite::Message::Binary(data.into());
            if let Err(e) = sink.send(msg).await {
                error!("WebSocket write error: {e}");
                break;
            }
        }
        debug!("Writer task exiting");
    }

    async fn reader_task(
        mut stream: futures_util::stream::SplitStream<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
        >,
        pending: Arc<Mutex<HashMap<i32, PendingRequest>>>,
        sensor_tx: broadcast::Sender<Vec<proto::SensorData>>,
    ) {
        while let Some(result) = stream.next().await {
            match result {
                Ok(tokio_tungstenite::tungstenite::Message::Binary(data)) => {
                    match codec::decode_message(&data) {
                        Ok(msg) => {
                            // Handle response correlation
                            if msg.r#type == proto::message::Type::Response as i32 {
                                if let Some(response) = msg.response {
                                    let mut pending = pending.lock().await;
                                    if let Some(req) = pending.remove(&response.request_id) {
                                        let _ = req.sender.send(response);
                                    }
                                }
                            }

                            // Handle incoming sensor data (PUT_SENSOR_DATA from camera)
                            if msg.r#type == proto::message::Type::Request as i32 {
                                if let Some(request) = msg.request {
                                    if request.r#type
                                        == proto::RequestType::PutSensorData as i32
                                        && !request.sensor_data.is_empty()
                                    {
                                        let _ =
                                            sensor_tx.send(request.sensor_data);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Failed to decode WebSocket message: {e}");
                        }
                    }
                }
                Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => {
                    info!("WebSocket closed by server");
                    break;
                }
                Err(e) => {
                    error!("WebSocket read error: {e}");
                    break;
                }
                _ => {}
            }
        }
        debug!("Reader task exiting");
    }

    async fn keepalive_task(tx: mpsc::Sender<Vec<u8>>, mut shutdown_rx: mpsc::Receiver<()>) {
        let mut interval = time::interval(KEEPALIVE_INTERVAL);
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let data = codec::encode_keepalive();
                    if tx.send(data).await.is_err() {
                        break;
                    }
                }
                _ = shutdown_rx.recv() => {
                    break;
                }
            }
        }
        debug!("Keepalive task exiting");
    }
}
