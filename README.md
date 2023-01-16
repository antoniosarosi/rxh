# RXH

RXH is an HTTP reverse proxy built with [`hyper`](https://github.com/hyperium/hyper)
and [`tokio`](https://github.com/tokio-rs/tokio) just for fun. For now, the
configuration file ([`rxh.json`](rxh.json)) only accepts this:

```json
{
    "listen": "127.0.0.1:8100",
    "target": "127.0.0.1:8080",
    "prefix": "/api"
}
```

`"listen"` is the address where the proxy accepts connections (full socket
address including IP and port) and `"target"` is the address where requests are
forwarded (full socket address). Finally, `"prefix"` is an optional
string that will make RXH return `HTTP 404` for any request whose URI doesn't
start with such string. If omitted, the default prefix is `"/"`.

Start the server using `cargo`:

```bash
cargo run
```

# Features

- [x] HTTP `Forwarded` header ([RFC 7239](https://www.rfc-editor.org/rfc/rfc7239)).
- [x] Graceful shutdown (don't kill the process until all sockets are closed).
- [x] HTTP/1.1 upgraded connections (works like a TCP tunnel).
- [ ] HTTP `Via` header ([Section 3.6.7 of RFC 5322](https://httpwg.org/specs/rfc9110.html#field.via))
- [ ] Static files server.
- [ ] Multiple servers on different ports, both static and proxy.
- [ ] Header customization configs (see [`config.sketch.json`](config.sketch.json)).
- [ ] Hot reloading (switch the config on the fly without stopping).
- [ ] Cache.
- [ ] Load balancing.
- [ ] Dameonize process.
- [ ] TLS.
