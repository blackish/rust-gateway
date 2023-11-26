use log::debug;
use std::collections::HashMap;
use std::io;
use std::ops::Deref;
use tokio::select;
use tokio::sync::oneshot;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc::Sender;
use bytes::BytesMut;
use crate::configs::{listener, config, metric, terms, message, buffer};
use crate::managers::common::{BUFFER, METRIC, CLUSTER};
use crate::utils::{http, utils};

const CONN_BUFFER: usize = 8192;
const ROUTE_BUFFER: usize = 1_024_000;
const HTTP_PROTO: &str = "HTTP";
const HTTP_VERSIONS: [&str; 2] = ["1.0", "1.1"];

#[derive(Debug)]
pub struct HttpConnection {
    pub headers: HashMap<config::NoCaseStr, Box<str>>,
    pub response_code: Option<u16>,
    pub sni: Option<Box<str>>,
    pub uri: Option<Box<str>>,
    pub method: Option<config::NoCaseStr>,
    pub protocol: Option<Box<str>>,
    pub protocol_version: Option<Box<str>>,
    pub scope: Vec<metric::MetricSource>,
    metric_sender: Sender<message::MetricMessage>,
    pub retries: u8,
    pub sent: usize,
    pub the_rest: Vec<u8>,
    pub received: usize
}

impl HttpConnection {
    pub async fn new(new_scope: Vec<metric::MetricSource>) -> Self {
        Self{
            headers: HashMap::new(),
            response_code: None,
            sni: None,
            uri: None,
            method: None,
            protocol: None,
            protocol_version: None,
            scope: new_scope,
            metric_sender: METRIC.read().await.as_ref().unwrap().clone(),
            retries: 0,
            sent: 0,
            the_rest: Vec::new(),
            received: 0
        }
    }

    pub async fn send_metrics(&self) {
        let _ = self.metric_sender.send(message::MetricMessage {
            scope: self.scope.clone(),
            name: terms::metric::BYTES_RECEIVED.into(),
            value: metric::MetricValue::Rate(self.received as i64)
        }).await;
        let _ = self.metric_sender.send(message::MetricMessage {
            scope: self.scope.clone(),
            name: terms::metric::BYTES_SENT.into(),
            value: metric::MetricValue::Rate(self.sent as i64)
        }).await;
        let _ = self.metric_sender.send(message::MetricMessage {
            scope: self.scope.clone(),
            name: terms::metric::CONNECTIONS.into(),
            value: metric::MetricValue::Rate(1)
        }).await;
    }
}

pub async fn process_cluster<T: AsyncReadExt + AsyncWriteExt + Send + Unpin>(
    mut connection: T,
    _config: listener::RouteConfig,
    cluster: Box<str>,
    clustermember: Box<str>,
    listener: oneshot::Sender<message::ListenerConnection>,
    mut client_reader: buffer::StrictBufferReader
) -> io::Result<()> {
    let mut http_connection: HttpConnection;
    let buffer_requester = BUFFER.read().await.as_ref().unwrap().clone();
    let client_writer: buffer::StrictBufferWriter;
    let (buffer_tx, buffer_rx) = oneshot::channel();
    let mut listener_buffer = BytesMut::zeroed(CONN_BUFFER);
    let mut sent_headers = false;
    let _ = buffer_requester.send(
        message::BufferMessage::BufferRequest(
            message::BufferRequestMessage {
                request: message::BufferRequest::RequestCluster(cluster.clone(), ROUTE_BUFFER),
                requester: buffer_tx
            }
        )
    ).await.unwrap();
    let receive_buffer = buffer_rx.await;
    match receive_buffer.unwrap() {
        message::BufferResponseMessage::Buffer((buffer_writer, buffer_reader)) => {
            client_writer = buffer_writer;
            debug!("Got buffer response for cluster");
            http_connection = HttpConnection::new(
                vec![
                    metric::MetricSource::Cluster(cluster),
                    metric::MetricSource::ClusterMember(clustermember),
                ]
            ).await;
            let _ = listener.send(message::ListenerConnection::ListenerBuffer(buffer_reader));
        },
        message::BufferResponseMessage::OverLimit => {
            let _ = listener.send(message::ListenerConnection::BufferOverLimit);
            return Ok(())
        }
    }
    while !sent_headers {
        select! {
            res = client_reader.read(&mut listener_buffer) => {
                if let Ok(read_len) = res {
                    if read_len > 0 {
                        debug!("Sending to backend: {:?}", std::str::from_utf8(&listener_buffer[..read_len]));
                        let _ = &connection.write(&listener_buffer[..read_len]).await?;
                    }
                }
            },
            _res = read_headers(&mut http_connection, &mut connection, false) => {
                sent_headers = true;
            }
        }
    }
    let _ = process_cluster_request(
        &mut connection,
        &mut http_connection,
        client_reader,
        client_writer
    ).await?;
    Ok(())
}

