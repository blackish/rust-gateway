use tokio::sync::{RwLock, mpsc::Sender};
use once_cell::sync::Lazy;

use crate::configs::message::{ConfigRequest, ConfigUpdate, ClusterMessage, BufferMessage, MetricMessage};

pub static CONFIG: Lazy<RwLock<Option<Sender<ConfigRequest>>>> = Lazy::new(|| RwLock::new(None));
pub static METRIC: Lazy<RwLock<Option<Sender<MetricMessage>>>> = Lazy::new(|| RwLock::new(None));
pub static LISTENER: Lazy<RwLock<Option<Sender<ConfigUpdate>>>> = Lazy::new(|| RwLock::new(None));
pub static CLUSTER: Lazy<RwLock<Option<Sender<ClusterMessage>>>> = Lazy::new(|| RwLock::new(None));
pub static BUFFER: Lazy<RwLock<Option<Sender<BufferMessage>>>> = Lazy::new(|| RwLock::new(None));
