use yaml_rust::Yaml;
use log::debug;
use std::net::{SocketAddr, ToSocketAddrs};
use crate::configs::terms::{common, cluster};

const DEFAULT_BUFFER: i64 = 1048_578;
const DEFAULT_INTERVAL: i64 = 10;
const DEFAULT_DEAD_INTERVAL: i64 = 3;
const DEFAULT_LIVE_INTERVAL: i64 = 5;
const DEFAULT_WEIGHT: i64 = 1;

#[derive(Clone, Debug)]
pub struct ClusterConfig {
    pub name: Box<str>,
    pub buffer: i64,
    pub lb_method: LbMethod,
    pub tls: ClusterTlsConfig,
    pub keepalive: Option<Keepalive>,
    pub members: Vec<ClusterMemberConfig>
}

#[derive(Clone, Debug)]
pub enum ClusterTlsConfig {
    None,
    TransparentSni(Box<str>),
    Sni(Box<str>, Box<str>)
}

#[derive(Clone, Debug)]
pub enum LbMethod {
    RoundRobin,
    LeastConn
}

#[derive(Eq, PartialEq, Debug, Clone)]
pub enum Keepalive {
    IcmpKeepalive(IcmpKeepaliveConfig),
    TcpKeepalive(TcpKeepaliveConfig),
    HttpKeepalive(HttpKeepaliveConfig)
}

#[derive(Eq, PartialEq, Debug, Clone)]
pub struct CommonKeepaliveConfig {
    pub interval: i64,
    pub dead_interval: i64,
    pub live_interval: i64
}

#[derive(Eq, PartialEq, Debug, Clone)]
pub struct IcmpKeepaliveConfig {
    pub common_config: CommonKeepaliveConfig
}

#[derive(Eq, PartialEq, Debug, Clone)]
pub struct TcpKeepaliveConfig {
    pub common_config: CommonKeepaliveConfig
}

#[derive(Eq, PartialEq, Debug, Clone)]
pub struct HttpKeepaliveConfig {
    pub common_config: CommonKeepaliveConfig,
    pub use_tls: bool,
    pub uri: Box<str>,
    pub response_code: i64
}

#[derive(Clone, Debug)]
pub struct ClusterMemberConfig {
    pub address: SocketAddr,
    pub status: ClusterMemberStatus,
    pub weight: i64
}

#[derive(Clone, Debug, PartialEq)]
pub enum ClusterMemberStatus {
    Active(u16),
    Disabled,
    Unavailable
}

impl ClusterConfig {
    pub fn new(config: &Yaml) -> Option<Self> {
        match config {
            Yaml::Hash(_) => {
                debug!("Loading cluster: {:?}", config[common::NAME].as_str()?);
                let mut result = Self {
                    name: config[common::NAME].as_str()?.into(),
                    buffer: config[common::BUFFER].as_i64().unwrap_or(DEFAULT_BUFFER),
                    lb_method: LbMethod::new(&config[cluster::LB_METHOD]),
                    keepalive: Keepalive::new(&config[cluster::KEEPALIVE]),
                    tls: ClusterTlsConfig::new(&config[cluster::TLS]),
                    members: Vec::new()
                };
                if let Yaml::Array(members_yaml) = &config[cluster::MEMBERS] {
                    for member_yaml in members_yaml {
                        if let Yaml::Hash(member) = member_yaml {
                            if let Yaml::String(ref saddr_str) = member[&Yaml::String(cluster::SOCKET_ADDRESS.into())] {
                                debug!("Loading cluster member: {:?}", saddr_str);
                                if let Ok(mut saddr) = saddr_str.to_socket_addrs() {
                                    result.members.push(
                                        ClusterMemberConfig {
                                            address: saddr.next().unwrap(),
                                            status: ClusterMemberStatus::new(&member_yaml[cluster::STATUS])?,
                                            weight: member_yaml[cluster::WEIGHT].as_i64().unwrap_or(DEFAULT_WEIGHT)
                                        }
                                    );
                                };
                            };
                        };
                    };
                };
                Some(result)

            },
            _ => {None}
        }
    }
}

