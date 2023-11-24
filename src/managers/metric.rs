use log::debug;
use std::{collections::HashMap, io};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::interval;
use tokio::sync::mpsc::Receiver;
use tokio::sync::Mutex;
use tokio::select;

use crate::configs::{metric, message};

const RATE_TIMER: u16 = 30;

pub struct MetricManager {
    metrics: Mutex<HashMap<metric::MetricSource, HashMap<Box<str>, metric::MetricEntry>>>
}

impl MetricManager {
    pub fn new() -> Self {
        Self {
            metrics: Mutex::new(HashMap::new())
        }
    }

    pub async fn receiver(&mut self, new_receiver: Receiver<message::MetricMessage>) -> io::Result<()> {
        let mut timer = interval(Duration::from_secs(RATE_TIMER.clone().into()));
        let mut receiver = new_receiver;
        loop {
            select! {
                _ = async {
                    timer.tick().await;
                    let mut metrics = self.metrics.lock().await;
                    for instance_value in metrics.values_mut() {
                        for metric in instance_value.values_mut() {
                            if let metric::MetricValue::Rate(ref mut rate) = metric.metric {
                                *rate = (metric.current_value - metric.last_value) / (SystemTime::now().duration_since(metric.timestamp).unwrap().as_secs() as i64);
                                metric.current_value = metric.last_value;
                                metric.last_value = 0;
                                metric.timestamp = SystemTime::now();
                            };
                        }
                    }
                    Ok::<_, io::Error>(())
                } => {},
                _ = async {
                    let message = receiver.recv().await;
                    if let Some(metric_message) = message {
                        debug!("Got metric {:?}", metric_message);
                        let mut metrics = self.metrics.lock().await;
                        for metric_source in metric_message.scope {
                            metrics.entry(metric_source).and_modify(|entry| {
                                entry.entry(metric_message.name.clone()).and_modify(|metric| {
                                    match metric_message.value {
                                        metric::MetricValue::Rate(value) => {
                                            metric.current_value += value;
                                        },
                                        metric::MetricValue::Counter(value) => {
                                            if let metric::MetricValue::Counter(ref mut current_value) = metric.metric {
                                                *current_value += value;
                                                metric.timestamp = SystemTime::now();
                                            }
                                        },
                                        _ => {
                                            metric.metric = metric_message.value.clone();
                                            metric.timestamp = SystemTime::now();
                                        }
                                    }
                                });
                            }).or_insert_with(|| {
                                let mut new_entry = HashMap::new();
                                let mut metric_entry = metric::MetricEntry {
                                    metric: metric_message.value.clone(),
                                    timestamp: SystemTime::now(),
                                    last_value: 0,
                                    current_value: 0
                                };
                                match metric_message.value {
                                    metric::MetricValue::Rate(value) => {
                                        metric_entry.current_value = value;
                                    },
                                    _ => {}
                                }
                                new_entry.insert(metric_message.name.clone(), metric_entry);
                                new_entry
                            });
                        }
                    }
                } => {}
            }
        }
    }
}
