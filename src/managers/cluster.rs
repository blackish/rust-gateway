use log::{info, debug};
use std::collections::HashMap;
use tokio::sync::mpsc;
use crate::configs::message;
use crate::workers::cluster;

pub struct ClusterManager {
    manager: mpsc::Receiver<message::ClusterMessage>,
    clusters: HashMap<Box<str>, mpsc::Sender<message::ClusterMessage>>
}

impl ClusterManager {
    pub fn new(new_manager: mpsc::Receiver<message::ClusterMessage>) -> Self {
        Self{
            manager: new_manager,
            clusters: HashMap::new()
        }
    }
    pub async fn worker(mut self) {
        info!("Starting cluster manager");
        while let Some(update) = self.manager.recv().await {
            match update {
                message::ClusterMessage::ConfigUpdate(message) => {
                    match message {
                        message::ConfigUpdate::ClusterConfig(cluster) => {
                            if let Some(sender) = self.clusters.get(&cluster.name) {
                                debug!("Got cluster update for: {:?}", cluster.name);
                                let _ = sender.send(
                                    message::ClusterMessage::ConfigUpdate(
                                        message::ConfigUpdate::ClusterConfig(cluster)
                                    )
                                ).await;
                            } else {
                                debug!("Got new cluster: {:?}", cluster.name);
                                let (tx, rx) = mpsc::channel(1);
                                self.clusters.insert(cluster.name.clone(), tx);
                                tokio::spawn(async move {cluster::work(cluster, rx).await});
                            }
                        }
                        message::ConfigUpdate::RemoveCluster(cluster) => {
                            debug!("Got cluster remove for: {:?}", cluster);
                            if let Some(sender) = self.clusters.remove(&cluster) {
                                let _ = sender.send(
                                    message::ClusterMessage::ConfigUpdate(
                                        message::ConfigUpdate::RemoveCluster(cluster)
                                    )
                                ).await;
                            }
                        },
                        _ => {}
                    }
                },
                message::ClusterMessage::ClusterConnectionClosed(ref cluster, _) => {
                    if let Some(sender) = self.clusters.get(cluster) {
                        let _ = sender.send(update).await;
                    }
                },
                message::ClusterMessage::ClusterConnection(ref cluster, _, _, _) => {
                    if let Some(sender) = self.clusters.get(cluster) {
                        let _ = sender.send(update).await;
                    }
                    //FIXME: add nonexistent cluster handler
                }
                _ => {}
            }
        }
        panic!("Cluster manager has paniced");

    }
}
