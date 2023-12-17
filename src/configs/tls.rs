use log::debug;
use rustls::RootCertStore;
use tokio_rustls::rustls;
use rustls_pki_types;
use webpki_roots;
use std::io::{self, BufReader, Read};
use std::fs::File;
use yaml_rust::Yaml;
use crate::configs::terms::{common, tls};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TlsConfig {
    pub name: Box<str>,
    certificate_chain: Vec::<u8>,
    common_config: CommonTlsConfig,
    client_verify: ClientVerifyConfig
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommonTlsConfig {
    pub suites: Vec<Box<str>>,
    pub kx: Vec<Box<str>>,
    pub protocols: Vec<Box<str>>
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientVerifyConfig {
    pub root_certificates: Vec<u8>,
    pub crls: Vec<u8>
}

impl TlsConfig {
    pub fn new(config: &Yaml) -> io::Result<Self> {
        let mut certs: Vec::<u8> = Vec::new();
        match &config[tls::CERT_FILE] {
            Yaml::String(fln) => {
                File::open(fln)?.read_to_end(&mut certs)?;
            },
            _ => {}
        }
        debug!("Loaded tls config {:?}", config[common::NAME].as_str().unwrap());
        return Ok(
            Self {
                name: config[common::NAME].as_str().unwrap().into(),
                certificate_chain: certs,
                common_config: CommonTlsConfig::new(&config[tls::COMMON_CONFIG]),
                client_verify: ClientVerifyConfig::new(&config[tls::CLIENT_VERIFY])?
            }
        )
    }

    pub fn get_server_config(self) -> Result<rustls::ServerConfig, rustls::Error> {
        let certs: Vec<rustls_pki_types::CertificateDer>;
        let private_key: rustls_pki_types::PrivateKeyDer;
        certs = rustls_pemfile::certs(
            &mut BufReader::new(&self.certificate_chain[..])
            )
            .map(|result| {debug!("{:?}", result); result.unwrap()})
            .collect();

        if let Ok(Some(key)) = rustls_pemfile::private_key(
                &mut BufReader::new(&self.certificate_chain[..])
            ) {
            private_key = key;
        } else {
            return Err(rustls::Error::General("Failed to read key".into()))
        }
        self.client_verify.build_server_config(
            self.common_config
            .build_server_config()?
        )?
        .with_single_cert_with_ocsp(certs, private_key, Vec::new())
    }

    pub fn get_client_config(self) -> Result<rustls::ClientConfig, rustls::Error> {
        if self.certificate_chain.len() > 0 {
            let certs: Vec<rustls_pki_types::CertificateDer>;
            let private_key: rustls_pki_types::PrivateKeyDer;
            certs = rustls_pemfile::certs(
                &mut BufReader::new(&self.certificate_chain[..])
                )
                .map(|result| result.unwrap())
                .collect();

            if let Ok(Some(key)) = rustls_pemfile::private_key(
                    &mut BufReader::new(&self.certificate_chain[..])
                ) {
                private_key = key;
            } else {
                return Err(rustls::Error::General("Failed to read key".into()))
            }
            self.common_config
                .build_client_config()?
                .with_client_auth_cert(certs, private_key)
        } else {
            Ok(self.common_config
                .build_client_config()?
                .with_no_client_auth()
                )
        }
    }
}

pub fn get_suite(name: &str) -> Option<rustls::SupportedCipherSuite> {
    let lower_name = name.to_string().to_lowercase();
    debug!("Current suites {:?}", lower_name);
    rustls::crypto::ring::ALL_CIPHER_SUITES
        .iter()
        .copied()
        .find(|x| {
            debug!("All ciphers {:?}", x.suite().as_str().unwrap().to_lowercase());
            x.suite().as_str().unwrap().to_lowercase() == lower_name
        })
}

pub fn get_protocol(name: &str) -> Option<&'static rustls::SupportedProtocolVersion> {
    let lower_name = name.to_string().to_lowercase();
    debug!("Current protocol {:?}", lower_name);
    rustls::ALL_VERSIONS
        .iter()
        .copied()
        .find(|x| {
            debug!("All protocols {:?}", x.version.as_str().unwrap().to_lowercase());
            x.version.as_str().unwrap().to_lowercase() == lower_name
        })
}

pub fn get_kx(name: &str) -> Option<&'static dyn rustls::crypto::SupportedKxGroup> {
    let lower_name = name.to_string().to_lowercase();
    debug!("Current KXs {:?}", lower_name);
    rustls::crypto::ring::ALL_KX_GROUPS
        .iter()
        .copied()
        .find(|x| {
            debug!("All KXs {:?}", x.name().as_str().unwrap().to_lowercase());
            x.name().as_str().unwrap().to_lowercase() == lower_name
        })
}

