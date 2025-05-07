//! P2P message types

use std::{collections::HashSet, num::NonZeroUsize, net::SocketAddr};

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

use crate::{
    net::peer::{PeerState, PeerStateId},
    types::{AuthorizedTransaction, BlockHash, Body, Header, Tip, Txid, Block},
    net::peer_management::{PeerInfo, PeerServices},
};

#[derive(BorshSerialize, Clone, Debug, Deserialize, Serialize)]
pub struct Heartbeat(pub PeerState);

#[derive(BorshSerialize, Clone, Debug, Deserialize, Serialize)]
pub struct GetBlockRequest {
    pub block_hash: BlockHash,
    /// Mainchain descendant tip that we are requesting the block to reach.
    /// Only relevant for the requester, so serialization is skipped
    #[borsh(skip)]
    #[serde(skip)]
    pub descendant_tip: Option<Tip>,
    /// Ancestor block. If no bodies are missing between `descendant_tip`
    /// and `ancestor`, then `descendant_tip` is ready to apply.
    /// Only relevant for the requester, so serialization is skipped
    #[borsh(skip)]
    #[serde(skip)]
    pub ancestor: Option<BlockHash>,
    /// Only relevant for the requester, so serialization is skipped
    #[borsh(skip)]
    #[serde(skip)]
    pub peer_state_id: Option<PeerStateId>,
}

impl GetBlockRequest {
    /// Limit bytes to read in a response to a request
    pub const fn read_response_limit(&self) -> NonZeroUsize {
        // 10MB limit for blocks
        NonZeroUsize::new(10 * 1024 * 1024).unwrap()
    }
}

/// Request headers up to [`end`]
#[derive(BorshSerialize, Clone, Debug, Deserialize, Serialize)]
pub struct GetHeadersRequest {
    /// Request headers AFTER (not including) the first ancestor found in
    /// the specified list, if such an ancestor exists.
    pub start: HashSet<BlockHash>,
    pub end: BlockHash,
    /// Height is only relevant for the requester,
    /// so serialization is skipped
    #[borsh(skip)]
    #[serde(skip)]
    pub height: Option<u32>,
    /// Only relevant for the requester, so serialization is skipped
    #[borsh(skip)]
    #[serde(skip)]
    pub peer_state_id: Option<PeerStateId>,
}

impl GetHeadersRequest {
    /// Limit bytes to read in a response to a request
    pub const fn read_response_limit(&self) -> NonZeroUsize {
        // 2KB limit per header
        const READ_HEADER_LIMIT: NonZeroUsize =
            NonZeroUsize::new(2048).unwrap();
        let expected_headers = self.height.expect(
            "GetHeaders height should always be Some in an outbound request",
        ) as usize
            + 1;
        NonZeroUsize::new(expected_headers)
            .unwrap()
            .checked_mul(READ_HEADER_LIMIT)
            .unwrap()
    }
}

#[derive(BorshSerialize, Clone, Debug, Deserialize, Serialize)]
pub struct PushTransactionRequest {
    pub transaction: AuthorizedTransaction,
}

impl PushTransactionRequest {
    /// Limit bytes to read in a response to a request
    pub const fn read_response_limit(&self) -> NonZeroUsize {
        // 256B limit per tx ack (response size is ~192)
        NonZeroUsize::new(256).unwrap()
    }
}

/// Message containing a list of peer addresses
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Serialize, Deserialize)]
pub struct AddrMessage {
    /// List of peer addresses with their services
    pub addresses: Vec<PeerInfo>,
}

impl From<Vec<PeerInfo>> for AddrMessage {
    fn from(addresses: Vec<PeerInfo>) -> Self {
        Self { addresses }
    }
}

#[derive(BorshSerialize, Clone, Debug)]
pub enum Request {
    GetBlock(GetBlockRequest),
    GetHeaders(GetHeadersRequest),
    PushTransaction(PushTransactionRequest),
    /// Request to send peer addresses
    GetAddr,
    /// Message containing peer addresses
    Addr(AddrMessage),
}

impl Request {
    /// Limit bytes to read in a response to a request
    pub const fn read_response_limit(&self) -> NonZeroUsize {
        match self {
            Self::GetBlock(request) => request.read_response_limit(),
            Self::GetHeaders(request) => request.read_response_limit(),
            Self::PushTransaction(request) => request.read_response_limit(),
            Self::GetAddr => NonZeroUsize::new(1).unwrap(),
            Self::Addr(_) => NonZeroUsize::new(1).unwrap(),
        }
    }
}

impl From<GetBlockRequest> for Request {
    fn from(request: GetBlockRequest) -> Self {
        Self::GetBlock(request)
    }
}

impl From<GetHeadersRequest> for Request {
    fn from(request: GetHeadersRequest) -> Self {
        Self::GetHeaders(request)
    }
}

impl From<PushTransactionRequest> for Request {
    fn from(request: PushTransactionRequest) -> Self {
        Self::PushTransaction(request)
    }
}

