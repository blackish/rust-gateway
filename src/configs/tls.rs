use log::debug;
use rustls;
use std::io::{self, BufReader};
use std::fs::File;
use rustls_pemfile::{Item, read_all};
use yaml_rust::Yaml;

#[derive(Clone, Debug)]
pub struct TlsConfig {
    pub name: Box<str>,
    pub certificate_chain: Vec::<rustls::Certificate>,
    pub private_key: rustls::PrivateKey
}

impl TlsConfig {
    pub fn new(config: &Yaml) -> io::Result<Self> {
        let mut certs: Vec::<rustls::Certificate> = Vec::new();
        let mut key: Option<rustls::PrivateKey> = None;
        for item in read_all(&mut BufReader::new(File::open(config["file"].as_str().unwrap()).unwrap()))? {
            match item {
                Item::X509Certificate(cert) => {certs.push(rustls::Certificate(cert));},
                Item::PKCS8Key(new_key) => {key = Some(rustls::PrivateKey(new_key));},
                _ => {},
            }
        }
        if let Some(priv_key) = key {
            if certs.len() > 0 {
                debug!("Loaded tls config {:?}", config["name"].as_str().unwrap());
                return Ok(
                    Self {
                        name: config["name"].as_str().unwrap().into(),
                        certificate_chain: certs,
                        private_key: priv_key
                    }
                )
            }
        }
        Err(io::Error::new(io::ErrorKind::NotFound, "Cert not found"))
    }
}
