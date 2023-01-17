# RXH

RXH is an HTTP reverse proxy built with [`hyper`](https://github.com/hyperium/hyper)
and [`tokio`](https://github.com/tokio-rs/tokio) just for fun. For now, the
configuration file ([`rxh.toml`](rxh.toml)) only accepts this:

```toml
[[server]]

kind = "proxy"
listen = "127.0.0.1:8100"
target = "127.0.0.1:8080"

[[server]]

kind = "static"
listen = "127.0.0.1:8200"
root = "/home/user/website"
```

Multiple servers can be configured to run on different ports, each one of them
with its own configuration. `"proxy"` servers forward data to an upstream server
whose address is specified by `"target"`. All addresses must include IP and port
number. On the other hand, `"static"` servers simply serve static files from
the `"root"` directory, which must be an absolute path.

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
- [ ] Load balancing.
- [ ] Dameonize process.
- [ ] TLS.
