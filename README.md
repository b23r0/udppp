# udppp
High performence UDP reverse proxy support with Proxy Protocol

# Features

* Async
* Support Protocol V2
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
                        enable proxy-protocol transparent

```