pub async fn process_client<T: AsyncReadExt + AsyncWriteExt + Send + Unpin>(
    mut connection: T,
    config: listener::ListenerHttpProtocolConfig,
    listener: Box<str>,
    new_sni: Option<Box<str>>
) -> io::Result<()> {
    let result_action: Option<listener::ActionConfig>;
    let result_route: Option<listener::RouteConfig>;
    let mut buffer_size = config.buffer.clone();
    if buffer_size == 0 {
        buffer_size = CONN_BUFFER as i64;
    }
    let buffer_requester = BUFFER.read().await.as_ref().unwrap().clone();
    let mut http_connection = HttpConnection::new(
        vec![
            metric::MetricSource::Listener(listener.clone()),
            metric::MetricSource::ListenerProtocol(terms::listener::HTTP.into())
        ]
    ).await;
    http_connection.sni = new_sni;
    read_headers(&mut http_connection, &mut connection, true).await?;
    (result_action, result_route) = route(&http_connection, config);
    if let Some(target) = result_action {
        let cluster_manager = CLUSTER.read().await.as_ref().unwrap().clone();
        match target {
            listener::ActionConfig::Backend(backend) => {
                debug!("Route request to {:?}", backend);
                let (buffer_tx, buffer_rx) = oneshot::channel();
                let _ = buffer_requester.send(
                    message::BufferMessage::BufferRequest(
                        message::BufferRequestMessage {
                            request: message::BufferRequest::RequestListener(listener.clone(), buffer_size as usize),
                            requester: buffer_tx
                        }
                    )
                ).await.unwrap();
                if let Ok(buffer_message) = buffer_rx.await {
                    match buffer_message {
                        message::BufferResponseMessage::Buffer((buffer_writer, buffer_reader)) => {
                            debug!("Got buffer response");
                            let (buffer_tx, buffer_rx) = oneshot::channel();
                            let _ = cluster_manager.send(message::ClusterMessage::ClusterConnection(backend, result_route.unwrap(), buffer_reader, buffer_tx)).await;
                            if let Ok(cluster_message) = buffer_rx.await {
                                match cluster_message {
                                    message::ListenerConnection::ListenerBuffer(buffer) => {
                                        debug!("Listener: got cluster handle");
                                        let _ = process_client_request(connection, &mut http_connection, buffer, buffer_writer).await?;
                                    },
                                    message::ListenerConnection::ClusterNotFound => {
                                        fail_and_close(&mut connection, "404".into(), "Cluster not found".into()).await?;
                                        http_connection.sent += 50 + 20;
                                        http_connection.send_metrics().await;
                                    },
                                    message::ListenerConnection::NoAvailableMember => {
                                        fail_and_close(&mut connection, "503".into(), "No available backends".into()).await?;
                                        http_connection.sent += 50 + 24;
                                        http_connection.send_metrics().await;
                                    },
                                    message::ListenerConnection::BufferOverLimit => {
                                        fail_and_close(&mut connection, "503".into(), "Out of memory".into()).await?;
                                        http_connection.sent += 50 + 16;
                                        http_connection.send_metrics().await;
                                    }
                                }

                            }
                        },
                        message::BufferResponseMessage::OverLimit => {
                            debug!("Got buffer over limit");
                            fail_and_close(&mut connection, "503".into(), "Out of memory".into()).await?;
                            http_connection.sent += 50 + 16;
                            http_connection.send_metrics().await;
                        }
                    }
                } else {
                    debug!("Got unexpected response from buffer manager");
                    http_connection.sent += 50 + 16;
                    fail_and_close(&mut connection, "503".into(), "Out of memory".into()).await?;
                }
                return Ok(())
            },
            _ => {}
        }
    } else {
        debug!("Route not found");
        http_connection.sent += 50 + 18;
        fail_and_close(&mut connection, "404".into(), "Route not found".into()).await?;
    }
    Ok(())
}

