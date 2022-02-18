use async_std::sync::Mutex;
use futures::FutureExt;
use getopts::Options;
use std::collections::HashMap;
use std::{env};
use std::net::{SocketAddrV4, SocketAddr};
use std::sync::{Arc};
use proxy_protocol::{version2, ProxyHeader};
use async_std::{io, net::{UdpSocket}, task};
use futures::select;

fn print_usage(program: &str, opts: Options) {
    let program_path = std::path::PathBuf::from(program);
    let program_name = program_path.file_stem().unwrap().to_str().unwrap();
    let brief = format!("Usage: {} [-b BIND_ADDR] -l LOCAL_PORT -h REMOTE_ADDR -r REMOTE_PORT -p",
                        program_name);
    print!("{}", opts.usage(&brief));
}

#[async_std::main]
async fn main() -> io::Result<()>  {
    let args: Vec<String> = env::args().collect();
    let program = args[0].clone();

    let mut opts = Options::new();
    opts.reqopt("l",
                "local-port",
                "The local port to which udpproxy should bind to",
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
                "enable proxy-protocol transparent");

    let matches = opts.parse(&args[1..])
        .unwrap_or_else(|_| {
                            print_usage(&program, opts);
                            std::process::exit(-1);
                        });
    
    let enable_proxy_protocol = matches.opt_present("p");
    let local_port: u32 = matches.opt_str("l").unwrap().parse().unwrap();
    let remote_port: u32 = matches.opt_str("r").unwrap().parse().unwrap();
    let remote_host = matches.opt_str("h").unwrap();
    let bind_addr = match matches.opt_str("b") {
        Some(addr) => addr,
        None => "127.0.0.1".to_owned(),
    };

    forward(&bind_addr, local_port, &remote_host, remote_port , enable_proxy_protocol).await;

    return Ok(());
}

async fn forward(bind_addr: &str, local_port: u32, remote_host: &str, remote_port: u32 , enable_proxy_protocol : bool) {

    let local_addr = format!("{}:{}", bind_addr, local_port);
    let local_socket = UdpSocket::bind(&local_addr).await.unwrap();

    let remote_addr = format!("{}:{}", remote_host, remote_port);

    let mut buf = [0; 64 * 1024];

    let ( c_send , c_recv) = async_std::channel::unbounded::<(SocketAddr, Vec<u8>)>();

    let send_lck = Arc::new(Mutex::new(c_send));

    let mut socket_addr_map: HashMap<SocketAddr , Arc<UdpSocket>> = HashMap::new();

    loop{
        select! {
            a = local_socket.recv_from(&mut buf).fuse() => {
                let (size, src_addr) = a.unwrap();
                let mut old_stream = false;
                let upstream: Arc<UdpSocket>;

                if socket_addr_map.contains_key(&src_addr) {
                    upstream = socket_addr_map[&src_addr].clone();
                    old_stream = true;
                } else {
                    upstream = Arc::new(UdpSocket::bind(bind_addr.to_string() + ":0").await.unwrap());
                    socket_addr_map.insert(src_addr, upstream.clone());
                }

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
                    task::spawn(async move {
                        loop{
                            let mut buf = [0; 64 * 1024];
                            let (size , _) = upstream.recv_from(&mut buf).await.unwrap();
                            send_lck.lock().await.send((src_addr , buf[..size].to_vec())).await.unwrap();
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