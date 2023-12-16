use log::debug;
use std::{net::SocketAddr, io};
use std::sync::Arc;
use std::collections::HashMap;
use tokio_rustls::{self, rustls};
use rustls_pki_types;
use tokio::time::{Duration, sleep};
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Receiver;
use tokio_icmp_echo::Pinger;
use rand::random;

use crate::configs::{message, cluster, terms, metric};
use crate::managers::common;
use crate::workers::connections::http;

const TIMEOUT: u8 = 10;

#[derive(Clone)]
pub struct Member {
    pub cluster: Box<str>,
    pub socket_address: SocketAddr,
    pub tls_config: cluster::ClusterTlsConfig,
    pub keepalive: Option<cluster::Keepalive>
}

impl Member {
    pub fn new(
        new_cluster: Box<str>,
        new_socket_address: SocketAddr,
        new_keepalive: Option<cluster::Keepalive>,
        tls: cluster::ClusterTlsConfig
    ) -> Self {
        Self {
            cluster: new_cluster,
            socket_address: new_socket_address,
            tls_config: tls,
            keepalive: new_keepalive.into()
        }
    }
}

pub async fn run_member(
    self_member: Member,
    statuses: Arc<RwLock<HashMap<Box<str>, cluster::ClusterMemberStatus>>>,
    new_config_receiver: Receiver<message::ClusterMessage>,
) -> io::Result<()> {
    debug!("Starting cluster {:?} member {:?}", self_member.cluster, self_member.socket_address);
    let tls_config: Option<rustls::ClientConfig>;
    let mut sni: Option<Box<str>> = None;
    match self_member.tls_config {
        cluster::ClusterTlsConfig::None => {
            tls_config = None;
        },
        cluster::ClusterTlsConfig::Sni(ref new_sni, ref global_config) => {
            if let Ok(global_tls_config) = global_config.clone().get_client_config() {
                tls_config = Some(global_tls_config);
                sni = Some(new_sni.clone());
            } else {
                debug!("Failed to create client tls config");
                tls_config = None;
            }
        },
        cluster::ClusterTlsConfig::TransparentSni(ref global_config) => {
            if let Ok(global_tls_config) = global_config.clone().get_client_config() {
                tls_config = Some(global_tls_config);
            } else {
                debug!("Failed to create client tls config");
                tls_config = None;
            }
        }
    }
    let member = Arc::new(RwLock::new(self_member));
    let mut config_receiver = new_config_receiver;
    let mut checker_handle: Option<JoinHandle<Result<(),io::Error>>> = None;
    if member.read().await.keepalive.is_some() {
        checker_handle = Some(start_checker(statuses.clone(), member.clone()).await);
    }
    loop {
        let res = config_receiver.recv().await;
        if let Some(update) = res {
            match update {
                message::ClusterMessage::ConfigUpdate(config_update) => {
                    match config_update {
                        message::ConfigUpdate::ClusterConfig(new_config) => {
                            let new_keepalive = new_config.keepalive;
                            if new_keepalive != member.read().await.keepalive {
                                member.write().await.keepalive = new_keepalive;
                                if member.read().await.keepalive.is_some() && checker_handle.is_none() {
                                    checker_handle = Some(start_checker(statuses.clone(), member.clone()).await);
                                } else if member.read().await.keepalive.is_none() && checker_handle.is_some() {
                                    let _ = checker_handle.unwrap().await;
                                    checker_handle = None;
                                }
                            }
                        },
                        message::ConfigUpdate::RemoveCluster(_) => {
                            if checker_handle.is_some() {
                                member.write().await.keepalive = None;
                                let _ = checker_handle.unwrap().await;
                            }
                            return Ok(())
                        }
                        _ => {}
                    }
                },
                message::ClusterMessage::ClusterConnection(
                    cluster,
                    client_sni,
                    config,
                    client,
                    client_receiver
                ) => {
                    let conn = TcpStream::connect(member.read().await.socket_address).await;
                    if let Ok(cluster_conn) = conn {
                        if let Some(ref tls) = tls_config {
                            debug!("Starting TLS for backend");
                            let server_sni: rustls_pki_types::ServerName;
                            if let Some(ref new_sni) = sni {
                                server_sni = rustls_pki_types::ServerName::try_from(
                                        new_sni.clone().into_string()
                                    ).unwrap();
                            } else {
                                server_sni = rustls_pki_types::ServerName::try_from(
                                        client_sni.into_string()
                                    ).unwrap();
                            }
                            let connector = tokio_rustls::TlsConnector::from(Arc::new(tls.clone()));
                            debug!("Backend SNI: {:?}", server_sni);
                            if let Ok(tls_conn)  = connector.connect(server_sni, cluster_conn).await {
                                let member_name = member.read().await.socket_address.to_string().into();
                                let _ = tokio::spawn(async move {
                                        http::process_cluster(
                                            tls_conn,
                                            config,
                                            cluster,
                                            member_name,
                                            client_receiver,
                                            client
                                        ).await
                                    }).await;
                            } else {
                                debug!("Failed to connect to backend");
                                let _ = client_receiver.send(message::ListenerConnection::NoAvailableMember);
                            }
                        } else {
                            let member_name = member.read().await.socket_address.to_string().into();
                            let _ = tokio::spawn(async move {
                                    http::process_cluster(
                                        cluster_conn,
                                        config,
                                        cluster,
                                        member_name,
                                        client_receiver,
                                        client
                                    ).await
                                }).await;
                        }
                    } else {
                        debug!("Failed to connect to backend");
                        let _ = client_receiver.send(message::ListenerConnection::NoAvailableMember);
                    }
                },
                _ => {}
            }
        }
    }
}

