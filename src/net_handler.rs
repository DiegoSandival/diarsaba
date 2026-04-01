use core::fmt::Display;
use core::future::Future;

use crate::protocol::{
    build_header, DiarsabaHeader, DOMAIN_EVENT, DOMAIN_NET, EVT_DIRECT_MSG, EVT_GOSSIP_MSG,
    EVT_PROVIDER_FOUND, FLAG_ERROR, NET_ANNOUNCE, NET_DIRECT_MSG, NET_FIND, NET_PUBLISH,
};

#[cfg(feature = "synap2p")]
use core::str::FromStr;

#[cfg(feature = "synap2p")]
use synap2p::{NetworkEvent, NodeClient, PeerId};

#[cfg(not(feature = "synap2p"))]
pub type PeerId = String;

#[cfg(not(feature = "synap2p"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkEvent {
    DirectMessageReceived { peer: PeerId, data: Vec<u8> },
    GossipMessageReceived { topic: String, data: Vec<u8> },
    ProviderFound { key: String, providers: Vec<PeerId> },
}

#[cfg(not(feature = "synap2p"))]
pub trait NodeClientLike {
    type Error: Display;

    fn send_direct_message(
        &self,
        peer: PeerId,
        data: Vec<u8>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn publish_message(
        &self,
        topic: String,
        data: Vec<u8>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn announce_provider(&self, key: String) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn find_providers(
        &self,
        key: String,
    ) -> impl Future<Output = Result<Vec<PeerId>, Self::Error>> + Send;
}

#[cfg(feature = "synap2p")]
pub trait NodeClientLike {
    type Error: Display;

    fn send_direct_message(
        &self,
        peer: PeerId,
        data: Vec<u8>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn publish_message(
        &self,
        topic: String,
        data: Vec<u8>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn announce_provider(&self, key: String) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn find_providers(
        &self,
        key: String,
    ) -> impl Future<Output = Result<Vec<PeerId>, Self::Error>> + Send;
}

#[cfg(feature = "synap2p")]
impl NodeClientLike for NodeClient {
    type Error = synap2p::Error;

    async fn send_direct_message(&self, peer: PeerId, data: Vec<u8>) -> Result<(), Self::Error> {
        self.send_direct_message(peer, data).await
    }

    async fn publish_message(&self, topic: String, data: Vec<u8>) -> Result<(), Self::Error> {
        self.publish_message(topic, data).await
    }

    async fn announce_provider(&self, key: String) -> Result<(), Self::Error> {
        self.announce_provider(key).await
    }

    async fn find_providers(&self, key: String) -> Result<Vec<PeerId>, Self::Error> {
        self.find_providers(key).await
    }
}

pub async fn handle_net_request<C>(
    header: &DiarsabaHeader,
    payload: &[u8],
    client: &C,
) -> Vec<u8>
where
    C: NodeClientLike,
{
    let Some((&opcode, rest)) = payload.split_first() else {
        return build_error_response(header.req_id, "payload too short for net request");
    };

    match opcode {
        NET_DIRECT_MSG => {
            let (peer_bytes, data) = match split_len_prefixed(rest) {
                Ok(parts) => parts,
                Err(error) => return build_error_response(header.req_id, error),
            };
            let peer = match parse_peer_id(peer_bytes) {
                Ok(peer) => peer,
                Err(error) => return build_error_response(header.req_id, &error),
            };

            match client.send_direct_message(peer, data.to_vec()).await {
                Ok(()) => build_success_response(header.req_id, Vec::new()),
                Err(error) => build_error_response(header.req_id, &error.to_string()),
            }
        }
        NET_PUBLISH => {
            let (topic_bytes, data) = match split_len_prefixed(rest) {
                Ok(parts) => parts,
                Err(error) => return build_error_response(header.req_id, error),
            };
            let topic = String::from_utf8_lossy(topic_bytes).into_owned();

            match client.publish_message(topic, data.to_vec()).await {
                Ok(()) => build_success_response(header.req_id, Vec::new()),
                Err(error) => build_error_response(header.req_id, &error.to_string()),
            }
        }
        NET_ANNOUNCE => {
            let (key_bytes, trailing) = match split_len_prefixed(rest) {
                Ok(parts) => parts,
                Err(error) => return build_error_response(header.req_id, error),
            };
            if !trailing.is_empty() {
                return build_error_response(header.req_id, "unexpected trailing bytes in announce request");
            }
            let key = String::from_utf8_lossy(key_bytes).into_owned();

            match client.announce_provider(key).await {
                Ok(()) => build_success_response(header.req_id, Vec::new()),
                Err(error) => build_error_response(header.req_id, &error.to_string()),
            }
        }
        NET_FIND => {
            let (key_bytes, trailing) = match split_len_prefixed(rest) {
                Ok(parts) => parts,
                Err(error) => return build_error_response(header.req_id, error),
            };
            if !trailing.is_empty() {
                return build_error_response(header.req_id, "unexpected trailing bytes in find request");
            }
            let key = String::from_utf8_lossy(key_bytes).into_owned();

            match client.find_providers(key).await {
                Ok(providers) => match encode_peer_list(&providers) {
                    Ok(encoded) => build_success_response(header.req_id, encoded),
                    Err(error) => build_error_response(header.req_id, error),
                },
                Err(error) => build_error_response(header.req_id, &error.to_string()),
            }
        }
        _ => build_error_response(header.req_id, "unsupported net opcode"),
    }
}

pub fn pack_network_event(event: NetworkEvent) -> Vec<Vec<u8>> {
    match event {
        NetworkEvent::DirectMessageReceived { peer, data } => {
            let peer_bytes = peer_id_to_bytes(&peer);
            match build_event_payload_with_tail(EVT_DIRECT_MSG, &peer_bytes, &data) {
                Some(frame) => vec![frame],
                None => Vec::new(),
            }
        }
        NetworkEvent::GossipMessageReceived { topic, data } => {
            match build_event_payload_with_tail(EVT_GOSSIP_MSG, topic.as_bytes(), &data) {
                Some(frame) => vec![frame],
                None => Vec::new(),
            }
        }
        NetworkEvent::ProviderFound { key, providers } => providers
            .into_iter()
            .filter_map(|peer| {
                let key_bytes = key.as_bytes();
                let peer_bytes = peer_id_to_bytes(&peer);
                build_provider_found_frame(key_bytes, &peer_bytes)
            })
            .collect(),
    }
}

fn split_len_prefixed(data: &[u8]) -> Result<(&[u8], &[u8]), &'static str> {
    let Some((&field_len, rest)) = data.split_first() else {
        return Err("missing length-prefixed field");
    };

    let field_len = usize::from(field_len);
    let field = rest
        .get(..field_len)
        .ok_or("length-prefixed field exceeds payload")?;
    let trailing = rest
        .get(field_len..)
        .ok_or("invalid trailing payload slice")?;

    Ok((field, trailing))
}

#[cfg(feature = "synap2p")]
fn parse_peer_id(data: &[u8]) -> Result<PeerId, String> {
    let peer = String::from_utf8_lossy(data);
    PeerId::from_str(peer.as_ref()).map_err(|error| error.to_string())
}

#[cfg(not(feature = "synap2p"))]
fn parse_peer_id(data: &[u8]) -> Result<PeerId, String> {
    Ok(String::from_utf8_lossy(data).into_owned())
}

fn encode_peer_list(providers: &[PeerId]) -> Result<Vec<u8>, &'static str> {
    let mut payload = Vec::new();

    for peer in providers {
        let peer_bytes = peer_id_to_bytes(peer);
        let peer_len = u8::try_from(peer_bytes.len()).map_err(|_| "peer id exceeds u8 length")?;
        payload.push(peer_len);
        payload.extend_from_slice(&peer_bytes);
    }

    Ok(payload)
}

fn build_event_payload_with_tail(opcode: u8, field: &[u8], tail: &[u8]) -> Option<Vec<u8>> {
    let field_len = u8::try_from(field.len()).ok()?;
    let mut payload = Vec::with_capacity(2 + field.len() + tail.len());
    payload.push(opcode);
    payload.push(field_len);
    payload.extend_from_slice(field);
    payload.extend_from_slice(tail);
    build_push_frame(payload)
}

fn build_provider_found_frame(key: &[u8], peer: &[u8]) -> Option<Vec<u8>> {
    let key_len = u8::try_from(key.len()).ok()?;
    let peer_len = u8::try_from(peer.len()).ok()?;

    let mut payload = Vec::with_capacity(3 + key.len() + peer.len());
    payload.push(EVT_PROVIDER_FOUND);
    payload.push(key_len);
    payload.extend_from_slice(key);
    payload.push(peer_len);
    payload.extend_from_slice(peer);
    build_push_frame(payload)
}

fn build_push_frame(payload: Vec<u8>) -> Option<Vec<u8>> {
    let payload_len = u16::try_from(payload.len()).ok()?;
    let header = DiarsabaHeader {
        domain: DOMAIN_EVENT,
        flags: 0x00,
        req_id: 0,
        payload_len,
    };

    Some(build_response(header, payload))
}

fn build_success_response(req_id: u32, payload: Vec<u8>) -> Vec<u8> {
    match u16::try_from(payload.len()) {
        Ok(payload_len) => build_response(
            DiarsabaHeader {
                domain: DOMAIN_NET,
                flags: 0x00,
                req_id,
                payload_len,
            },
            payload,
        ),
        Err(_) => build_error_response(req_id, "response payload exceeds u16 length"),
    }
}

fn build_error_response(req_id: u32, message: &str) -> Vec<u8> {
    let payload = message.as_bytes();
    let payload_len = u16::try_from(payload.len()).unwrap_or(u16::MAX);
    let payload = if payload.len() > usize::from(u16::MAX) {
        payload[..usize::from(u16::MAX)].to_vec()
    } else {
        payload.to_vec()
    };

    build_response(
        DiarsabaHeader {
            domain: DOMAIN_NET,
            flags: FLAG_ERROR,
            req_id,
            payload_len,
        },
        payload,
    )
}

fn build_response(header: DiarsabaHeader, payload: Vec<u8>) -> Vec<u8> {
    let mut frame = Vec::with_capacity(8 + payload.len());
    frame.extend_from_slice(&build_header(&header));
    frame.extend_from_slice(&payload);
    frame
}

#[cfg(feature = "synap2p")]
fn peer_id_to_bytes(peer: &PeerId) -> Vec<u8> {
    peer.to_string().into_bytes()
}

#[cfg(not(feature = "synap2p"))]
fn peer_id_to_bytes(peer: &PeerId) -> Vec<u8> {
    peer.as_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Mutex;
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    use super::*;
    use crate::protocol::parse_header;

    #[derive(Default)]
    struct MockClient {
        direct_calls: Mutex<Vec<(PeerId, Vec<u8>)>>,
        publish_calls: Mutex<Vec<(String, Vec<u8>)>>,
        announce_calls: Mutex<Vec<String>>,
        find_calls: Mutex<Vec<String>>,
        find_response: Mutex<Vec<PeerId>>,
    }

    impl NodeClientLike for MockClient {
        type Error = String;

        async fn send_direct_message(&self, peer: PeerId, data: Vec<u8>) -> Result<(), Self::Error> {
            self.direct_calls.lock().expect("lock poisoned").push((peer, data));
            Ok(())
        }

        async fn publish_message(&self, topic: String, data: Vec<u8>) -> Result<(), Self::Error> {
            self.publish_calls.lock().expect("lock poisoned").push((topic, data));
            Ok(())
        }

        async fn announce_provider(&self, key: String) -> Result<(), Self::Error> {
            self.announce_calls.lock().expect("lock poisoned").push(key);
            Ok(())
        }

        async fn find_providers(&self, key: String) -> Result<Vec<PeerId>, Self::Error> {
            self.find_calls.lock().expect("lock poisoned").push(key);
            Ok(self.find_response.lock().expect("lock poisoned").clone())
        }
    }

    #[test]
    fn handle_direct_message_request_returns_empty_success_frame() {
        let header = DiarsabaHeader {
            domain: DOMAIN_NET,
            flags: 0,
            req_id: 77,
            payload_len: 0,
        };
        let client = MockClient::default();
        let payload = [NET_DIRECT_MSG, 6, b'p', b'e', b'e', b'r', b'-', b'1', 1, 2, 3];

        let frame = block_on(handle_net_request(&header, &payload, &client));
        let response_header = parse_header(&frame[..8]).expect("header should parse");

        assert_eq!(response_header.domain, DOMAIN_NET);
        assert_eq!(response_header.flags, 0);
        assert_eq!(response_header.req_id, 77);
        assert_eq!(response_header.payload_len, 0);
        assert_eq!(frame.len(), 8);
        assert_eq!(
            client.direct_calls.lock().expect("lock poisoned").as_slice(),
            &[(String::from("peer-1"), vec![1, 2, 3])]
        );
    }

    #[test]
    fn handle_find_request_encodes_provider_list() {
        let header = DiarsabaHeader {
            domain: DOMAIN_NET,
            flags: 0,
            req_id: 9,
            payload_len: 0,
        };
        let client = MockClient {
            find_response: Mutex::new(vec![String::from("peer-a"), String::from("peer-b")]),
            ..MockClient::default()
        };
        let payload = [NET_FIND, 3, b'k', b'e', b'y'];

        let frame = block_on(handle_net_request(&header, &payload, &client));
        let response_header = parse_header(&frame[..8]).expect("header should parse");

        assert_eq!(response_header.flags, 0);
        assert_eq!(response_header.req_id, 9);
        assert_eq!(
            &frame[8..],
            &[6, b'p', b'e', b'e', b'r', b'-', b'a', 6, b'p', b'e', b'e', b'r', b'-', b'b']
        );
        assert_eq!(
            client.find_calls.lock().expect("lock poisoned").as_slice(),
            &[String::from("key")]
        );
    }

    #[test]
    fn handle_announce_rejects_trailing_bytes() {
        let header = DiarsabaHeader {
            domain: DOMAIN_NET,
            flags: 0,
            req_id: 5,
            payload_len: 0,
        };
        let client = MockClient::default();
        let payload = [NET_ANNOUNCE, 3, b'k', b'e', b'y', 0xff];

        let frame = block_on(handle_net_request(&header, &payload, &client));
        let response_header = parse_header(&frame[..8]).expect("header should parse");

        assert_eq!(response_header.flags, FLAG_ERROR);
        assert_eq!(response_header.req_id, 5);
        assert_eq!(
            std::str::from_utf8(&frame[8..]).expect("valid utf8 error"),
            "unexpected trailing bytes in announce request"
        );
    }

    #[test]
    fn pack_provider_found_emits_one_frame_per_peer() {
        let frames = pack_network_event(NetworkEvent::ProviderFound {
            key: String::from("cell"),
            providers: vec![String::from("peer-1"), String::from("peer-2")],
        });

        assert_eq!(frames.len(), 2);

        let first_header = parse_header(&frames[0][..8]).expect("header should parse");
        assert_eq!(first_header.domain, DOMAIN_EVENT);
        assert_eq!(first_header.flags, 0);
        assert_eq!(first_header.req_id, 0);
        assert_eq!(
            &frames[0][8..],
            &[EVT_PROVIDER_FOUND, 4, b'c', b'e', b'l', b'l', 6, b'p', b'e', b'e', b'r', b'-', b'1']
        );
        assert_eq!(
            &frames[1][8..],
            &[EVT_PROVIDER_FOUND, 4, b'c', b'e', b'l', b'l', 6, b'p', b'e', b'e', b'r', b'-', b'2']
        );
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