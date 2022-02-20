use async_std::future::timeout;
use async_std::sync::Mutex;
use futures::FutureExt;
use getopts::Options;
use log::LevelFilter;
use simple_logger::SimpleLogger;
use std::collections::HashMap;
use std::time::Duration;
use std::{env};
use std::net::{SocketAddrV4, SocketAddr};
use std::sync::{Arc};
use proxy_protocol::{version2, ProxyHeader};
use async_std::{io, net::{UdpSocket}, task};
use futures::select;
mod utils;
use utils::*;
mod mmproxy;
use mmproxy::*;

fn print_usage(program: &str, opts: Options) {
    let program_path = std::path::PathBuf::from(program);
    let program_name = program_path.file_stem().unwrap().to_str().unwrap();
    let brief = format!("Usage: {} -m MODE [-b BIND_ADDR] -l LOCAL_PORT -h REMOTE_ADDR -r REMOTE_PORT -p",
                        program_name);
    print!("{}", opts.usage(&brief));
}

#[async_std::main]
async fn main() -> io::Result<()>  {
	SimpleLogger::new().with_colors(true).init().unwrap();
	::log::set_max_level(LevelFilter::Info);

    let args: Vec<String> = env::args().collect();
    let program = args[0].clone();

    let mut opts = Options::new();

    opts.reqopt("m",
                "mode",
                "1 : reverse proxy mode , 2 : mmproxy mode",
                "MODE");

    opts.reqopt("l",
                "local-port",
                "The local port to which udppp should bind to",
                "LOCAL_PORT");
    opts.reqopt("r",
                "remote-port",
                "The remote port to which UDP packets should be forwarded",
                "REMOTE_PORT");
    opts.reqopt("h",
                "host",
                "The remote address to which packets will be forwarded",
                "REMOTE_ADDR");
    opts.optopt("b",
                "bind",
                "The address on which to listen for incoming requests",
                "BIND_ADDR");
    opts.optflag("p",
                "proxyprotocol",
                "enable proxy-protocol");
    opts.optflag("s",
                "slient",
                "disable print log");

    let matches = opts.parse(&args[1..])
        .unwrap_or_else(|_| {
                            print_usage(&program, opts);
                            std::process::exit(-1);
                        });
    
    let enable_proxy_protocol = matches.opt_present("p");
    let mode: u32 = matches.opt_str("m").unwrap().parse().unwrap();
    let local_port: u32 = matches.opt_str("l").unwrap().parse().unwrap();
    let remote_port: u32 = matches.opt_str("r").unwrap().parse().unwrap();
    let remote_host = matches.opt_str("h").unwrap();
    let bind_addr = match matches.opt_str("b") {
        Some(addr) => addr,
        None => "127.0.0.1".to_owned(),
    };

    if matches.opt_present("s") {
        ::log::set_max_level(LevelFilter::Off);
    }
    if mode == 1{
        forward(&bind_addr, local_port, &remote_host, remote_port , enable_proxy_protocol).await;
    } else if mode == 2{
        forward_mmproxy(&bind_addr, local_port, &remote_host, remote_port ).await;
    } else {
        log::error!("unknown mode {}!!" , mode);
        std::process::exit(-1);
    }
    

    return Ok(());
}

async fn forward(bind_addr: &str, local_port: u32, remote_host: &str, remote_port: u32 , enable_proxy_protocol : bool) {

    let local_addr = format!("{}:{}", bind_addr, local_port);
    let local_socket = match UdpSocket::bind(&local_addr).await{
        Ok(p) => p,
        Err(_) => {
            log::error!("listen to {} faild!" , local_addr);
            return;
        },
    };

    log::info!("listen to {}" , local_addr);

    if enable_proxy_protocol {
        log::info!("enable proxy-protocol");
    }

    let remote_addr = format!("{}:{}", remote_host, remote_port);

    let mut buf = [0; 64 * 1024];

    let ( c_send , c_recv) = async_std::channel::unbounded::<(SocketAddr, Vec<u8>)>();

    let send_lck = Arc::new(async_std::sync::Mutex::new(c_send));

    let socket_addr_map: Arc<Mutex<HashMap<SocketAddr , (Arc<UdpSocket>, i64)>>> = Arc::new(Mutex::new(HashMap::new()));
    loop{
        select! {
            a = local_socket.recv_from(&mut buf).fuse() => {
                let mut socket_addr_map_lck = socket_addr_map.lock().await;
                let (size, src_addr) = a.unwrap();
                let mut old_stream = false;
                let upstream: Arc<UdpSocket>;

                log::info!("recv from [{}:{}] size : {} " , src_addr.ip().to_string() , src_addr.port() , size);

                if socket_addr_map_lck.contains_key(&src_addr) {
                    upstream = socket_addr_map_lck[&src_addr].0.clone();
                    socket_addr_map_lck.get_mut(&src_addr).unwrap().1 = cur_timestamp();
                    old_stream = true;
                } else {
                    upstream = Arc::new(UdpSocket::bind(bind_addr.to_string() + ":0").await.unwrap());
                    socket_addr_map_lck.insert(src_addr, (upstream.clone(), cur_timestamp()));

                    log::info!("bind new forwarding address [{}:{}] " , upstream.local_addr().unwrap().ip().to_string() , upstream.local_addr().unwrap().port());
                }

                log::info!("send to upstream [{}] size : {} " , remote_addr , size);

                if enable_proxy_protocol {
                    let srcaddr : SocketAddrV4 = src_addr.to_string().as_str().parse().unwrap();
                    let dstaddr : SocketAddrV4 = local_socket.local_addr().unwrap().to_string().as_str().parse().unwrap();
                    let pp_header = ProxyHeader::Version2 {
                        command: version2::ProxyCommand::Proxy,
                        addresses: version2::ProxyAddresses::Ipv4 {
                            source: srcaddr,
                            destination: dstaddr
                        },
                        transport_protocol: version2::ProxyTransportProtocol::Datagram,
                    };
                    let ori_pp_header = proxy_protocol::encode(pp_header).unwrap();
                    let mut pp_buf = ori_pp_header.to_vec();
                    pp_buf.append(&mut buf[..size].to_vec());
        
                    upstream.send_to(pp_buf.as_slice(), &remote_addr).await.unwrap();
                } else {
                    upstream.send_to(&buf[..size].to_vec(), &remote_addr).await.unwrap();
                }
        
                if ! old_stream {
                    let send_lck = send_lck.clone();
                    let socket_addr_map_in_worker_lck = socket_addr_map.clone();
                    task::spawn(async move {
                        let mut buf = [0; 64 * 1024];
                        loop{
                            match timeout(Duration::from_secs(TIMEOUT_SECOND) ,upstream.recv_from(&mut buf)).await{
                                Ok(p) => {
                                    let size = p.unwrap().0;
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