impl ClusterMemberStatus {
    fn new(status_yaml: &Yaml) -> Option<Self> {
        if let Some(status_text) = status_yaml.as_str() {
            match status_text {
                cluster::ACTIVE => {
                    Some(ClusterMemberStatus::Active(0))
                },
                cluster::DISABLED => {
                    Some(ClusterMemberStatus::Disabled)
                },
                _ => {debug!("Cluster member status not found"); None}
            }
        } else {
            debug!("Cluster member status not found");
            None
        }
    }
}

impl LbMethod {
    fn new(name: &Yaml) -> Self {
        if name.as_str().unwrap_or(cluster::ROUND_ROBIN) == cluster::LEAST_CONN {
            LbMethod::LeastConn
        } else {
            LbMethod::RoundRobin
        }
    }
}

impl ClusterTlsConfig {
    fn new(config: &Yaml) -> Self {
        match config {
            Yaml::Hash(tls_config) => {
                debug!("Reading cluster TLS config");
                if let Some(tls_name) = tls_config.get(&Yaml::String(common::NAME.into())) {
                    if let Yaml::String(tls_name_string) = tls_name {
                        if let Some(sni) = tls_config.get(&Yaml::String(cluster::SNI.into())) {
                            return ClusterTlsConfig::Sni(sni.as_str().unwrap().into(), tls_name_string.as_str().into())
                        } else {
                            return ClusterTlsConfig::TransparentSni(tls_name_string.as_str().into())
                        }
                    } else {
                        return ClusterTlsConfig::None
                    }
                } else {
                    return ClusterTlsConfig::None
                }
            },
            _ => {
                return ClusterTlsConfig::None
            }
        }
    }
}

impl Keepalive {
    fn new(config: &Yaml) -> Option<Self> {
        match config {
            Yaml::Hash(_) => {
                let mut new_common_config = CommonKeepaliveConfig {
                    interval: DEFAULT_INTERVAL,
                    dead_interval: DEFAULT_DEAD_INTERVAL,
                    live_interval: DEFAULT_LIVE_INTERVAL
                };
                if let Yaml::Hash(common_name_yaml) = &config[cluster::COMMON] {
                    if let Yaml::Hash(common_config_yaml) = &common_name_yaml[&Yaml::String(common::CONFIG.into())] {
                        new_common_config.interval = common_config_yaml[&Yaml::String(cluster::INTERVAL.into())].as_i64().unwrap_or(DEFAULT_INTERVAL);
                        new_common_config.dead_interval = common_config_yaml[&Yaml::String(cluster::DEAD_INTERVAL.into())].as_i64().unwrap_or(DEFAULT_DEAD_INTERVAL);
                        new_common_config.live_interval = common_config_yaml[&Yaml::String(cluster::LIVE_INTERVAL.into())].as_i64().unwrap_or(DEFAULT_LIVE_INTERVAL);
                    };
                };
                if let Yaml::Hash(icmp_name_yaml) = &config[cluster::ICMP] {
                    if let Yaml::Hash(_) = icmp_name_yaml[&Yaml::String(common::CONFIG.into())] {
                        return Some(
                            Keepalive::IcmpKeepalive(
                                IcmpKeepaliveConfig{ common_config: new_common_config }
                            )
                        );
                    };
                } else if let Yaml::Hash(tcp_name_yaml) = &config[cluster::TCP] {
                    if let Yaml::Hash(_) = tcp_name_yaml[&Yaml::String(common::CONFIG.into())] {
                        return Some(
                            Keepalive::TcpKeepalive(
                                TcpKeepaliveConfig { common_config: new_common_config }   
                            )
                        )
                    };
                } else if let Yaml::Hash(http_name_yaml) = &config[cluster::HTTP] {
                    if let Yaml::Hash(http_config_yaml) = &http_name_yaml[&Yaml::String(common::CONFIG.into())] {
                        return Some(
                            Keepalive::HttpKeepalive(
                                HttpKeepaliveConfig {
                                    common_config: new_common_config,
                                    use_tls: http_config_yaml[&Yaml::String(cluster::USE_TLS.into())].as_bool().unwrap_or(false),
                                    uri: http_config_yaml[&Yaml::String(cluster::URI.into())].as_str()?.into(),
                                    response_code: http_config_yaml[&Yaml::String(cluster::RESPONSE_CODE.into())].as_i64()?
                                }
                            )
                        );
                    };
                } else {
                    return None;
                };
            },
            _ => {return None;}
        };
        return None;
    }
}


