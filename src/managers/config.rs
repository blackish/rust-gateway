use log::{info, debug};
use std::io;
use tokio::sync::mpsc;
use tokio::fs;
use yaml_rust::{YamlLoader, Yaml};
use std::collections::HashMap;

use crate::configs::{cluster, listener, message, tls};
use crate::configs::terms::common;
use crate::managers::common::{LISTENER, CLUSTER, BUFFER};

pub struct ConfigManager {
    request_receiver: mpsc::Receiver<message::ConfigRequest>,
    listeners: HashMap<Box<str>, listener::ListenerConfig>,
    tls: HashMap<Box<str>, tls::TlsConfig>,
    clusters: HashMap<Box<str>, cluster::ClusterConfig>
}

impl ConfigManager {
    pub fn new(new_request_receiver: mpsc::Receiver<message::ConfigRequest>) -> Self {
        Self{
            request_receiver: new_request_receiver,
            listeners: HashMap::new(),
            tls: HashMap::new(),
            clusters: HashMap::new()
        }
    }
    pub async fn start(mut self, config_file_name: &str) -> io::Result<Self> {
        info!("Starting config manager");
        let listener_manager = LISTENER.read().await.as_ref().unwrap().clone();
        let cluster_manager = CLUSTER.read().await.as_ref().unwrap().clone();
        let buffer_manager = BUFFER.read().await.as_ref().unwrap().clone();
        let config_file = fs::read_to_string(config_file_name).await?;
        let config = YamlLoader::load_from_str(&config_file).unwrap();
        match config[0][common::TLS] {
            Yaml::Array(ref tlss_yaml) => {
                debug!("Loading TLS config");
                for tls_yaml in tlss_yaml {
                    if let Yaml::Hash(_) = &tls_yaml {
                        let new_tls_config = tls::TlsConfig::new(tls_yaml)?;
                        self.tls.insert(new_tls_config.name.clone(), new_tls_config.clone());
                        let _ = listener_manager.send(message::ConfigUpdate::TlsConfig(new_tls_config)).await;
                    };
                };
                debug!("Loading TLS done");
            },
            _ => {return Err(io::Error::new(io::ErrorKind::InvalidData, "Failed to parse config"))}
        };
        match config[0][common::LISTENER] {
            Yaml::Array(ref listeners_yaml) => {
                debug!("Loading listeners");
                for listener_yaml in listeners_yaml {
                    if let Yaml::Hash(_) = &listener_yaml {
                        if let Some(new_listener_config) = listener::ListenerConfig::new(&listener_yaml) {
                            self.listeners.insert(new_listener_config.name.clone(), new_listener_config.clone());
                            let _ = listener_manager.send(message::ConfigUpdate::ListenerConfig(new_listener_config.clone())).await;
                            let _ = buffer_manager.send(
                                message::BufferMessage::ConfigUpdate(
                                    message::ConfigUpdate::ListenerConfig(new_listener_config)
                                )
                            ).await;
                        };
                    };
                };
                debug!("Loading listeners done");
            },
            _ => {return Err(io::Error::new(io::ErrorKind::InvalidData, "Failed to parse config"))}
        };
        match config[0][common::CLUSTER] {
            Yaml::Array(ref clusters_yaml) => {
                debug!("Loading clusters");
                for cluster_yaml in clusters_yaml {
                    if let Yaml::Hash(_) = &cluster_yaml {
                        if let Some(new_cluster_config) = cluster::ClusterConfig::new(&cluster_yaml) {
                            self.clusters.insert(new_cluster_config.name.clone(), new_cluster_config.clone());
                            let _ = cluster_manager.send(
                                message::ClusterMessage::ConfigUpdate(
                                    message::ConfigUpdate::ClusterConfig(new_cluster_config.clone())
                                )
                            ).await;
                        };
                    };
                };
                debug!("Loading clusters done");
            },
            _ => {return Err(io::Error::new(io::ErrorKind::InvalidData, "Failed to parse config"))}
        };
        Ok(self)
    }

    pub async fn worker(&mut self) -> io::Result<()> {
        loop {
            let new_request = self.request_receiver.recv().await;
            debug!("Got config request");
            match new_request {
                Some(request) => {
                    if let message::ConfigRequestType::TlsConfig(name) = request.request_type {
                        debug!("Got tls config request: {:?}", name);
                        if let Some(response) = self.tls.get(&name) {
                            let _ = request.requester.send(message::ConfigUpdate::TlsConfig(response.clone()));
                            debug!("Send tls config request");
                        }
                    } else {
                        debug!("No config matching the request");
                        let _ = request.requester.send(message::ConfigUpdate::NotExist);
                    }
                },
                None => {},
            }
            
        }
    }
}
