use tokio::sync::oneshot::Sender;
use crate::configs::{tls, listener, cluster, metric};
use crate::configs::buffer::{StrictBufferWriter, StrictBufferReader};

// Config messages
#[derive(Clone, Debug)]
pub enum ConfigUpdate {
    ListenerConfig(listener::ListenerConfig),
    TlsConfig(tls::TlsConfig),
    ClusterConfig(cluster::ClusterConfig),
    RemoveCluster(Box<str>),
    RemoveListener(Box<str>),
    NotExist,
}

pub struct ConfigRequest {
    pub requester: Sender<ConfigUpdate>,
    pub request_type: ConfigRequestType

}

pub enum ConfigRequestType {
    ListenerConfig(Box<str>),
    TlsConfig(Box<str>)
}

// Cluster messages
pub enum ClusterMessage {
    ConfigUpdate(ConfigUpdate),
    ClusterConnection(
        Box<str>,
        Box<str>,
        listener::RouteConfig,
        StrictBufferReader,
        Sender<ListenerConnection>
    ),
    ClusterConnectionClosed(Box<str>, Box<str>)
}

// Listener messages
pub enum ListenerConnection {
    ListenerBuffer(StrictBufferReader),
    ClusterNotFound,
    NoAvailableMember,
    BufferOverLimit
}

// Buffer messages
pub enum BufferMessage {
    ConfigUpdate(ConfigUpdate),
    BufferRequest(BufferRequestMessage)
}

pub struct BufferRequestMessage {
    pub request: BufferRequest,
    pub requester: Sender<BufferResponseMessage>
}

pub enum BufferRequest {
    RequestListener(Box<str>, usize),
    RequestCluster(Box<str>, usize),
    ReleaseListener(Box<str>, usize),
    ReleaseCluster(Box<str>, usize)
}

#[derive(Debug)]
pub enum BufferResponseMessage {
    Buffer((StrictBufferWriter, StrictBufferReader)),
    OverLimit
}

// Metric messages
#[derive(Debug)]
pub struct MetricMessage {
    pub scope: Vec<metric::MetricSource>,
    pub name: Box<str>,
    pub value: metric::MetricValue
}
