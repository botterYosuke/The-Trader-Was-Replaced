# wasm_lsp — bevscode in the browser, LSP over WebSocket

The same `bevscode` editor surface that runs natively, compiled to
`wasm32-unknown-unknown` and served by Trunk. Language-server requests
travel over a WebSocket to a tiny Node bridge that pipes them into a
real `rust-analyzer` running on the host.

```
  ┌──────────────┐       ws       ┌──────────────┐    stdio    ┌──────────────┐
  │   bevscode   │ <───────────── │   bridge      │ <─────────  │ rust-analyzer │
  │  (browser)   │ ─────────────> │ (Node @ 9876) │ ─────────>  │  (subprocess) │
  └──────────────┘                └──────────────┘             └──────────────┘
        │                ▲
        │ http           │ trunk reverse-proxies /lsp → :9876
        ▼                │
  ┌──────────────┐        │
  │ trunk @ 8765 │ ───────┘
  └──────────────┘
```

## Prerequisites

- `cargo install trunk wasm-bindgen-cli`
- `rustup target add wasm32-unknown-unknown`
- `rustup component add rust-analyzer` (or any other LSP server you point
  `LSP_CMD` at)
- Node 20+
- Homebrew LLVM if you're on macOS: `arborium` parsers need a wasm-capable
  C compiler. Apple `clang` doesn't target wasm32 — install
  `brew install llvm` and either put the env vars in `~/.cargo/config.toml`
  or prefix the build:

  ```bash
  export CC_wasm32_unknown_unknown=/opt/homebrew/opt/llvm/bin/clang
  export AR_wasm32_unknown_unknown=/opt/homebrew/opt/llvm/bin/llvm-ar
  ```

## Run it

Two terminals. From `examples/wasm_lsp/`:

```bash
# terminal 1 — start the LSP bridge
cd bridge && npm install && npm start

# terminal 2 — build + serve the wasm app
trunk serve
```

Then open <http://127.0.0.1:8765>. The Bevy app connects to `/lsp`,
which Trunk reverse-proxies as a WebSocket to `ws://127.0.0.1:9876/`
(the bridge), which spawns one fresh `rust-analyzer` per browser tab.

### Point at a different server

```bash
LSP_CMD=pyright-langserver LSP_ARGS="--stdio" npm start
```

## How the wasm side is wired

`LspClient::start_with(WebSocketTransport::new(url))` is the only
target-specific call. Everything above the transport
(`LspMessage::Initialize`, the response router, the per-document state)
is identical to the native `editor_lsp` example.

The transport adapter lives in
`crates/bevy_lsp/src/transport/websocket.rs` and turns the gloo-net
WebSocket into the `AsyncRead + AsyncWrite` pair that
`async-lsp::MainLoop::run_buffered` consumes — so the rest of the
LSP plumbing doesn't know it's in a browser.

## Caveats

- `bevscode` uses the `clipboard-wasm` feature here. Read-from-clipboard
  is `None` (browser security); writes go through
  `navigator.clipboard.writeText`.
- Single browser thread: Bevy must be built without `multi_threaded`
  (this crate's `Cargo.toml` already omits it).
- The bridge spawns one rust-analyzer per WebSocket connection.
  Refreshing the tab kills and respawns it.
