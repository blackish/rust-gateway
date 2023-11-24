use log::{info, debug};
use std::collections::HashMap;
use tokio::sync::mpsc;
use crate::configs::message;
use crate::workers::listener;

pub struct ListenerManager {
    config_manager: mpsc::Receiver<message::ConfigUpdate>,
    listeners: HashMap<String, mpsc::Sender<message::ConfigUpdate>>
}

impl ListenerManager {
    pub fn new(manager: mpsc::Receiver<message::ConfigUpdate>) -> Self {
        Self{
            config_manager: manager,
            listeners: HashMap::new()
        }
    }

    pub async fn worker(mut self) {
        info!("Starting listener manager");
        while let Some(message) = self.config_manager.recv().await {
            match message {
                message::ConfigUpdate::ListenerConfig(listener) => {
                    if let Some(sender) = self.listeners.get(&String::from(listener.name.clone())) {
                        debug!("Got listener update for: {:?}", listener.name);
                        let _ = sender.send(message::ConfigUpdate::ListenerConfig(listener)).await;
                    } else {
                        debug!("Got new listener: {:?}", listener.name);
                        let (tx, rx) = mpsc::channel(1);
                        self.listeners.insert(String::from(listener.name.clone()), tx);
                        tokio::spawn(async move {listener::work(listener, rx).await});
                    }
                }
                message::ConfigUpdate::RemoveListener(listener) => {
                    debug!("Got listener remove for: {:?}", listener);
                    if let Some(sender) = self.listeners.remove(&String::from(listener.clone())) {
                        let _ = sender.send(message::ConfigUpdate::RemoveListener(listener)).await;
                    }
                },
                message::ConfigUpdate::TlsConfig(new_tls) => {
                    for listener in self.listeners.values() {
                        let _ = listener.send(message::ConfigUpdate::TlsConfig(new_tls.clone())).await;
                    }
                }
                _ => {}
            }

        }
        panic!("Listener manager has paniced");
    }
}
