# RXH

RXH is an HTTP reverse proxy built with [`hyper`](https://github.com/hyperium/hyper)
and [`tokio`](https://github.com/tokio-rs/tokio) just for fun. The
configuration file ([`rxh.toml`](rxh.toml)) accepts this options:

```toml
# Simple proxy example. All requests sent to port 8000 are forwarded to port
# 8080, including HTTP/1.1 upgrade requests. Upgraded requests will have their
# dedicated TCP tunnel.

[[server]]

listen = "127.0.0.1:8000"
forward = "127.0.0.1:8080"

# Simple static files server example. This server will run in parallel with the
# one defined above, as the configuration file accepts multiple server
# instances on different ports.

[[server]]

listen = "127.0.0.1:9000"
serve = "/home/user/website"

# Complex server example. In this case, the server listens on multiple IP
# addresses, should load balance requests that start with "/api" between ports
# 8080 and 8081 and also serves files from a directory.

[[server]]

listen = ["127.0.0.1:8100", "192.168.1.2:8100"]

match = [
    { uri = "/api", forward = ["127.0.0.1:8080", "127.0.0.1:8081"] },
    { uri = "/", serve = "/home/user/website" },
]

# Weighted load balancing example using WRR (Weighted Round Robin) algorithm.
# With this configuration, from every 6 requests received by the proxy at port
# 8200, 1 will be forwarded to port 8080, 3 of them will be forwarded to port
# 8081 and 2 of them to port 8082.

[[server]]

listen = ["127.0.0.1:8200", "192.168.1.2:8200"]

[[server.forward]]

algorithm = "WRR" # This is the default, and this is also the only one for now.

backends = [
    { address = "127.0.0.1:8080", weight = 1 },
    { address = "127.0.0.1:8081", weight = 3 },
    { address = "127.0.0.1:8082", weight = 2 },
]
```


Start the server using `cargo`:

```bash
cargo run
```

# Features

- [x] HTTP `Forwarded` header ([RFC 7239](https://www.rfc-editor.org/rfc/rfc7239)).
- [x] Graceful shutdown (don't kill the process until all sockets are closed).
- [x] HTTP/1.1 upgraded connections (works like a TCP tunnel).
- [ ] HTTP `Via` header ([Section 3.6.7 of RFC 5322](https://httpwg.org/specs/rfc9110.html#field.via))
- [ ] HTTP/2
- [x] Static files server.
- [x] Multiple servers on different ports, both static and proxy.
- [ ] Header customization configs (see [`config.sketch.toml`](config.sketch.toml)).
- [ ] Hot reloading (switch the config on the fly without stopping).
- [ ] Cache.
- [x] Load balancing.
- [ ] Dameonize process.
- [ ] TLS.