impl From<AddrMessage> for Request {
    fn from(message: AddrMessage) -> Self {
        Self::Addr(message)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum RequestMessageRef<'a> {
    Heartbeat(&'a Heartbeat),
    Request(&'a Request),
}

impl RequestMessageRef<'_> {
    /// Limit bytes to read in a response to a request.
    /// Returns `None` if no response is expected
    pub fn read_response_limit(&self) -> Option<NonZeroUsize> {
        // TODO: Add constant for discriminant
        match self {
            Self::Heartbeat(_) => None,
            Self::Request(request) => Some(request.read_response_limit()),
        }
    }
}

impl<'a> From<&'a Heartbeat> for RequestMessageRef<'a> {
    fn from(heartbeat: &'a Heartbeat) -> Self {
        Self::Heartbeat(heartbeat)
    }
}

impl<'a> From<&'a Request> for RequestMessageRef<'a> {
    fn from(request: &'a Request) -> Self {
        Self::Request(request)
    }
}

impl<'a> Serialize for RequestMessageRef<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(Serialize)]
        enum Repr<'b> {
            Heartbeat(&'b Heartbeat),
            GetBlock(&'b GetBlockRequest),
            GetHeaders(&'b GetHeadersRequest),
            PushTransaction(&'b PushTransactionRequest),
            GetAddr,
            Addr(&'b AddrMessage),
        }

        let repr = match self {
            RequestMessageRef::Heartbeat(heartbeat) => {
                Repr::Heartbeat(heartbeat)
            }
            RequestMessageRef::Request(request) => match request {
                Request::GetBlock(request) => Repr::GetBlock(request),
                Request::GetHeaders(request) => Repr::GetHeaders(request),
                Request::PushTransaction(request) => {
                    Repr::PushTransaction(request)
                }
                Request::GetAddr => Repr::GetAddr,
                Request::Addr(message) => Repr::Addr(message),
            },
        };
        repr.serialize(serializer)
    }
}

#[allow(clippy::duplicated_attributes)]
#[derive(transitive::Transitive, Debug)]
#[transitive(
    from(GetBlockRequest, Request),
    from(GetHeadersRequest, Request),
    from(PushTransactionRequest, Request),
    from(AddrMessage, Request)
)]
pub enum RequestMessage {
    Heartbeat(Heartbeat),
    Request(Request),
}

impl RequestMessage {
    pub fn as_ref<'a>(&'a self) -> RequestMessageRef<'a> {
        match self {
            Self::Heartbeat(heartbeat) => {
                RequestMessageRef::Heartbeat(heartbeat)
            }
            Self::Request(request) => RequestMessageRef::Request(request),
        }
    }
}

impl<'a> From<&'a RequestMessage> for RequestMessageRef<'a> {
    fn from(owned: &'a RequestMessage) -> Self {
        owned.as_ref()
    }
}

impl From<Heartbeat> for RequestMessage {
    fn from(heartbeat: Heartbeat) -> Self {
        Self::Heartbeat(heartbeat)
    }
}

impl From<Request> for RequestMessage {
    fn from(request: Request) -> Self {
        Self::Request(request)
    }
}

impl<'de> Deserialize<'de> for RequestMessage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        enum Repr {
            Heartbeat(Heartbeat),
            GetBlock(GetBlockRequest),
            GetHeaders(GetHeadersRequest),
            PushTransaction(PushTransactionRequest),
            GetAddr,
            Addr(AddrMessage),
        }
        let res = match Repr::deserialize(deserializer)? {
            Repr::Heartbeat(heartbeat) => heartbeat.into(),
            Repr::GetBlock(request) => request.into(),
            Repr::GetHeaders(request) => request.into(),
            Repr::PushTransaction(request) => request.into(),
            Repr::GetAddr => Request::GetAddr.into(),
            Repr::Addr(message) => message.into(),
        };
        Ok(res)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseMessage {
    Block(Block),
    Headers(Vec<Header>),
    TransactionAccepted(Txid),
    TransactionRejected(String),
    Addr(AddrMessage),
    Empty,
    NoBlock { block_hash: BlockHash },
    NoHeader { block_hash: BlockHash },
}

impl ResponseMessage {
    fn fmt_headers(headers: &Vec<Header>, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let [first, .., last] = headers.as_slice() {
            write!(f, "[{first:?}, .., {last:?}]")
        } else {
            std::fmt::Debug::fmt(headers, f)
        }
    }
}

impl From<Block> for ResponseMessage {
    fn from(block: Block) -> Self {
        Self::Block(block)
    }
}

impl From<Vec<Header>> for ResponseMessage {
    fn from(headers: Vec<Header>) -> Self {
        Self::Headers(headers)
    }
}

impl From<Txid> for ResponseMessage {
    fn from(txid: Txid) -> Self {
        Self::TransactionAccepted(txid)
    }
}

impl From<String> for ResponseMessage {
    fn from(reason: String) -> Self {
        Self::TransactionRejected(reason)
    }
}

impl From<AddrMessage> for ResponseMessage {
    fn from(message: AddrMessage) -> Self {
        Self::Addr(message)
    }
}

impl From<()> for ResponseMessage {
    fn from(_: ()) -> Self {
        Self::Empty
    }
}