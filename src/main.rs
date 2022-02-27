use getopts::Options;
use log::{LevelFilter};
use net2::{UdpBuilder};
use net2::unix::{UnixUdpBuilderExt};
use simple_logger::SimpleLogger;
use tokio::net::UdpSocket;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::Mutex;
use tokio::sync::mpsc::unbounded_channel;
use tokio::time::timeout;
use std::collections::HashMap;
use std::time::Duration;
use std::{env};
use std::net::{SocketAddrV4, SocketAddr};
use std::sync::{Arc};
use proxy_protocol::{version2, ProxyHeader};
use tokio::{self, select};
mod utils;
use utils::*;
mod mmproxy;
use mmproxy::*;

use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn print_usage(program: &str, opts: Options) {
    let program_path = std::path::PathBuf::from(program);
    let program_name = program_path.file_stem().unwrap().to_str().unwrap();
    let brief = format!("Usage: {} -m MODE [-b BIND_ADDR] -l LOCAL_PORT -h REMOTE_ADDR -r REMOTE_PORT -p",
                        program_name);
    print!("{}", opts.usage(&brief));
}

fn main() {
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

    let mut cpus = num_cpus::get();

    let mut workers = vec![];

    if mode == 1{

        while cpus != 0 {
            let bind_addr = bind_addr.clone();
            let remote_host = remote_host.clone();
            workers.push(std::thread::spawn(move || {
                let rt = Builder::new_current_thread().enable_all().build().unwrap();
                rt.block_on(forward(&bind_addr, local_port, &remote_host, remote_port , enable_proxy_protocol, &rt ));
            }));
            cpus -= 1;
        }

        for _ in 0..workers.len() {
            workers.pop().unwrap().join().unwrap();
        }

    } else if mode == 2{

        while cpus != 0 {
            let bind_addr = bind_addr.clone();
            let remote_host = remote_host.clone();
            workers.push(std::thread::spawn(move || {
                let rt = Builder::new_current_thread().enable_all().build().unwrap();
                rt.block_on(forward_mmproxy(&bind_addr, local_port, &remote_host, remote_port , &rt ));
            }));
            cpus -= 1;
        }

        for _ in 0..workers.len() {
            workers.pop().unwrap().join().unwrap();
        }
        
    } else {
        std::process::exit(-1);
    }
}

async fn forward(bind_addr: &str, local_port: u32, remote_host: &str, remote_port: u32 , enable_proxy_protocol : bool , rt : &Runtime) {

    let local_addr = format!("{}:{}", bind_addr, local_port);
    let local_socket = match UdpBuilder::new_v4().unwrap()
        .reuse_address(true).unwrap()
        .reuse_port(true).unwrap()
        .bind(local_addr.clone()) {
            Ok(p) => p,
            Err(_) => {
                log::error!("listen to {} faild!" , local_addr);
                return;
            },
        };
    local_socket.set_nonblocking(true).unwrap();
    let local_socket = UdpSocket::from_std(local_socket).unwrap();

    log::info!("listen to {}" , local_addr);

    if enable_proxy_protocol {
        log::info!("enable proxy-protocol");
    }

    let remote_addr = format!("{}:{}", remote_host, remote_port);

    let mut buf = [0; 64 * 1024];

    let ( c_send , mut c_recv) = unbounded_channel::<(SocketAddr, Vec<u8>)>();

    let send_lck = Arc::new(Mutex::new(c_send));

    let socket_addr_map: Arc<Mutex<HashMap<SocketAddr , (Arc<UdpSocket>, i64)>>> = Arc::new(Mutex::new(HashMap::new()));

    loop{
        select! {
            a = local_socket.recv_from(&mut buf) => {
                let mut socket_addr_map_lck = socket_addr_map.lock().await;
                let (size, src_addr) = a.unwrap();
                let mut old_stream = false;
                let upstream: Arc<UdpSocket>;

                log::info!("recv from [{}:{}] size : {} " , src_addr.ip().to_string() , src_addr.port() , size);

                if let std::collections::hash_map::Entry::Vacant(e) = socket_addr_map_lck.entry(src_addr) {
                    upstream = Arc::new(UdpSocket::bind(bind_addr.to_string() + ":0").await.unwrap());
                    e.insert((upstream.clone(), cur_timestamp()));

                    log::info!("bind new forwarding address [{}:{}] " , upstream.local_addr().unwrap().ip().to_string() , upstream.local_addr().unwrap().port());
                } else {
                    upstream = socket_addr_map_lck[&src_addr].0.clone();
                    socket_addr_map_lck.get_mut(&src_addr).unwrap().1 = cur_timestamp();
                    old_stream = true;
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
                    rt.spawn(async move {
                        let mut buf = [0; 64 * 1024];
                        loop{
                            match timeout(Duration::from_secs(TIMEOUT_SECOND) ,upstream.recv_from(&mut buf)).await{
                                Ok(p) => {
                                    let size = p.unwrap().0;
                                    log::info!("send downstream to [{}:{}] size : {} " , src_addr.ip().to_string() , src_addr.port() , size);
                                    send_lck.lock().await.send((src_addr , buf[..size].to_vec())).unwrap();
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
            b = c_recv.recv() => {
                let (src_addr , data) = b.unwrap();
                local_socket.send_to(data.as_slice() , src_addr).await.unwrap();
            }
        }
    }
}