async fn process_cluster_request<T: AsyncReadExt + AsyncWriteExt + Send + Unpin>(
    mut connection: T,
    http_connection: &mut HttpConnection,
    mut read_buffer: buffer::StrictBufferReader,
    mut write_buffer: buffer::StrictBufferWriter
) -> io::Result<()> {
    let mut listener_buffer = BytesMut::zeroed(CONN_BUFFER);
    let mut cluster_buffer = BytesMut::zeroed(CONN_BUFFER);
    http_connection.sent += write_buffer.write(http_connection.protocol.as_ref().unwrap().as_bytes()).await?;
    http_connection.sent += write_buffer.write(b"\r\n").await?;
    for (k, v) in &http_connection.headers {
        http_connection.sent += write_buffer.write(k.inner_value().as_bytes()).await?;
        http_connection.sent += write_buffer.write(b": ").await?;
        http_connection.sent += write_buffer.write(v.as_bytes()).await?;
        http_connection.sent += write_buffer.write(b"\r\n").await?;
    }
    http_connection.sent += write_buffer.write(b"\r\n").await?;
    http_connection.sent += write_buffer.write(&http_connection.the_rest).await?;
    loop {
        select! {
            result = connection.read(&mut listener_buffer[..]) => {
                debug!("Got response from backend: {:?}", result);
                if let Ok(result_len) = result {
                    if result_len > 0 {
                        http_connection.received += write_buffer.write(&listener_buffer[..result_len]).await?;
                    } else {
                        let _ = connection.shutdown().await;
                        http_connection.send_metrics().await;
                        return Ok(())
                    }
                }
            },
            result = read_buffer.read(&mut cluster_buffer[..]) => {
                if let Ok(result_len) = result {
                    if result_len > 0 {
                        http_connection.sent += connection.write(&cluster_buffer[..result_len]).await?;
                    } else {
                        let _ = connection.shutdown().await;
                        http_connection.send_metrics().await;
                        return Ok(())
                    }
                }
            }
        }
    }
}

async fn process_client_request<T: AsyncReadExt + AsyncWriteExt + Send + Unpin>(
    mut connection: T,
    http_connection: &mut HttpConnection,
    mut read_buffer: buffer::StrictBufferReader,
    mut write_buffer: buffer::StrictBufferWriter
) -> io::Result<()> {
    let mut listener_buffer = BytesMut::zeroed(CONN_BUFFER);
    let mut cluster_buffer = BytesMut::zeroed(CONN_BUFFER);
    debug!("Writing headers");
    http_connection.sent += write_buffer.write(http_connection.protocol.as_ref().unwrap().as_bytes()).await?;
    http_connection.sent += write_buffer.write(b"\r\n").await?;
    for (k, v) in &http_connection.headers {
        http_connection.sent += write_buffer.write(k.inner_value().as_bytes()).await?;
        http_connection.sent += write_buffer.write(b": ").await?;
        http_connection.sent += write_buffer.write(v.as_bytes()).await?;
        http_connection.sent += write_buffer.write(b"\r\n").await?;
    }
    http_connection.sent += write_buffer.write(b"\r\n").await?;
    http_connection.sent += write_buffer.write(&http_connection.the_rest).await?;
    debug!("Finished writing headers");
    loop {
        select! {
            result = connection.read(&mut listener_buffer[..]) => {
                if let Ok(result_len) = result {
                    if result_len > 0 {
                        http_connection.received += write_buffer.write(&listener_buffer[..result_len]).await?;
                    } else {
                        let _ = write_buffer.shutdown().await?;
                        let _ = connection.shutdown().await?;
                        http_connection.send_metrics().await;
                        return Ok(())
                    }
                }
            },
            result = read_buffer.read(&mut cluster_buffer[..]) => {
                debug!("Got result from backend buffer {:?}", result);
                if let Ok(result_len) = result {
                    if result_len > 0 {
                        http_connection.sent += connection.write(&cluster_buffer[..result_len]).await?;
                    } else {
                        let _ = connection.shutdown().await?;
                        http_connection.send_metrics().await;
                        return Ok(())
                    }
                }
            }
        }
    }
}

