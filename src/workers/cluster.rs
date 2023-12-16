use log::debug;
use std::io;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::mpsc::{Sender, Receiver, channel};
use crate::configs::{cluster, message};
use crate::workers::clustermember;

pub async fn work(
    new_config: cluster::ClusterConfig,
    new_receiver: Receiver<message::ClusterMessage>
) -> io::Result<()>{
    debug!("Starting cluster {:?}", new_config.name);
    let statuses: Arc<RwLock<HashMap<Box<str>, cluster::ClusterMemberStatus>>> = Arc::new(RwLock::new(HashMap::new()));
    let mut members: HashMap::<Box<str>, Sender<message::ClusterMessage>> = HashMap::new();
    let mut member_list: Vec<Box<str>> = Vec::new();
    let mut config_receiver = new_receiver;
    let index: usize = 0;
    for member in &new_config.members {
        member_list.push(member.address.to_string().into());
        add_member(statuses.clone(), &mut members, &new_config, &member).await;
    }
    let mut config:cluster::ClusterConfig = new_config;
    loop {
        let res = config_receiver.recv().await;
        if let Some(message) = res {
            match message {
                message::ClusterMessage::ConfigUpdate(ref update) => {
                    match update {
                        message::ConfigUpdate::ClusterConfig(new_config) => {
                            if new_config.keepalive != config.keepalive {
                                for member in members.values() {
                                    let _ = member.send(message::ClusterMessage::ConfigUpdate(update.clone())).await;
                                }
                            }
                            config = new_config.clone();
                            member_list.drain(..);
                            for member in config.members.clone() {
                                member_list.push(member.address.to_string().into());
                                if !members.contains_key(&member.address.to_string().into_boxed_str()) {
                                    add_member(statuses.clone(), &mut members, &config, &member).await;
                                }
                            }
                            for member in members.clone().keys() {
                                if !member_list.contains(member) {
                                    let _ = members
                                                .remove(member)
                                                .unwrap()
                                                .send(
                                                    message::ClusterMessage::ConfigUpdate(
                                                        message::ConfigUpdate::RemoveCluster(config.name.clone())
                                                    )
                                                )
                                                .await;
                                }
                            }
                        },
                        _ => {}
                    }
                },
                message::ClusterMessage::ClusterConnection(_,_,_,_,_) => {
                    let mut member_selection: Vec<Box<str>>;
                    let active_member: Option<Sender<message::ClusterMessage>>;
                    debug!("Got client request");
                    {
                        let member_statuses = statuses.read().await;
                        member_selection = member_list.clone();
                        member_selection.retain(
                            |x| member_statuses[x] != cluster::ClusterMemberStatus::Unavailable
                        );
                    }
                    match config.lb_method {
                        cluster::LbMethod::LeastConn => {
                            active_member = get_least_conn_member(&member_selection, statuses.clone(), &members).await;
                        },
                        cluster::LbMethod::RoundRobin => {
                            active_member = get_round_robin_member(index, &member_selection, &members).await;
                        }
                    }
                    if let Some(active_member_sender) = active_member {
                        let _ = active_member_sender.send(message).await;
                    }
                },
                _ => {}
            }
        }
    }
}

async fn get_round_robin_member(
    mut index: usize,
    member_list: &Vec<Box<str>>,
    members: &HashMap<Box<str>, Sender<message::ClusterMessage>>
) -> Option<Sender<message::ClusterMessage>> {
    if member_list.len() == 0 {
        return None;
    };
    index += 1;
    if index >= member_list.len() {
        index = 0;
    };
    return members.get(&member_list[index]).cloned();
}

async fn get_least_conn_member(
    member_list: &Vec<Box<str>>,
    statuses: Arc<RwLock<HashMap<Box<str>, cluster::ClusterMemberStatus>>>,
    members: &HashMap<Box<str>, Sender<message::ClusterMessage>>
) -> Option<Sender<message::ClusterMessage>> {
    let mut result: Option<&Sender<message::ClusterMessage>>;
    let mut result_conns: u16;
    let local_statuses = statuses.read().await;
    if let cluster::ClusterMemberStatus::Active(conns) = local_statuses.get(&member_list[0]).unwrap() {
        result_conns = *conns;
        result = members.get(&member_list[0]);
    } else {
        result_conns = 65535;
        result = None;
    }
    for index in &member_list[1..] {
        if let cluster::ClusterMemberStatus::Active(conns) = local_statuses.get(index).unwrap() {
            if *conns > result_conns {
                result = members.get(index);
                result_conns = *conns;
            }
        }
    }
    return result.cloned();
}

async fn add_member(
    status_list: Arc<RwLock<HashMap<Box<str>, cluster::ClusterMemberStatus>>>,
    member_list: &mut HashMap::<Box<str>, Sender<message::ClusterMessage>>,
    cluster_config: &cluster::ClusterConfig,
    member: &cluster::ClusterMemberConfig
) {
    debug!("Cluster: {:?}: adding member {:?}", cluster_config.name, member.address);
    let new_member = clustermember::Member::new(
        cluster_config.name.clone(),
        member.address.clone(),
        cluster_config.keepalive.clone(),
        cluster_config.tls.clone(),
    );
    let (tx, rx) = channel(1);
    let _ = member_list.insert(member.address.to_string().into(), tx);
    status_list
        .write()
        .await
        .insert(
            member
                .address
                .to_string()
                .as_str()
                .into(),
            member.status.clone()
        );
    let new_statuses = status_list.clone();
    tokio::spawn(async move {
        clustermember::run_member(new_member, new_statuses, rx).await
    });
}
