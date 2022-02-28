# udppp [![Build Status](https://img.shields.io/github/workflow/status/b23r0/udppp/Rust)](https://github.com/b23r0/udppp/actions/workflows/rust.yml) [![ChatOnDiscord](https://img.shields.io/badge/chat-on%20discord-blue)](https://discord.gg/ZKtYMvDFN4) [![Crate](https://img.shields.io/crates/v/udppp)](https://crates.io/crates/udppp)
High performence UDP proxy with Proxy Protocol and mmproxy support.

# Features

* Async
* Support Proxy Protocol V2
* SOCKET preserve client IP addresses in L7 proxies(mmproxy)
* Single executable

# Build & Run

`$> cargo build --release`

# Installation

`$> cargo install udppp`

# Usage

```
Usage: udppp -m MODE [-b BIND_ADDR] -l LOCAL_PORT -h REMOTE_ADDR -r REMOTE_PORT -p

Options:
    -m, --mode MODE     1 : reverse proxy mode , 2 : mmproxy mode
    -l, --local-port LOCAL_PORT
                        The local port to which udppp should bind to
    -r, --remote-port REMOTE_PORT
                        The remote port to which UDP packets should be
                        forwarded
    -h, --host REMOTE_ADDR
                        The remote address to which packets will be forwarded
    -b, --bind BIND_ADDR
                        The address on which to listen for incoming requests
    -p, --proxyprotocol
                        enable proxy-protocol
    -s, --slient        disable print log

```

# mmproxy

The idea comes from a creative paper, thanks to the author. 

[mmproxy - Creative Linux routing to preserve client IP addresses in L7 proxies](https://blog.cloudflare.com/mmproxy-creative-way-of-preserving-client-ips-in-spectrum/)

However, cloudflare has not officially implemented mmproxy that supports udp. This project makes up for this shortcoming.

![image]( https://github.com/b23r0/udppp/blob/main/example/mmproxy.jpg)

## Run Proxy (proxy server)

Suppose the udp reverse proxy service port is 8000 , upstream server is 192.168.0.2:8001

```
./udppp -m 1 -b 0.0.0.0 -l 8000 -h 192.168.0.2 -r 8001 -p
```

## Routing setup (upstream server) 

Route all traffic originating from loopback back to loopback

```
ip rule add from 127.0.0.1/8 iif lo table 123
ip route add local 0.0.0.0/0 dev lo table 123
```

Normally, response packets coming from the application are routed to the Internet - via a default gateway. We do this by totally [abusing the AnyIP trick](https://blog.cloudflare.com/how-we-built-spectrum/) and assigning 0.0.0.0/0 to "local" - meaning that entire internet shall be treated as belonging to our machine. 

## Run mmproxy (upstream server)

Suppose the port of the application server is 127.0.0.1:8001

```
./udppp -m 2 -b 0.0.0.0 -l 8001 -h 127.0.0.1 -r 8002 -p
```
# Benchmark

load test tool : `https://github.com/b23r0/udppp/tree/main/test/udpbench`

Send a udp packet of 4 bytes and get the same return packet once, loop 1000 times. In the actual test of nginx, when the single test exceeds 1000 times, the client will not receive the return packet.

## Test Envoriment

| Envoriment    | Value           |
|-------------- |-----------      |
| OS      | Ubuntu20.04       |
| CPU           | Intel Xeon(Cascade Lake) Platinum 8269        |
| CPU Cores       | 4             |
| Memory       | 8G             |
| Network       | LAN (0.2 Gbps)             |
| Test Count    | 1k              |

## Test Result

| Project        | Language | Base        | Take Time |
|----------------|----------|-------------|-----------|
| udppp          | Rust     | Tokio   | 56 ms    |
| nginx | C     | Multi-Thread   | 54 ms    |
| [go-proxy](https://github.com/snail007/goproxy)         | Golang     | Goroutine       | 65 ms    |

`Take Time` is take the average of 10 times.

(Test Date : 28 Feb 2022)