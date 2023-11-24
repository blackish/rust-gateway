use log::debug;
use std::collections::VecDeque;
use regex::{RegexBuilder, Regex};
use yaml_rust::Yaml;
use crate::configs::config;
use crate::configs::terms::{common, listener};

// buffer
const DEFAULT_BUFFER: i64 = 1_048_578;

pub struct Listener {
    pub name: Box<str>
}

#[derive(Clone, Debug)]
pub struct ListenerConfig {
    pub name: Box<str>,
    pub listen: Box<str>,
    pub preprocessors: Vec<config::KV>,
    pub buffer: i64,
    pub protocols: Vec<ListenerProtocolConfig>
}

#[derive(Clone, Debug)]
pub enum ListenerProtocolConfig {
    HTTPListener(ListenerHttpProtocolConfig),
    GrpcListener
}

#[derive(Clone, Debug)]
pub struct ListenerHttpProtocolConfig {
    pub name: Box<str>,
    pub sni: Vec<config::Value>,
    pub buffer: i64,
    pub virtual_hosts: Vec<VirtualHostConfig>
}

#[derive(Clone, Debug)]
pub struct VirtualHostConfig {
    pub name: Box<str>,
    pub host_names: Vec<config::Value>,
    pub routes: Vec<RouteConfig>
}

#[derive(Clone, Debug)]
pub struct RouteConfig {
    pub name: Box<str>,
    pub path_matches: Vec<PathMatchConfig>,
    pub actions: VecDeque<ActionConfig>
}

#[derive(Clone, Debug)]
pub struct PathMatchConfig {
    pub name: Box<str>,
    pub action: PathMatchActionConfig
}

#[derive(Clone, Debug)]
pub enum PathMatchActionConfig {
    PathRegex(Vec<Regex>),
    PathPrefix(Vec<Box<str>>),
    Method(Vec<config::NoCaseStr>),
    HeaderMatch(Vec<config::KV>)
}


#[derive(Clone, Debug)]
pub enum ActionConfig {
    Backend(Box<str>),
    None
}

impl ListenerConfig {
    pub fn new(config: &Yaml) -> Option<Self> {
        match config {
            Yaml::Hash(_) => {
                debug!("Loading listener: {:?}", config[common::NAME].as_str()?);
                let mut new_listener = ListenerConfig{
                    name: config[common::NAME].as_str()?.into(),
                    listen: config[listener::LISTEN].as_str()?.into(),
                    preprocessors: Vec::new(),
                    buffer: config[common::BUFFER].as_i64().unwrap_or(DEFAULT_BUFFER),
                    protocols: Vec::new()
                };
                match config[listener::PREPROCESSORS] {
                    Yaml::Array(ref preprocessors) => {
                        for preprocessor in preprocessors {
                            new_listener.preprocessors.push(
                                config::KV{
                                    key: config::Key::String(preprocessor[common::NAME].as_str()?.into()),
                                    value: config::Value::String(preprocessor[common::CONFIG].as_str()?.into())
                                }
                            );
                        };
                    },
                    _ => {}
                };
                if let Yaml::Array(ref protocols) = config[listener::PROTOCOLS] {
                    for protocol in protocols {
                        if protocol[listener::ENGINE].as_str().unwrap_or("") == listener::HTTP {
                            new_listener.protocols.push(
                                ListenerProtocolConfig::HTTPListener(
                                    ListenerHttpProtocolConfig::new(protocol)?
                                )
                            );
                        }
                    }
                };
                debug!("Loading listener: {:?} done", config[common::NAME].as_str()?);
                return Some(new_listener)
            }
            _ => return None
        }
    }
}

impl ListenerHttpProtocolConfig {
    pub fn new(config: &Yaml) -> Option<Self> {
        match config {
            Yaml::Hash(_) => {
                debug!("Loading HTTP protocol: {:?}", config[common::NAME].as_str()?);
                let mut new_listener = Self {
                    name: config[common::NAME].as_str()?.into(),
                    sni: Vec::new(),
                    buffer: config[common::BUFFER].as_i64().unwrap_or(0),
                    virtual_hosts: Vec::new()
                };
                if let Yaml::Array(ref snis) = &config[listener::SNI] {
                    for sni in snis {
                        new_listener.sni.push(
                            config::Value::Regex(
                                RegexBuilder::new(sni.as_str().unwrap())
                                    .case_insensitive(true)
                                    .build()
                                    .unwrap()
                            )
                        );
                    }
                }
                if let Yaml::Array(ref virtual_hosts) = &config[listener::VIRTUAL_HOSTS] {
                    for host in virtual_hosts {
                        new_listener.virtual_hosts.push(VirtualHostConfig::new(host)?);
                    }
                }
                debug!("Loading HTTP protocol: {:?} done", config[common::NAME].as_str()?);
                return Some(new_listener);
            }
            _ => {return None;}
        }
    }
}

