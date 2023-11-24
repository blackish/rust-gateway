pub mod managers;
pub mod configs;
pub mod workers;
pub mod utils;

use clap;
use log;
use simple_logger;
use tokio::sync::mpsc::channel;
use crate::managers::config::ConfigManager;
use crate::managers::listener::ListenerManager;
use crate::managers::metric::MetricManager;
use crate::managers::buffer::BufferManager;
use crate::managers::cluster::ClusterManager;
use crate::managers::common;

#[tokio::main]
async fn main() {
    let app = clap::Command::new("rust-gateway")
        .version("0.0.1")
        .arg(clap::arg!(config: -c --config <config> "config file")
            .required(true))
        .arg(clap::arg!(loglevel: -l --loglevel <LOGLEVEL> "loglevel")
        .value_parser([
                clap::builder::PossibleValue::new("error"),
                clap::builder::PossibleValue::new("warn"),
                clap::builder::PossibleValue::new("info"),
                clap::builder::PossibleValue::new("debug"),

            ]))
        .get_matches();

    match app.get_one::<String>("loglevel").unwrap_or(&String::from("info")).as_str() {
        "error" => {
            simple_logger::init_with_level(log::Level::Error).unwrap();
        },
        "warn" => {
            simple_logger::init_with_level(log::Level::Warn).unwrap();
        },
        "info" => {
            simple_logger::init_with_level(log::Level::Info).unwrap();
        },
        "debug" => {
            simple_logger::init_with_level(log::Level::Debug).unwrap();
        },
        _ => {
            simple_logger::init_with_level(log::Level::Info).unwrap();
        }
    }
    let (tx, rx) = channel(10);
    {
        let mut listener_sender = common::LISTENER.write().await;
        *listener_sender = Some(tx);
    }
    let (cluster_tx, cluster_rx) = channel(10);
    {
        let mut cluster_sender = common::CLUSTER.write().await;
        *cluster_sender = Some(cluster_tx);
    }
    let (buffer_tx, buffer_rx) = channel(10);
    {
        let mut buffer_sender = common::BUFFER.write().await;
        *buffer_sender = Some(buffer_tx);
    }
    let (metric_tx, metric_rx) = channel(10);
    {
        let mut metric_sender = common::METRIC.write().await;
        *metric_sender = Some(metric_tx);
    }
    let (request_tx, request_rx) = channel(10);
    {
        let mut config_sender = common::CONFIG.write().await;
        *config_sender = Some(request_tx);
    }
    let buffer = tokio::spawn(async move {
        BufferManager::new(buffer_rx)
        .worker()
        .await
    });
    let metric = tokio::spawn(async move {
        MetricManager::new()
        .receiver(metric_rx)
        .await
    });
    let listener = tokio::spawn(async move {
        ListenerManager::new(rx)
            .worker()
            .await
    });
    let cluster = tokio::spawn(async move {
        ClusterManager::new(cluster_rx)
            .worker()
            .await
    });
    let config = tokio::spawn(async move {
        ConfigManager::new(request_rx)
            .start(&app.get_one::<String>("config")
                .unwrap()
            )
            .await
            .expect("Failed to load config")
            .worker()
            .await
        });
    let _ = listener.await;
    let _ = config.await;
    let _ = metric.await;
    let _ = buffer.await;
    let _ = cluster.await;
}
