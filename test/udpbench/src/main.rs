use async_std::{io, net::UdpSocket};
use futures::future::*;
use getopts::Options;
use net2::{UdpBuilder, unix::UnixUdpBuilderExt};
use std::{env};
use chrono::prelude::*;

pub fn cur_timestamp() -> i64{
    let dt = Local::now();
    dt.timestamp_millis()
}

async fn listen(port : i32) {
    let local_addr = format!("0.0.0.0:{}", port);
    let local_socket = match UdpBuilder::new_v4().unwrap()
        .reuse_address(true).unwrap()
        .reuse_port(true).unwrap()
        .bind(local_addr.clone()) {
            Ok(p) => p,
            Err(_) => {
                return;
            },
        };
    let local_socket = UdpSocket::from(local_socket);

    let mut buf =[0u8;1024];

    loop {
        let (size , addr) = local_socket.recv_from(&mut buf).await.unwrap();
        local_socket.send_to(&buf[..size], addr).await.unwrap();
    }
}

async fn load_once(addr : &str) {

    let mut buf =[0u8;1024];

    let cli = UdpSocket::bind("0.0.0.0:0").await.unwrap();
    cli.send_to(&[1,2,3,4], addr).await.unwrap();
    cli.recv_from(&mut buf).await.unwrap();
}

#[async_std::main]
async fn main() -> io::Result<()>  {

    let args: Vec<String> = env::args().collect();

    let mut opts = Options::new();

    opts.optopt("l",
                "local-port",
                "listen a port for echo server",
                "LOCAL_PORT");
    opts.optopt("r",
                "remote-addr",
                "test target address",
                "REMOTE_ADDR");
    opts.optopt("c",
                "count",
                "test request count",
                "COUNT");
    opts.optflag("s",
                "slient",
                "disable print log");

    let matches = opts.parse(&args[1..])
        .unwrap_or_else(|_| {
                            std::process::exit(-1);
                        });
    
    if matches.opt_present("l") {
        let port: i32 = matches.opt_str("l").unwrap().parse().unwrap();

        let mut workers = vec![];
        let mut cpus = num_cpus::get()*2;

        while cpus != 0 {
            workers.push(listen(port));
            cpus -= 1;
        }

        join_all(workers).await;

    } else if matches.opt_present("r") {
        let target = matches.opt_str("r").unwrap();
        let mut count: i32 = matches.opt_str("c").unwrap().parse().unwrap();

        let mut workers = vec![];
        while count != 0 {
            workers.push(load_once(&target));
            count -= 1;
        }
        let t = cur_timestamp();
        join_all(workers).await;
        println!("cost times : {} ms" , cur_timestamp() - t);
    }
    Ok(())
}