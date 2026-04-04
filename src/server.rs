use std::io;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::header::{CONTENT_TYPE, HeaderValue};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc};

use crate::db_handler::{handle_db_request, CellEngineLike};
use crate::net_handler::{handle_net_request, pack_network_event, NetworkEvent, NodeClientLike};
use crate::protocol::{
    build_header, parse_header, DiarsabaHeader, DOMAIN_AUTH, DOMAIN_DB, DOMAIN_NET, FLAG_ERROR,
    HEADER_LEN,
};

const INDEX_HTML: &str = include_str!("../index.html");

#[derive(Clone)]
pub struct AppState<D, N> {
    pub db: Arc<D>,
    pub net: Arc<N>,
    pub event_tx: broadcast::Sender<Vec<u8>>,
}

pub fn build_app<D, N>(state: Arc<AppState<D, N>>) -> Router
where
    D: CellEngineLike + Send + Sync + 'static,
    N: NodeClientLike + Send + Sync + 'static,
{
    Router::new()
        .route("/", get(index_handler))
        .route("/do", post(http_frame_handler::<D, N>))
        .route("/ws", get(ws_upgrade_handler::<D, N>))
        .with_state(state)
}

pub async fn serve<D, N>(bind_addr: &str, state: Arc<AppState<D, N>>) -> io::Result<()>
where
    D: CellEngineLike + Send + Sync + 'static,
    N: NodeClientLike + Send + Sync + 'static,
{
    let listener = TcpListener::bind(bind_addr).await?;
    axum::serve(listener, build_app(state)).await
}

pub async fn route_diarsaba_frame<D, N>(frame: &[u8], db: &D, net: &N) -> Vec<u8>
where
    D: CellEngineLike,
    N: NodeClientLike,
{
    route_diarsaba_frame_inner(frame, db, net).await
}

pub async fn forward_network_events(
    mut events: mpsc::Receiver<NetworkEvent>,
    event_tx: broadcast::Sender<Vec<u8>>,
) {
    while let Some(event) = events.recv().await {
        for frame in pack_network_event(event) {
            let _ = event_tx.send(frame);
        }
    }
}

async fn route_diarsaba_frame_inner<D, N>(frame: &[u8], db: &D, net: &N) -> Vec<u8>
where
    D: CellEngineLike,
    N: NodeClientLike,
{
    let header = match parse_header(frame) {
        Ok(header) => header,
        Err(error) => return build_error_frame(frame.first().copied().unwrap_or(0), extract_req_id(frame), error),
    };

    let payload_end = HEADER_LEN + usize::from(header.payload_len);
    let payload = match frame.get(HEADER_LEN..payload_end) {
        Some(payload) => payload,
        None => return build_error_frame(header.domain, header.req_id, "frame payload shorter than declared length"),
    };

    match header.domain {
        DOMAIN_DB => handle_db_request(&header, payload, db).await,
        DOMAIN_NET => handle_net_request(&header, payload, net).await,
        DOMAIN_AUTH => build_success_frame(header.domain, header.req_id, b"auth not implemented".to_vec()),
        _ => build_error_frame(header.domain, header.req_id, "unknown diarsaba domain"),
    }
}

async fn http_frame_handler<D, N>(
    State(state): State<Arc<AppState<D, N>>>,
    body: Bytes,
) -> Response
where
    D: CellEngineLike + Send + Sync + 'static,
    N: NodeClientLike + Send + Sync + 'static,
{
    binary_response(route_diarsaba_frame(body.as_ref(), state.db.as_ref(), state.net.as_ref()).await)
}

async fn ws_upgrade_handler<D, N>(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState<D, N>>>,
) -> impl IntoResponse
where
    D: CellEngineLike + Send + Sync + 'static,
    N: NodeClientLike + Send + Sync + 'static,
{
    ws.on_upgrade(move |socket| websocket_session(socket, state))
}

