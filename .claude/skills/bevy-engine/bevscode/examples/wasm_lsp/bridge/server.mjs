#!/usr/bin/env node
// WebSocket-to-stdio bridge. Listens on :9876 and forwards every WebSocket
// connection to a fresh `rust-analyzer` child process (or whatever LSP_CMD
// names). Bytes flow verbatim in both directions — `async-lsp` on the
// browser side already handles JSON-RPC framing.

import { spawn } from "node:child_process";
import { WebSocketServer } from "ws";

const PORT = Number(process.env.PORT ?? 9876);
const LSP_CMD = process.env.LSP_CMD ?? "rust-analyzer";
const LSP_ARGS = (process.env.LSP_ARGS ?? "").split(" ").filter(Boolean);

const wss = new WebSocketServer({ port: PORT, host: "127.0.0.1" });
console.log(`[bridge] listening on ws://127.0.0.1:${PORT}`);
console.log(`[bridge] spawning ${LSP_CMD} ${LSP_ARGS.join(" ")} per connection`);

wss.on("connection", (ws, req) => {
    console.log(`[bridge] client connected from ${req.socket.remoteAddress}`);
    const child = spawn(LSP_CMD, LSP_ARGS, { stdio: ["pipe", "pipe", "pipe"] });

    child.stdout.on("data", (chunk) => {
        if (ws.readyState === ws.OPEN) ws.send(chunk);
    });
    child.stderr.on("data", (chunk) => {
        process.stderr.write(`[${LSP_CMD}] ${chunk}`);
    });
    child.on("exit", (code, signal) => {
        console.log(`[bridge] ${LSP_CMD} exited code=${code} signal=${signal}`);
        if (ws.readyState === ws.OPEN) ws.close();
    });

    ws.on("message", (data) => {
        // Trunk delivers WS messages as Buffer/Uint8Array; pipe straight
        // through to the language server's stdin.
        if (Buffer.isBuffer(data)) {
            child.stdin.write(data);
        } else if (data instanceof ArrayBuffer) {
            child.stdin.write(Buffer.from(data));
        } else if (typeof data === "string") {
            child.stdin.write(data);
        } else if (Array.isArray(data)) {
            for (const piece of data) child.stdin.write(piece);
        }
    });

    ws.on("close", () => {
        console.log("[bridge] client disconnected; killing LSP child");
        child.kill();
    });
    ws.on("error", (err) => {
        console.error("[bridge] ws error:", err);
        child.kill();
    });
});

wss.on("error", (err) => {
    console.error("[bridge] server error:", err);
    process.exit(1);
});

process.on("SIGINT", () => {
    console.log("[bridge] shutting down");
    wss.close(() => process.exit(0));
});