impl CommonTlsConfig {
    pub fn new(config: &Yaml) -> Self {
        let mut result = CommonTlsConfig {
            suites: Vec::new(),
            kx: Vec::new(),
            protocols: Vec::new()
        };
        if let Yaml::Array(ref protos) = config[tls::PROTOCOL_LIST] {
            result.protocols = protos
                            .iter()
                            .map(|x| {
                                x.as_str().unwrap().into()
                            })
                            .collect()
        };
        if let Yaml::Array(ref kxs) = config[tls::KX_LIST] {
            result.kx = kxs
                            .iter()
                            .map(|x| {
                                x.as_str().unwrap().into()
                            })
                            .collect()
        };
        if let Yaml::Array(ref suites) = config[tls::CIPHER_LIST] {
            result.suites = suites
                            .iter()
                            .map(|x| {
                                x.as_str().unwrap().into()
                            })
                            .collect()
        }
        result
    }
    pub fn build_server_config(self) -> Result<rustls::ConfigBuilder<rustls::ServerConfig, rustls::WantsVerifier>,rustls::Error> {
        let suites = {
            if self.suites.len() > 0 {
                self.suites
                    .iter()
                    .filter_map(|suite| {
                        get_suite(suite)
                    })
                    .collect::<Vec<rustls::SupportedCipherSuite>>()
            } else {
                rustls::crypto::ring::DEFAULT_CIPHER_SUITES.to_vec()
            }
        };
        let kxs = {
            if self.kx.len() > 0 {
                self.kx
                .iter()
                .filter_map(|kx| {
                        get_kx(kx)
                    })
                .collect::<Vec<&dyn rustls::crypto::SupportedKxGroup>>()
            } else {
                rustls::crypto::ring::ALL_KX_GROUPS.to_vec()
            }
        };
        let protos = {
            if self.protocols.len() > 0 {
                self.protocols
                .iter()
                .map(|proto| {
                        get_protocol(proto).unwrap()
                    })
                .collect::<Vec<&rustls::SupportedProtocolVersion>>()
            } else {
                rustls::DEFAULT_VERSIONS.to_vec()
            }
        };
        rustls::ServerConfig::builder_with_provider(
            rustls::crypto::CryptoProvider {
                cipher_suites: suites,
                kx_groups: kxs,
                ..rustls::crypto::ring::default_provider()
            }
            .into()
        )
        .with_protocol_versions(&protos)
    }

    pub fn build_client_config(self) -> Result<rustls::ConfigBuilder<rustls::ClientConfig, rustls::client::WantsClientCert>,rustls::Error> {
        let suites = {
            if self.suites.len() > 0 {
                self.suites
                    .iter()
                    .map(|suite| {
                        get_suite(suite).unwrap()
                    })
                    .collect::<Vec<rustls::SupportedCipherSuite>>()
            } else {
                rustls::crypto::ring::DEFAULT_CIPHER_SUITES.to_vec()
            }
        };
        let kxs = {
            if self.kx.len() > 0 {
                self.kx
                .iter()
                .map(|kx| {
                        get_kx(kx).unwrap()
                    })
                .collect::<Vec<&dyn rustls::crypto::SupportedKxGroup>>()
            } else {
                rustls::crypto::ring::ALL_KX_GROUPS.to_vec()
            }
        };
        let protos = {
            if self.protocols.len() > 0 {
                self.protocols
                .iter()
                .map(|proto| {
                        get_protocol(proto).unwrap()
                    })
                .collect::<Vec<&rustls::SupportedProtocolVersion>>()
            } else {
                rustls::DEFAULT_VERSIONS.to_vec()
            }
        };
        let mut roots = RootCertStore::empty();
        roots.extend(
            webpki_roots::TLS_SERVER_ROOTS
                .iter()
                .cloned()
        );
        let result = rustls::ClientConfig::builder_with_provider(
            rustls::crypto::CryptoProvider {
                cipher_suites: suites,
                kx_groups: kxs,
                ..rustls::crypto::ring::default_provider()
            }
            .into()
        )
        .with_protocol_versions(&protos)?
        .with_root_certificates(roots);
        return Ok(result);
    }

}

impl ClientVerifyConfig {
    pub fn new(config: &Yaml) -> io::Result<Self> {
        debug!("Building client verify tls config");
        let mut result = Self {
            root_certificates: Vec::new(),
            crls: Vec::new()
        };
        match config {
            Yaml::Hash(_client_config) => {
                let _ = File::open(config[tls::CA_LIST].as_str().unwrap())?.read_to_end(&mut result.root_certificates)?;
                let _ = File::open(config[tls::CRL_LIST].as_str().unwrap())?.read_to_end(&mut result.crls)?;
            },
            _ => {}
        }
        debug!("Building client verify tls config completed");
        Ok(result)
    }
    pub fn build_server_config(self, config_builder: rustls::ConfigBuilder<rustls::ServerConfig, rustls::WantsVerifier>) -> Result<rustls::ConfigBuilder<rustls::ServerConfig, rustls::server::WantsServerCert>, rustls::Error> {
        debug!("Building client verifier");
        if self.root_certificates.len() == 0 {
            return Ok(config_builder.with_client_cert_verifier(rustls::server::WebPkiClientVerifier::no_client_auth()));
        }
        let mut ca_certs = rustls::RootCertStore::empty();
        debug!("Building client verifier CA certs");
        for cert in rustls_pemfile::certs(
            &mut BufReader::new(&self.root_certificates[..])
            )
            .map(|result| result.unwrap()) {
                let _ = ca_certs.add(cert);
            }
        debug!("Building client verifier CA finished {:?}", ca_certs);
        debug!("Building client verifier CRLs");
        let crls: Vec<rustls_pki_types::CertificateRevocationListDer<'_>> = rustls_pemfile::crls(
            &mut BufReader::new(&self.crls[..])
            )
            .filter_map(|result| {
                if let Ok(result_crl) = result {
                    Some(result_crl)
                } else {
                    None
                }
            })
            .collect();
        debug!("Building client verifier CRLs finished {:?}", crls);
        if let Ok(verifier) = rustls::server::WebPkiClientVerifier::builder(ca_certs.into()).with_crls(crls).build() {
            debug!("Building client verifier successfull");
            return Ok(config_builder.with_client_cert_verifier(verifier))
        } else {
            debug!("Failed to build verifier");
            return Err(rustls::Error::NoCertificatesPresented);
        }
    }
}