fn route(http_connection: &HttpConnection, config: listener::ListenerHttpProtocolConfig) -> (Option<listener::ActionConfig>, Option<listener::RouteConfig>) {
    for v_host in config.virtual_hosts {
        if match_vhost(&http_connection, &v_host) {
            for mut route in v_host.routes {
                if match_route(&http_connection, &route) {
                    let action = route.actions.pop_front();
                    return (action, Some(route));
                }
            }
        }
    }
    (None, None)
}

fn match_vhost(http_connection: &HttpConnection, config: &listener::VirtualHostConfig) -> bool {
    let host_name = http_connection.headers.get(&config::NoCaseStr::new(terms::http::HOST)).unwrap();
    for host in &config.host_names {
        if utils::value_match(host_name, &host) {
            return true;
        }
    }
    return false;
}

fn match_route(http_connection: &HttpConnection, config: &listener::RouteConfig) -> bool {
    'route: for single_match in &config.path_matches {
        match &single_match.action {
            listener::PathMatchActionConfig::Method(method) => {
                if !method.contains(http_connection.method.as_ref().unwrap()) {
                    return false;
                }
            },
            listener::PathMatchActionConfig::PathPrefix(prefix) => {
                for single_prefix in prefix {
                    if http_connection.uri.as_ref().unwrap().starts_with(single_prefix.deref()) {
                        continue 'route;
                    }
                }
            },
            listener::PathMatchActionConfig::PathRegex(prefix) => {
                for single_prefix in prefix {
                    if single_prefix.is_match(http_connection.uri.as_ref().unwrap()) {
                        continue 'route;
                    }
                }
            },
            listener::PathMatchActionConfig::HeaderMatch(headers) => {
                for kv in headers {
                    if let config::Key::NoCaseString(ref header_name) = kv.key {
                        if let Some(val) = http_connection.headers.get(header_name) {
                            if !utils::value_match(val, &kv.value) {
                                return false;
                            }
                        } else {
                            return false;
                        }
                    }
                }
            }
        }
    }
    return true;
}

