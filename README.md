# vercel-xhttp-relay

A high-performance **Rust relay server for Vercel Edge** that forwards
**XHTTP** traffic to your backend Xray/V2Ray server. Use Vercel's globally
distributed edge network (and its `vercel.com` / `*.vercel.app` SNI) as a
front for your real Xray endpoint — useful in regions where the backend
host is blocked but Vercel is reachable.

> ⚠️ **XHTTP transport only.** This relay is purpose-built for Xray's
> `xhttp` transport. It will **not** work with `WebSocket`, `gRPC`, `TCP`,
> `mKCP`, `QUIC`, or any other V2Ray/Xray transport.

---

## How It Works

```
┌──────────┐    TLS / SNI: *.vercel.app    ┌──────────────────┐    HTTP/2     ┌──────────────┐
│  Client  │ ────────────────────────────► │   Vercel (this   │ ───────────►  │  Your Xray   │
│ (v2rayN, │   XHTTP request (POST/GET)    │  Rust function)  │  XHTTP frames │  server with │
│ xray-core│                               │  buffers + fwd   │  forwarded    │ XHTTP inbound│
└──────────┘                               └──────────────────┘               └──────────────┘
```

1. Your Xray client opens an XHTTP request to a Vercel domain
   (`your-app.vercel.app`, or any custom domain pointed at Vercel).
2. The TLS handshake uses **Vercel's certificate / SNI**, so to a censor it
   looks like ordinary traffic to a legitimate Vercel-hosted site.
3. The Rust function receives the request, copies the headers, method, and
   body, then issues an equivalent request to your real Xray server defined
   by `TARGET_DOMAIN`.
4. The upstream response (status, headers, body) is returned to the client.

## Important: Buffered, not Streamed

Vercel's Rust runtime is built on AWS Lambda's request/response model:
the entire request body is buffered in memory before your handler runs,
and the entire response body is buffered before being sent back. **There
is no true bidirectional streaming.**

This is why the relay is **XHTTP-only**:

- XHTTP's `packet-up` / chunked POST mode uses many short, bounded HTTP
  requests — each one fits naturally into Lambda's request/response model
  and works through this relay without trouble.
- Transports that rely on long-lived bidirectional streams (WebSocket,
  gRPC, raw TCP, mKCP, QUIC) **cannot** work on Vercel's serverless Rust
  runtime, regardless of how the relay is implemented.

If you need true streaming behind Vercel, you'd need Vercel's Edge
*Middleware* (JavaScript only, with `WebStreams`) — not the Rust runtime.

## Why Rust?

- **Compiled native code** — header copying and request building run in
  microseconds, minimizing the relay's contribution to total latency.
- **HTTP/2 client by default** (`reqwest` with `http2_prior_knowledge`)
  — matches the protocol XHTTP backends speak.
- **Vercel's anycast edge** — clients connect to the closest PoP and
  benefit from Vercel's optimized backbone to your origin.

---

## Setup & Deployment

### 1. Requirements

- A working **Xray server with XHTTP inbound** already running on a public
  host (this is your `TARGET_DOMAIN`).
- [Vercel CLI](https://vercel.com/docs/cli): `npm i -g vercel`
- A Vercel account (Pro recommended for higher bandwidth and concurrent
  connection limits).

### 2. Configure Environment Variable

In the Vercel Dashboard → your project → **Settings → Environment Variables**,
add:

| Name            | Example                          | Description                                   |
| --------------- | -------------------------------- | --------------------------------------------- |
| `TARGET_DOMAIN` | `https://xray.example.com:2096`  | Full URL of your backend Xray XHTTP endpoint. |

> Use `https://` if your backend terminates TLS, `http://` if it's plain.
> Include a non-default port if needed.

### 3. Deploy

```bash
git clone https://github.com/ramynn/vercel-xhttp-relay.git
cd vercel-xhttp-relay

vercel --prod
```

After deployment Vercel gives you a URL like `your-app.vercel.app`.

---

## Client Configuration (VLESS / Xray with XHTTP)

In your client config, point the **address** at your Vercel domain and set
**SNI / Host** to a `vercel.com`-family hostname. The `id`, `path`, and
inbound settings must match what your real Xray server expects.

### Example VLESS share link

```
vless://UUID@your-app.vercel.app:443?encryption=none&security=tls&sni=your-app.vercel.app&type=xhttp&path=/yourpath&host=your-app.vercel.app#vercel-relay
```

### Example Xray client JSON (outbound)

```json
{
  "protocol": "vless",
  "settings": {
    "vnext": [{
      "address": "your-app.vercel.app",
      "port": 443,
      "users": [{ "id": "YOUR-UUID", "encryption": "none" }]
    }]
  },
  "streamSettings": {
    "network": "xhttp",
    "security": "tls",
    "tlsSettings": {
      "serverName": "your-app.vercel.app",
      "allowInsecure": false
    },
    "xhttpSettings": {
      "path": "/yourpath",
      "host": "your-app.vercel.app",
      "mode": "auto"
    }
  }
}
```

### Tips

- You can use **any Vercel-fronted hostname** for SNI as long as the TLS
  handshake reaches Vercel. Custom domains pointed at Vercel work too.
- The `path` and `id` (UUID) must match the **backend Xray** XHTTP inbound,
  not this relay — the relay is transport-agnostic and just pipes bytes.
- If censorship targets `*.vercel.app` directly, attach a custom domain in
  the Vercel dashboard and use that as both `address` and `sni`.

---

## Limitations

- **XHTTP only.** WebSocket / gRPC / raw TCP / mKCP / QUIC transports do
  **not** work because Vercel's serverless Rust runtime cannot stream.
- **Bounded request size.** Each XHTTP request body is fully buffered in
  memory by the runtime; very large single chunks may hit Vercel's request
  size limit (≈ 4.5 MB for serverless functions).
- **Function execution time.** Each request must finish within Vercel's
  per-invocation timeout. XHTTP's chunked POST/GET model is short-lived
  per request, so this is normally fine.
- **Bandwidth costs.** All traffic counts against your Vercel account's
  bandwidth quota. Heavy use → upgrade to Pro/Enterprise.
- **Logging.** Vercel logs request metadata (path, IP, status). The body is
  not logged, but be aware of the trust model.

## Project Layout

```
.
├── api/index.rs   # Serverless function: forwards request → TARGET_DOMAIN, returns response
├── Cargo.toml     # Rust dependencies (vercel_runtime, reqwest, tokio)
├── vercel.json    # Routes all paths → /api/index, region pinned to fra1
└── README.md
```

To change the deployment region, edit `regions` in `vercel.json` (e.g.
`["sin1"]`, `["iad1"]`).

## License

MIT.
