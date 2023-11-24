use std::time::SystemTime;

#[derive(Clone, Debug)]
pub enum MetricValue {
    Counter(u64),
    Gauge(i64),
    Rate(i64),
    String(Box<str>)
}

#[derive(Eq, Hash, PartialEq, Debug, Clone)]
pub enum MetricSource {
    Listener(Box<str>),
    ListenerProtocol(Box<str>),
    VirtualHost(Box<str>),
    Route(Box<str>),
    Cluster(Box<str>),
    ClusterMember(Box<str>)
}

#[derive(Debug)]
pub struct MetricEntry {
    pub metric: MetricValue,
    pub timestamp: SystemTime,
    pub last_value: i64,
    pub current_value: i64
}