async fn read_headers<T: AsyncReadExt + AsyncWriteExt + Unpin + Send>(
    http_connection: &mut HttpConnection,
    connection: &mut T,
    request: bool
) -> io::Result<()> {
    let mut read_buffer = BytesMut::zeroed(CONN_BUFFER);
    let mut pos: usize = 0;
    let mut read_buf: usize = 0;
    let mut new_string: String;
    (new_string, pos, read_buf) = read_line(connection, &mut read_buffer, pos, read_buf).await?;
    http_connection.received += new_string[..].as_bytes().len() + read_buf - pos;
    let head: Vec<&str> = new_string.trim().split(" ").collect();
    if (head.len() != 3 && request) || (head.len() <2 && !request) {
        connection.shutdown().await?;
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid request"))
    }
    if request {
        http_connection.method = Some(config::NoCaseStr::new(head[0]));
        http_connection.uri = Some(http::normalized(String::from(head[1])).into());
        let protocol_and_version: Vec::<&str> = head[2].split("/").collect();
        if protocol_and_version.len() != 2 {
            connection.shutdown().await?;
            http_connection.send_metrics().await;
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid request"))
        }
        if protocol_and_version[0] != HTTP_PROTO || !HTTP_VERSIONS.contains(&protocol_and_version[1].into()) {
            connection.shutdown().await?;
            http_connection.send_metrics().await;
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid request"))
        }
        http_connection.protocol = Some(new_string.trim().into());
        http_connection.protocol_version = Some(protocol_and_version[1].into());
    } else {
        let protocol_and_version: Vec::<&str> = head[0].split("/").collect();
        if protocol_and_version[0] != HTTP_PROTO {
            connection.shutdown().await?;
            http_connection.send_metrics().await;
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid request"))
        }
        http_connection.protocol = Some(new_string.clone().into());
        http_connection.protocol_version = Some(protocol_and_version[1].into());
        http_connection.response_code = Some(head[1].parse().unwrap());

    }
    while new_string.trim().len() > 0 {
        (new_string, pos, read_buf) = read_line(connection, &mut read_buffer, pos, read_buf).await?;
        http_connection.received += new_string[..].as_bytes().len() + read_buf - pos;
        let header: Vec<&str> = new_string.trim().split(": ").collect();
        if header.len() ==2 {
            http_connection.headers.insert(
                config::NoCaseStr::new(header[0]),
                header[1].into());
        }
    }
    if pos < read_buf {
        http_connection.the_rest = Vec::from(&read_buffer[pos..read_buf]);
    }
    Ok(())
}

async fn read_line<T: AsyncReadExt + AsyncWriteExt + Unpin>(client: &mut T, buf: &mut BytesMut, old_pos: usize, old_read_buf: usize) -> io::Result<(String, usize, usize)> {
    let mut new_string = String::new();
    let mut new_char: u32 = 0;
    let mut new_pos = old_pos;
    let mut read_result = old_read_buf;
    loop {
        for pos in new_pos..read_result {
            if buf[pos] == 10 {
                return Ok((new_string, pos+1, read_result))
            }
            if buf[pos] < 128 {
                new_char = 0;
                new_string.push(char::from_u32(buf[pos] as u32).unwrap());
            } else {
                new_char = (new_char << 8) + buf[pos] as u32;
                if new_char & 0b1100000000000000 == 0b1100000000000000 {
                    new_char = 0;
                    if let Some(decoded_char) = char::from_u32(new_char) {
                        new_string.push(decoded_char);
                    }
                } else if new_char & 0b111000000000000000000000 == 0b111000000000000000000000 {
                    new_char = 0;
                    if let Some(decoded_char) = char::from_u32(new_char) {
                        new_string.push(decoded_char);
                    }
                } else if new_char & 0b11110000000000000000000000000000 == 0b11110000000000000000000000000000 {
                    new_char = 0;
                    if let Some(decoded_char) = char::from_u32(new_char) {
                        new_string.push(decoded_char);
                    }
                }
            }
        }
        read_result = client.read(&mut buf[..]).await?;
        new_pos = 0;
        if read_result == 0 {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "Connection closed"))
        }
    }
}

//FIXME: add custom err codes and messages
async fn fail_and_close<T: AsyncReadExt + AsyncWriteExt + Send + Unpin>(http_connection: &mut T, err: Box<str>, msg: Box<str>) -> io::Result<()> {
    debug!("Route not found");
    http_connection
            .write_all(&b"HTTP/1.0 "[..]).await?;
    http_connection
            .write_all(err.as_bytes()).await?;
    http_connection
            .write_all(&b"\r\n
            Connection: close\r\n
            Content-length: "[..]).await?;
    http_connection
            .write_all(msg.len().to_string().as_bytes()).await?;
    http_connection
            .write_all(&b"\r\n\r\n"[..]).await?;
    http_connection
            .write_all(msg.as_bytes()).await?;
    http_connection.shutdown().await?;
    // http_connection.send_metrics().await;
    Ok(())
}
