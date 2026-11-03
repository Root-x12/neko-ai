# 🐱 Neko AI

A self-hosted, browser-based chat app for **any AI provider**: bring your own API key, run nothing locally.
The backend is Rust (Axum + SQLite) and the frontend is Rust compiled to WebAssembly (Leptos).
It works on your computer and your phone, and your chats sync between them.

**Two moods, one switch:**

- **Neko** 🐾 is soft and cozy, for relaxing. Palettes: Sakura, Matcha, Mochi, Lavender.
- **Cyber** ⚡ is neon and sharp, for coding. Palettes: Neon, Matrix, Synthwave, Ice.

Each mode has a light and a dark version. There's an animated cat mascot that blinks and wags its tail, "thinks" while the AI answers, and dozes off when the app is idle. It wears a scanning visor in Cyber mode.

## Features (Phase 1)

| | |
|---|---|
| **Providers** | OpenAI, Anthropic, Google Gemini, OpenRouter, Groq, Mistral, DeepSeek, xAI, Together, Ollama, LM Studio, or any OpenAI-compatible URL |
| **Chat** | Streaming answers, Markdown, syntax-highlighted code with copy buttons, tables, task lists |
| **Control** | Switch model mid-chat, system prompt, temperature / top-p / max tokens, regenerate, edit & resend, stop |
| **History** | Every chat is saved; full-text search across titles and messages; rename and delete |
| **Accounts** | Sign-up and sign-in, admin/user roles, a one-time setup code for the first admin, login rate limiting |
| **Devices** | Address and QR code to open on your phone, list of signed-in devices, remote sign-out, installable as an app |
| **Privacy** | No telemetry, no CDNs, no external fonts. API keys are encrypted at rest (ChaCha20-Poly1305) and never sent back to the browser |

## Quick start

You need [Rust](https://rustup.rs) (stable).

```bash
# one-time setup
rustup target add wasm32-unknown-unknown
cargo install trunk --locked

# build the web UI
cd crates/web && trunk build --release && cd ../..

# run the server
cargo run --release -p neko-server
```

The terminal prints the addresses and a **setup code**. Open the address in your browser, create the admin account using that code, then go to **Settings → Providers** and add an API key.

### Use it from your phone

By default the server listens on all network interfaces (`0.0.0.0:8080`). With your phone on the same Wi-Fi, scan the QR code shown in the terminal, or under **Devices** in the app. Sign in with the same account and your chats are there. Use "Add to Home Screen" to get an app icon.

To keep it on this computer only, run with `NEKO_BIND=127.0.0.1:8080`.

> Over plain `http://` on a LAN, traffic between your phone and the server isn't encrypted. If you use it outside your home network, put it behind HTTPS (for example Caddy, or Tailscale) and set `NEKO_SECURE_COOKIES=true`.

## Configuration

| Variable | Default | Meaning |
|---|---|---|
| `NEKO_BIND` | `0.0.0.0:8080` | Address to listen on |
| `NEKO_DATA_DIR` | `data` | Database and secret key location |
| `NEKO_WEB_DIR` | `crates/web/dist` | Built web UI |
| `NEKO_ALLOW_SIGNUP` | `false` | Let others create accounts after the admin |
| `NEKO_SECURE_COOKIES` | `false` | Mark the session cookie `Secure` (use with HTTPS) |
| `NEKO_SECRET_KEY` | *(generated)* | Base64 32-byte key for encrypting API keys; otherwise `data/secret.key` is created |
| `RUST_LOG` | `info` | Log level |

**Back up `data/`**. If you lose `secret.key`, the stored API keys can no longer be decrypted (chats are unaffected).

## Development

```bash
cargo test -p neko-server -p neko-shared -p neko-web   # unit + end-to-end API tests (mock providers)
cargo clippy -p neko-web --target wasm32-unknown-unknown
cargo run -p neko-server                                # API on :8080
cd crates/web && trunk serve                            # UI with hot reload on :8081, proxies /api
```

```
crates/
  shared/   types shared by server & web, SSE parser
  server/   Axum API, SQLite (FTS5 search), auth, provider adapters, streaming
  web/      Leptos UI, themes (style/main.css), mascot, PWA assets
```

## Roadmap

1. ✅ Core: providers, streaming, Markdown and code, history and search, model switching, parameters, auth
2. ✅ Themes and devices: Neko/Cyber, palettes, light/dark, phone layout, device sessions
3. Tools: MCP client, web search and URL fetch toggles, PDF and image uploads
4. Canvas: HTML, SVG, Mermaid and React previews in a side panel
5. Security: PII and secret detection before sending, prompt-injection scanning, audit logs, team roles, model allowlists, retention policies
6. Extras: export to Markdown/PDF/JSON, memory across chats, RAG over your files
