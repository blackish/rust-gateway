use log::debug;
use tokio::io::AsyncWriteExt;
use std::io;
use std::sync:: Arc;
use tokio::sync::mpsc::Receiver;
use tokio::sync::oneshot;
use tokio::sync::Mutex;
use tokio::net::TcpListener;
use tokio::select;
use tokio_rustls::TlsAcceptor;
use rustls;
use crate::configs::{message, listener, config};
use crate::managers::common::CONFIG;
use crate::workers::connections::http;
use crate::utils::utils;
use crate::configs::terms::common;

pub async fn work(new_config: listener::ListenerConfig, new_receiver: Receiver<message::ConfigUpdate>) -> io::Result<()>{
    debug!("Accepting {:?}", new_config.listen);
    let mut use_tls = false;
    let config: Mutex<listener::ListenerConfig> = Mutex::new(new_config);
    let mut update_receiver = new_receiver;
    let config_requester = (CONFIG.read().await.as_ref().unwrap()).clone();
    let tls_acceptor: Mutex<Option<TlsAcceptor>> = Mutex::new(None);
    let socket = TcpListener::bind(String::from(config.lock().await.listen.clone())).await?;
    for preprocessor in &config.lock().await.preprocessors {
        if preprocessor.key == config::Key::String(common::TLS.into()) {
            use_tls = true;
            debug!("TLS in use");
            let mut local_tls_acceptor = tls_acceptor.lock().await;
            if let None = local_tls_acceptor.clone() {
                debug!("No TLS config found");
                if let config::Value::String(tls_config_name) = &preprocessor.value {
                    let (request_tx, request_rx) = oneshot::channel();
                    debug!("Sendng request for TLS config {:?}", tls_config_name);
                    let send_result = config_requester.send(
                        message::ConfigRequest {
                            requester: request_tx,
                            request_type: message::ConfigRequestType::TlsConfig(tls_config_name.clone())
                        }
                    ).await;
                    match send_result {
                        Ok(()) => {
                            debug!("Request sent");
                            let config_update = request_rx.await;
                            match config_update {
                                Ok(update) => {
                                    if let message::ConfigUpdate::TlsConfig(new_tls_config) = update {
                                        debug!("Got tls config response");
                                        *local_tls_acceptor = Some(
                                            TlsAcceptor::from(
                                                Arc::new(
                                                    rustls::ServerConfig::builder()
                                                        .with_safe_defaults()
                                                        .with_no_client_auth()
                                                        .with_single_cert(new_tls_config.certificate_chain, new_tls_config.private_key).unwrap()
                                                )
                                            )
                                        );

                                    }
                                },
                                Err(_) => {}
                            };
                        },
                        Err(_) => {}
                    };
                }
            }
        }
    }
    loop {
        select! {
            _res = async {
                let (mut sock, _) = socket.accept().await?;
                let current_config = config.lock().await.clone();
                if use_tls {
                    let local_acceptor = tls_acceptor.lock().await.clone();
                    if let Some(tls_instance) = local_acceptor {
                        let sock = tls_instance.accept(sock).await?;
                        let (_, connection) = sock.get_ref();
                        'protocols: for protocol_config in &current_config.protocols {
                            if let listener::ListenerProtocolConfig::HTTPListener(http_config) = protocol_config {
                                if let Some(sni) = connection.server_name() {
                                    debug!("SNI: {:?}", sni);
                                    for listener_sni in &http_config.sni {
                                        if utils::value_match(sni, listener_sni) {
                                            let thread_http_config = http_config.clone();
                                            let conn_sni = Some(sni.into());
                                            tokio::spawn(async move{http::process_client(sock, thread_http_config, current_config.name.clone(), conn_sni).await});
                                            // tokio::spawn(async move{http::process_client(sock, thread_http_config, current_config.name.clone(), new_sni).await});
                                            break 'protocols;
                                        }
                                    }
                                    debug!("No connection found for SNI");
                                } else {
                                    debug!("No SNI found");
                                    break;
                                }
                            }
                        }
                    } else {
                        debug!("Tls enabled but no tls config found");
                        sock.shutdown().await?;
                    }
                } else {
                    for protocol_config in &current_config.protocols {
                        if let listener::ListenerProtocolConfig::HTTPListener(http_config) = protocol_config {
                            let thread_http_config = http_config.clone();
                            tokio::spawn(async move{http::process_client(sock, thread_http_config, current_config.name.clone(), None).await});
                            // tokio::spawn(async move{http::process_client(sock, thread_http_config, current_config.name.clone(), None).await});
                            break;
                        }
                    }
                }
                Ok::<_, io::Error>(())
            } => {}
            _res = async {
                if let Some(update) = update_receiver.recv().await {
                    match update {
                        message::ConfigUpdate::ListenerConfig(updated_config) => {
                            let mut new_config = config.lock().await;
                            *new_config = updated_config;
                        },
                        message::ConfigUpdate::RemoveListener(_) => {
                            return Ok(())
                        },
                        message::ConfigUpdate::TlsConfig(updated_tlsconfig) => {
                            debug!("Got tls config {:?}", updated_tlsconfig.name);
                            let mut acceptor = tls_acceptor.lock().await;
                            *acceptor = Some(
                                TlsAcceptor::from(
                                    Arc::new(
                                        rustls::ServerConfig::builder()
                                            .with_safe_defaults()
                                            .with_no_client_auth()
                                            .with_single_cert(updated_tlsconfig.certificate_chain, updated_tlsconfig.private_key).unwrap()
                                    )
                                )
                            );
                        },
                        _ => {}
                    }
                } else {
                    debug!("Receive None update");
               }
                Ok::<_, io::Error>(())
            } => {}
        }
    }
}