impl VirtualHostConfig {
    pub fn new(config: &Yaml) -> Option<Self> {
        match config {
            Yaml::Hash(_) => {
                debug!("Loading virtual host: {:?}", config[common::NAME].as_str()?);
                let mut new_host = VirtualHostConfig {
                    name: config[common::NAME].as_str()?.into(),
                    host_names: Vec::new(),
                    routes: Vec::new()
                };
                if let Yaml::Array(hosts) = &config[listener::HOST_NAMES] {
                    for host in hosts {
                        new_host.host_names.push(
                            config::Value::Regex(
                                RegexBuilder::new(host.as_str().unwrap())
                                    .case_insensitive(true)
                                    .build()
                                    .unwrap()
                            )
                        );
                    }
                };
                if let Yaml::Array(routes) = &config[listener::ROUTES] {
                    for route in routes {
                        new_host.routes.push(RouteConfig::new(&route)?);
                    }
                }
                debug!("Loading virtual host: {:?} done", config[common::NAME].as_str()?);
                return Some(new_host)
            }
            _ => {return None;}
        };
    }
}

impl RouteConfig {
    pub fn new(config: &Yaml) -> Option<Self> {
        match config {
            Yaml::Hash(_) => {
                debug!("Loading route: {:?}", config[common::NAME].as_str()?);
                let mut new_route = Self {
                    name: config[common::NAME].as_str()?.into(),
                    path_matches: Vec::new(),
                    actions: VecDeque::new()
                };
                if let Yaml::Array(ref path_matches) = config[listener::PATH_MATCHES] {
                    debug!("Loading paths");
                    for path_match in path_matches {
                        if let Some(path_name) = path_match[common::NAME].as_str() {
                            if let Yaml::Array(ref path_regex) = path_match[listener::PATH_REGEX] {
                                debug!("Loading path regex: {:?}", path_name);
                                let mut new_path_regex = Vec::new();
                                for regex in path_regex {
                                    new_path_regex.push(Regex::new(regex.as_str().unwrap()).unwrap());
                                }
                                new_route.path_matches.push(
                                    PathMatchConfig{
                                        name: path_name.into(),
                                        action: PathMatchActionConfig::PathRegex(new_path_regex)
                                    }
                                );
                                debug!("Loading path regex: {:?} done", path_name);
                            } else if let Yaml::Array(ref path_prefix) = path_match[listener::PATH_PREFIX] {
                                debug!("Loading path prefix: {:?}", path_name);
                                let mut new_path_prefix = Vec::new();
                                for prefix in path_prefix {
                                    new_path_prefix.push(prefix.as_str().unwrap().into());
                                }
                                new_route.path_matches.push(
                                    PathMatchConfig{
                                        name: path_name.into(),
                                        action: PathMatchActionConfig::PathPrefix(new_path_prefix)
                                    }
                                );
                                debug!("Loading path prefix: {:?} done", path_name);
                            } else if let Yaml::Array(ref headers) = path_match[listener::HEADER] {
                                debug!("Loading headers");
                                let mut new_header = Vec::new();
                                for header in headers {
                                    if let Some(header_name) = header[listener::HEADER_NAME].as_str() {
                                        if let Some(header_value) = header[listener::HEADER_VALUE].as_str() {
                                            new_header.push(
                                                config::KV{
                                                    key: config::Key::String(header_name.into()),
                                                    value: config::Value::String(header_value.into())
                                                }
                                            );
                                        } else if let Some(header_value) = header[listener::HEADER_REGEX].as_str() {
                                            new_header.push(
                                                config::KV{
                                                    key: config::Key::NoCaseString(config::NoCaseStr::new(header_name)),
                                                    value: config::Value::Regex(
                                                        Regex::new(header_value).unwrap()
                                                    )
                                                }
                                            );
                                        }
                                    }
                                }
                                new_route.path_matches.push(
                                    PathMatchConfig{
                                        name: path_name.into(),
                                        action: PathMatchActionConfig::HeaderMatch(new_header)
                                    }
                                );
                                debug!("Loading headers done");
                            }
                        }
                    }
                };
                if let Yaml::Array(ref actions) = config[listener::ACTIONS] {
                    debug!("Loading actions");
                    for action in actions {
                        if let Some(backend) = action[listener::BACKEND].as_str() {
                            new_route.actions.push_back(ActionConfig::Backend(backend.into()));
                        }
                    }
                    debug!("Loading actions done");
                }
                return Some(new_route)
            }
            _ => return None
        };
    }
}