async fn checker(
    member: Arc<RwLock<Member>>,
    statuses: Arc<RwLock<HashMap<Box<str>,
    cluster::ClusterMemberStatus>>>
) -> io::Result<()> {
    let metric_sender = common::METRIC.read().await.as_ref().unwrap().clone();
    let mut check_counter: i64 = 0;
    let upstream_status = statuses.clone().read().await.get(member.read().await.socket_address.to_string().as_str().into()).unwrap().clone();
    loop {
        if let Some(ref keepalive) = member.read().await.keepalive {
            let status: Option<Duration>; 
            let common_config: cluster::CommonKeepaliveConfig;
            match keepalive {
                cluster::Keepalive::TcpKeepalive(config) => {
                    common_config = config.common_config.clone();
                    status = tcp_checker(member.read().await.socket_address).await;
                },
                cluster::Keepalive::IcmpKeepalive(config) => {
                    common_config = config.common_config.clone();
                    status = icmp_checker(member.read().await.socket_address).await;
                },
                _ => {
                    status = Some(Duration::from_secs(0));
                    common_config = cluster::CommonKeepaliveConfig {
                        interval: 0,
                        dead_interval: 0,
                        live_interval: 0
                    }
                }
            }
            if let Some(rtt) = status {
                let _ = metric_sender.send(
                    message::MetricMessage {
                        scope: vec![metric::MetricSource::ClusterMember(member.read().await.socket_address.to_string().into())],
                        name: terms::metric::RTT.into(),
                        value: metric::MetricValue::Gauge(rtt.as_millis() as i64)
                    }).await;
                let _ = metric_sender.send(
                    message::MetricMessage {
                        scope: vec![metric::MetricSource::ClusterMember(member.read().await.socket_address.to_string().into())],
                        name: terms::metric::AVAILABILITY.into(),
                        value: metric::MetricValue::String(terms::metric::UP.into())
                    }).await;
                match upstream_status {
                    cluster::ClusterMemberStatus::Unavailable => {
                        check_counter += 1;
                        if check_counter >= common_config.live_interval {
                            check_counter = 0;
                            let _ = statuses.write()
                                .await
                                .insert(
                                    member.read().await.socket_address
                                        .to_string()
                                        .as_str()
                                        .into(),
                                    cluster::ClusterMemberStatus::Active(0)
                                );
                        }
                    },
                    _ => {}
                }
            } else {
                let _ = metric_sender.send(
                    message::MetricMessage {
                        scope: vec![metric::MetricSource::ClusterMember(member.read().await.socket_address.to_string().into())],
                        name: terms::metric::AVAILABILITY.into(),
                        value: metric::MetricValue::String(terms::metric::DOWN.into())
                    }).await;
                match upstream_status {
                    cluster::ClusterMemberStatus::Active(_) => {
                        check_counter += 1;
                        if check_counter >= common_config.dead_interval {
                            check_counter = 0;
                            let _ = statuses.write()
                                .await
                                .insert(
                                    member.read().await.socket_address
                                        .to_string()
                                        .as_str()
                                        .into(),
                                    cluster::ClusterMemberStatus::Unavailable
                                );
                        }
                    },
                    _ => {}
                }
            }
            sleep(Duration::from_secs(common_config.interval as u64)).await;
        } else {
            return Ok(())
        }
    }
}

async fn start_checker(
    statuses: Arc<RwLock<HashMap<Box<str>, cluster::ClusterMemberStatus>>>,
    member: Arc<RwLock<Member>>
) -> JoinHandle<io::Result<()>> {
    let check_statuses = statuses.clone();
    let local_member = member.clone();
    return tokio::spawn(
        async move {
            checker(local_member, check_statuses).await
        }
    );

}

async fn icmp_checker(addr: SocketAddr) -> Option<Duration> {
    debug!("Starting icmp checker: {:?}", addr);
    if let Ok(pinger) = Pinger::new().await {
        match pinger.ping(
            addr.ip(),
            random::<u16>(),
            0,
            Duration::from_secs(TIMEOUT.into())
        ).await {
            Ok(result) => {
                if let Some(duration) = result {
                    Some(duration)
                } else {
                    None
                }
            },
            Err(_) => {
                debug!("Failed to send icmp probe");
                None
            }
        }
    } else {
        debug!("Failed to create icmp probe");
        None
    }
}

async fn tcp_checker(addr: SocketAddr) -> Option<Duration> {
    debug!("Starting tcp checker: {:?}", addr);
    if let Ok(pinger) = Pinger::new().await {
        match pinger.ping(
            addr.ip(),
            random::<u16>(),
            0,
            Duration::from_secs(TIMEOUT.into())
        ).await {
            Ok(result) => {
                if let Some(duration) = result {
                    Some(duration)
                } else {
                    None
                }
            },
            Err(_) => {
                debug!("Failed to send icmp probe");
                None
            }
        }
    } else {
        debug!("Failed to create icmp probe");
        None
    }
}