async fn websocket_session<D, N>(socket: WebSocket, state: Arc<AppState<D, N>>)
where
    D: CellEngineLike + Send + Sync + 'static,
    N: NodeClientLike + Send + Sync + 'static,
{
    let (mut ws_tx, mut ws_rx) = socket.split();
    let mut event_rx = state.event_tx.subscribe();

    loop {
        tokio::select! {
            incoming = ws_rx.next() => {
                match incoming {
                    Some(Ok(Message::Binary(frame))) => {
                        let response = route_diarsaba_frame(frame.as_ref(), state.db.as_ref(), state.net.as_ref()).await;
                        if ws_tx.send(Message::Binary(response.into())).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Ping(data))) => {
                        if ws_tx.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
            event = event_rx.recv() => {
                match event {
                    Ok(frame) => {
                        if ws_tx.send(Message::Binary(frame.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

async fn index_handler() -> Html<&'static str> {
    Html(INDEX_HTML)
}

fn binary_response(frame: Vec<u8>) -> Response {
    (
        StatusCode::OK,
        [(CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"))],
        frame,
    )
        .into_response()
}

fn build_success_frame(domain: u8, req_id: u32, payload: Vec<u8>) -> Vec<u8> {
    match u16::try_from(payload.len()) {
        Ok(payload_len) => build_frame(
            DiarsabaHeader {
                domain,
                flags: 0,
                req_id,
                payload_len,
            },
            payload,
        ),
        Err(_) => build_error_frame(domain, req_id, "response payload exceeds u16 length"),
    }
}

fn build_error_frame(domain: u8, req_id: u32, message: &str) -> Vec<u8> {
    let payload = message.as_bytes();
    let payload_len = u16::try_from(payload.len()).unwrap_or(u16::MAX);
    let payload = if payload.len() > usize::from(u16::MAX) {
        payload[..usize::from(u16::MAX)].to_vec()
    } else {
        payload.to_vec()
    };

    build_frame(
        DiarsabaHeader {
            domain,
            flags: FLAG_ERROR,
            req_id,
            payload_len,
        },
        payload,
    )
}

fn build_frame(header: DiarsabaHeader, payload: Vec<u8>) -> Vec<u8> {
    let mut frame = Vec::with_capacity(HEADER_LEN + payload.len());
    frame.extend_from_slice(&build_header(&header));
    frame.extend_from_slice(&payload);
    frame
}

fn extract_req_id(frame: &[u8]) -> u32 {
    frame
        .get(2..6)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_le_bytes)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    use super::*;
    use crate::net_handler::PeerId;
    use crate::protocol::{parse_header, DOMAIN_EVENT, DOMAIN_NET, NET_FIND};

    #[cfg(feature = "synap2p")]
    fn test_peer_id() -> PeerId {
        PeerId::random()
    }

    #[cfg(not(feature = "synap2p"))]
    fn test_peer_id() -> PeerId {
        String::from("peer-1")
    }

    struct MockDb;

    impl CellEngineLike for MockDb {
        type Error = &'static str;

        async fn ejecutar(
            &self,
            _target_index: u32,
            _solucion: &[u8; 32],
            _opcode: u8,
            _params: &[u8],
        ) -> Result<Vec<u8>, Self::Error> {
            Ok(b"db-ok".to_vec())
        }
    }

    #[derive(Default)]
    struct MockNet {
        providers: Vec<PeerId>,
    }

    impl NodeClientLike for MockNet {
        type Error = &'static str;

        async fn send_direct_message(&self, _peer: PeerId, _data: Vec<u8>) -> Result<(), Self::Error> {
            Ok(())
        }

        async fn publish_message(&self, _topic: String, _data: Vec<u8>) -> Result<(), Self::Error> {
            Ok(())
        }

        async fn announce_provider(&self, _key: String) -> Result<(), Self::Error> {
            Ok(())
        }

        async fn find_providers(&self, _key: String) -> Result<Vec<PeerId>, Self::Error> {
            Ok(self.providers.clone())
        }
    }

    #[test]
    fn route_returns_auth_mock_payload() {
        let net = MockNet::default();
        let frame = build_frame(
            DiarsabaHeader {
                domain: DOMAIN_AUTH,
                flags: 0,
                req_id: 12,
                payload_len: 0,
            },
            Vec::new(),
        );

        let response = block_on(route_diarsaba_frame(&frame, &MockDb, &net));
        let header = parse_header(&response[..HEADER_LEN]).expect("header should parse");

        assert_eq!(header.domain, DOMAIN_AUTH);
        assert_eq!(header.flags, 0);
        assert_eq!(header.req_id, 12);
        assert_eq!(&response[HEADER_LEN..], b"auth not implemented");
    }

    #[test]
    fn route_returns_error_for_short_frame() {
        let net = MockNet::default();
        let response = block_on(route_diarsaba_frame(&[1, 2, 3], &MockDb, &net));
        let header = parse_header(&response[..HEADER_LEN]).expect("header should parse");

        assert_eq!(header.flags, FLAG_ERROR);
        assert_eq!(header.req_id, 0);
    }

    #[test]
    fn route_dispatches_net_requests() {
        let expected_peer = test_peer_id();
        let expected_peer_bytes = expected_peer.to_string().into_bytes();
        let mut expected_payload = Vec::new();
        expected_payload.push(u8::try_from(expected_peer_bytes.len()).expect("peer id should fit in test payload"));
        expected_payload.extend_from_slice(&expected_peer_bytes);
        let payload = vec![NET_FIND, 3, b'k', b'e', b'y'];
        let frame = build_frame(
            DiarsabaHeader {
                domain: DOMAIN_NET,
                flags: 0,
                req_id: 33,
                payload_len: payload.len() as u16,
            },
            payload,
        );
        let net = MockNet {
            providers: vec![expected_peer],
        };

        let response = block_on(route_diarsaba_frame(&frame, &MockDb, &net));
        let header = parse_header(&response[..HEADER_LEN]).expect("header should parse");

        assert_eq!(header.domain, DOMAIN_NET);
        assert_eq!(header.req_id, 33);
        assert_eq!(&response[HEADER_LEN..], expected_payload.as_slice());
    }

    #[test]
    fn forward_network_events_broadcasts_frames() {
        let provider = test_peer_id();
        let (event_tx, mut event_rx) = broadcast::channel(4);
        let (network_tx, network_rx) = mpsc::channel(4);

        network_tx.try_send(NetworkEvent::ProviderFound {
            key: String::from("cell"),
            providers: vec![provider.clone()],
        }).expect("send should work");
        drop(network_tx);

        block_on(forward_network_events(network_rx, event_tx));

        let frame = event_rx.try_recv().expect("broadcast frame expected");
        let header = parse_header(&frame[..HEADER_LEN]).expect("header should parse");
        let provider_bytes = provider.to_string().into_bytes();
        let mut expected_payload = vec![3, 4, b'c', b'e', b'l', b'l'];
        expected_payload.push(u8::try_from(provider_bytes.len()).expect("peer id should fit in test payload"));
        expected_payload.extend_from_slice(&provider_bytes);

        assert_eq!(header.domain, DOMAIN_EVENT);
        assert_eq!(header.req_id, 0);
        assert_eq!(&frame[HEADER_LEN..], expected_payload.as_slice());
    }

    fn block_on<F>(future: F) -> F::Output
    where
        F: Future,
    {
        let waker = dummy_waker();
        let mut future = Pin::from(Box::new(future));
        let mut context = Context::from_waker(&waker);

        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(output) => return output,
                Poll::Pending => std::thread::yield_now(),
            }
        }
    }

    fn dummy_waker() -> Waker {
        unsafe { Waker::from_raw(dummy_raw_waker()) }
    }

    fn dummy_raw_waker() -> RawWaker {
        RawWaker::new(std::ptr::null(), &DUMMY_WAKER_VTABLE)
    }

    unsafe fn clone_waker(_: *const ()) -> RawWaker {
        dummy_raw_waker()
    }

    unsafe fn wake_waker(_: *const ()) {}

    unsafe fn wake_by_ref_waker(_: *const ()) {}

    unsafe fn drop_waker(_: *const ()) {}

    static DUMMY_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
        clone_waker,
        wake_waker,
        wake_by_ref_waker,
        drop_waker,
    );
}