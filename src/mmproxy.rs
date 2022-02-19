use std::{net::{SocketAddr, SocketAddrV4, IpAddr}, io::{Error, ErrorKind}};

use async_std::net::UdpSocket;
use proxy_protocol::parse;
use udp_sas::UdpSas;

fn calc_header_size(version : i32 , buf : &[u8]) -> usize{

    if version == 1{
        let mut idx = 0;
        while buf[idx] != b'\n'{
            idx += 1;
        }
        idx+1
    } else if version == 2 {
        let a = u16::from_be_bytes([buf[14] , buf[15]]);
        (a + 16) as usize
    } else {
        0
    }
}

fn parse_proxy_protocol(mut buf : &[u8]) -> Result<(SocketAddrV4 , usize) , Error> {
    
    let mut version = 0;
    
    let addr = match parse(&mut buf) {
        Ok(p) => { 
            match p{
                proxy_protocol::ProxyHeader::Version1 { addresses } => {
                    match addresses{
                        proxy_protocol::version1::ProxyAddresses::Unknown => Err(Error::new(ErrorKind::Other,"parse faild")),
                        proxy_protocol::version1::ProxyAddresses::Ipv4 { source, destination : _ } => {version = 1; Ok(source)},
                        proxy_protocol::version1::ProxyAddresses::Ipv6 { source: _, destination : _ } => Err(Error::new(ErrorKind::Other,"parse faild")),
                    }
                },
                proxy_protocol::ProxyHeader::Version2 { command : _, transport_protocol : _, addresses } => {
                    match addresses{
                        proxy_protocol::version2::ProxyAddresses::Unspec => Err(Error::new(ErrorKind::Other,"parse faild")),
                        proxy_protocol::version2::ProxyAddresses::Ipv4 { source, destination : _ } => {version = 2; Ok(source)},
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

    Ok((addr , calc_header_size(version ,buf)))
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

    let mut buf = [0; 64 * 1024];

    loop{
        let (size , _) = local_socket.recv_from(&mut buf).await.unwrap();
        let buf = &buf[..size];
        let cli = std::net::UdpSocket::bind_sas("127.0.0.1:0".parse::<SocketAddr>().unwrap()).unwrap();
        
        let (src_addr , size) = parse_proxy_protocol(buf).unwrap();
        
        cli.send_sas(&buf[size..], &remote_addr ,&IpAddr::V4(*src_addr.ip())).unwrap();
    }
}