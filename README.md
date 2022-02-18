# udppp [![Build Status](https://img.shields.io/github/workflow/status/b23r0/udppp/Rust)](https://github.com/b23r0/udppp/actions/workflows/rust.yml) [![ChatOnDiscord](https://img.shields.io/badge/chat-on%20discord-blue)](https://discord.gg/ZKtYMvDFN4) [![Crate](https://img.shields.io/crates/v/udppp)](https://crates.io/crates/udppp)
High performence UDP reverse proxy support with Proxy Protocol

# Features

* Async
* Support Proxy Protocol V2
* No unsafe code
* Single executable
* Linux/Windows/Mac/BSD support

# Build & Run

`$> cargo build --release`

# Installation

`$> cargo install udppp`

# Usage

```
Usage: udppp [-b BIND_ADDR] -l LOCAL_PORT -h REMOTE_ADDR -r REMOTE_PORT -p

Options:
    -l, --local-port LOCAL_PORT
                        The local port to which udpproxy should bind to
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