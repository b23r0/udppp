use std::{net::{SocketAddr, SocketAddrV4, IpAddr}, io::{Error, ErrorKind}, time::Duration};
use std::sync::{Arc};
use async_std::{net::UdpSocket, task};
use proxy_protocol::parse;
use udp_sas::UdpSas;
use async_std::sync::Mutex;
use std::collections::HashMap;
use futures::FutureExt;
use futures::select;

use crate::utils::{TIMEOUT_SECOND, cur_timestamp, is_timeout};

fn parse_proxy_protocol(mut buf : &[u8]) -> Result<(SocketAddrV4 , &[u8]), Error> {
    
    let addr = match parse(&mut buf) {
        Ok(p) => { 
            match p{
                proxy_protocol::ProxyHeader::Version1 { addresses } => {
                    match addresses{
                        proxy_protocol::version1::ProxyAddresses::Unknown => Err(Error::new(ErrorKind::Other,"parse faild")),
                        proxy_protocol::version1::ProxyAddresses::Ipv4 { source, destination : _ } => Ok(source),
                        proxy_protocol::version1::ProxyAddresses::Ipv6 { source: _, destination : _ } => Err(Error::new(ErrorKind::Other,"parse faild")),
                    }
                },
                proxy_protocol::ProxyHeader::Version2 { command : _, transport_protocol : _, addresses } => {
                    match addresses{
                        proxy_protocol::version2::ProxyAddresses::Unspec => Err(Error::new(ErrorKind::Other,"parse faild")),
                        proxy_protocol::version2::ProxyAddresses::Ipv4 { source, destination : _ } => Ok(source),
                        proxy_protocol::version2::ProxyAddresses::Ipv6 { source : _, destination : _ } => Err(Error::new(ErrorKind::Other,"parse faild")),
                        proxy_protocol::version2::ProxyAddresses::Unix { source : _, destination : _ } => Err(Error::new(ErrorKind::Other,"parse faild")),
                    }
                },
                _ => Err(Error::new(ErrorKind::Other,"parse faild"))
            }
        },
        Err(_) => Err(Error::new(ErrorKind::Other,"parse faild"))
    };

    let addr = match addr {
        Ok(p) => p,
        Err(e) => return Err(e),
    };

    Ok((addr , buf))
}

pub async fn forward_mmproxy(_: &str, local_port: u32, remote_host: &str, remote_port: u32){
    let remote_addr : SocketAddr = format!("{}:{}", remote_host, remote_port).parse().unwrap();
    let local_addr = format!("0.0.0.0:{}", local_port);
    let local_socket = match UdpSocket::bind(&local_addr).await{
        Ok(p) => p,
        Err(_) => {
            log::error!("listen to {} faild!" , local_addr);
            return;
        },
    };

    log::info!("listen mmproxy to {}" , local_addr);

    let ( c_send , c_recv) = async_std::channel::unbounded::<(SocketAddr, Vec<u8>)>();

    let send_lck = Arc::new(async_std::sync::Mutex::new(c_send));

    let mut buf = [0; 64 * 1024];
    let socket_addr_map: Arc<Mutex<HashMap<SocketAddr , (std::net::UdpSocket, i64)>>> = Arc::new(Mutex::new(HashMap::new()));

    loop{
        select!{
            a = local_socket.recv_from(&mut buf).fuse() => {
                let mut socket_addr_map_lck = socket_addr_map.lock().await;
                let (size, src_addr) = a.unwrap();
                let buf = &buf[..size];
                let mut old_stream = false;
                let upstream: std::net::UdpSocket;
        
                log::info!("recv from [{}:{}] size : {} " , src_addr.ip().to_string() , src_addr.port() , size);

                if let std::collections::hash_map::Entry::Vacant(e) = socket_addr_map_lck.entry(src_addr) {
                    upstream = std::net::UdpSocket::bind_sas("0.0.0.0:0".parse::<SocketAddr>().unwrap()).unwrap();
                    e.insert((upstream.try_clone().unwrap(), cur_timestamp()));
        
                    log::info!("bind new forwarding address [{}:{}] " , upstream.local_addr().unwrap().ip().to_string() , upstream.local_addr().unwrap().port());
                } else {
                    upstream = socket_addr_map_lck[&src_addr].0.try_clone().unwrap();
                    socket_addr_map_lck.get_mut(&src_addr).unwrap().1 = cur_timestamp();
                    old_stream = true;
                }
        
                let (real_addr , buf) = match parse_proxy_protocol(buf){
                    Ok(p) => p,
                    Err(_) => {
                        log::error!("parse protocol proxy faild from : [{}:{}] " , src_addr.ip().to_string() , src_addr.port());
                        continue;
                    },
                };
                
                log::info!("send to upstream [{}] size : {} " , remote_addr , size);

                upstream.send_sas(buf, &remote_addr ,&IpAddr::V4(*real_addr.ip())).unwrap();
        
                if ! old_stream {
                    let send_lck = send_lck.clone();
                    let socket_addr_map_in_worker_lck = socket_addr_map.clone();
                    task::spawn(async move {
                        let mut buf = [0; 64 * 1024];
                        upstream.set_read_timeout(Some(Duration::from_secs(TIMEOUT_SECOND))).unwrap();
                        loop{
                            match upstream.recv_sas(&mut buf){
                                Ok(p) => {
                                    let size = p.0;
                                    log::info!("send downstream to [{}:{}] size : {} " , src_addr.ip().to_string() , src_addr.port() , size);
                                    send_lck.lock().await.send((src_addr , buf[..size].to_vec())).await.unwrap();
                                },
                                Err(_) => {
                                    let mut socket_addr_map = socket_addr_map_in_worker_lck.lock().await;
                                    if is_timeout(socket_addr_map[&src_addr].1, TIMEOUT_SECOND){
                                        log::info!("unbind [{}:{}] for source address: [{}:{}]" , socket_addr_map[&src_addr].0.local_addr().unwrap().ip().to_string() , socket_addr_map[&src_addr].0.local_addr().unwrap().port() , src_addr.ip().to_string() , src_addr.port());
                                        socket_addr_map.remove(&src_addr);
                                        break;
                                    }
                                }
                            };
                        }
                    });
                }
            },
            b = c_recv.recv().fuse() => {
                let (src_addr , data) = b.unwrap();
                local_socket.send_to(data.as_slice() , src_addr).await.unwrap();
            }
        }
    }
}