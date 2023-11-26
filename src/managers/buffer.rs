use log::{info, debug};
use std::collections::HashMap;
use tokio::sync::mpsc;
use crate::configs::{buffer, message};

pub struct BufferManager {
    manager: mpsc::Receiver<message::BufferMessage>,
}

impl BufferManager {
    pub fn new(new_manager: mpsc::Receiver<message::BufferMessage>) -> Self {
        Self {
            manager: new_manager,
        }
    }
    pub async fn worker(&mut self) {
        info!("Starting config manager");
        let mut listeners: HashMap<Box<str>, usize> = HashMap::new();
        let clusters: HashMap<Box<str>, usize> = HashMap::new();
        let mut allocated_listeners: HashMap<Box<str>, usize> = HashMap::new();
        let mut allocated_clusters: HashMap<Box<str>, usize> = HashMap::new();
        loop {
            let res = self.manager.recv().await;
            if let Some(message) = res {
                match message {
                    message::BufferMessage::ConfigUpdate(update) => {
                        if let message::ConfigUpdate::ListenerConfig(new_listener) = update {
                            debug!("Got listener config update: {:?}", new_listener.name);
                            listeners.insert(new_listener.name, new_listener.buffer as usize);
                        };
                    },
                    message::BufferMessage::BufferRequest(request) => {
                        match request.request {
                            message::BufferRequest::RequestListener(name, size) => {
                                debug!("Got buffer request for listener: {:?}", name);
                                let mut new_buffer = message::BufferResponseMessage::OverLimit;
                                let limit = listeners.get(&name).unwrap_or(&0);
                                allocated_listeners.entry(name).and_modify(|entry| {
                                    if *entry + size < *limit {
                                        *entry += size;
                                        new_buffer = message::BufferResponseMessage::Buffer(buffer::StrictBuffer::new(size));
                                    } else if *limit == 0 {
                                        new_buffer = message::BufferResponseMessage::Buffer(buffer::StrictBuffer::new(size));
                                    }
                                }).or_insert_with(|| {
                                    new_buffer = message::BufferResponseMessage::Buffer(buffer::StrictBuffer::new(size));
                                    size
                                });
                                let _ = request.requester.send(new_buffer);
                            },
                            message::BufferRequest::RequestCluster(name, size) => {
                                debug!("Got buffer request for cluster: {:?}", name);
                                let mut new_buffer: message::BufferResponseMessage;
                                let limit = clusters.get(&name).unwrap_or(&0);
                                    new_buffer = message::BufferResponseMessage::Buffer(buffer::StrictBuffer::new(size));
                                allocated_clusters.entry(name).and_modify(|entry| {
                                    if *entry + size < *limit {
                                        *entry += size;
                                    } else if *limit == 0 {
                                        new_buffer = message::BufferResponseMessage::Buffer(buffer::StrictBuffer::new(size));
                                    }
                                }).or_insert_with(|| {
                                    new_buffer = message::BufferResponseMessage::Buffer(buffer::StrictBuffer::new(size));
                                    size
                                });
                                let _ = request.requester.send(new_buffer);
                            },
                            message::BufferRequest::ReleaseListener(name, size) => {
                                debug!("Got buffer release for listener: {:?}", name);
                                allocated_listeners.entry(name).and_modify(|entry| {
                                    if *entry < size {
                                        *entry = 0;
                                    } else {
                                        *entry -= size;
                                    }
                                }).or_insert(0);
                            },
                            message::BufferRequest::ReleaseCluster(name, size) => {
                                debug!("Got buffer release for cluster: {:?}", name);
                                allocated_clusters.entry(name).and_modify(|entry| {
                                    if *entry < size {
                                        *entry = 0;
                                    } else {
                                        *entry -= size;
                                    }
                                }).or_insert(0);
                            }
                        }
                    }
                }
            }

        }
    }
}
