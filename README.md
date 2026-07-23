# Tentrackule

A Discord bot that tracks League of Legends players and posts a rich **match card**
to your server whenever a tracked player finishes a game.

The card is a modern HTML/CSS layout — champion splash, KDA, per-role performance
**grade**, KP / damage share / vision breakdown, the lane matchup, team objectives,
and either the ranked block (rank, LP delta, win-rate, recent-form streak) or a
champion-**mastery** block for unranked queues.

## How it works

Rendering is split across two Fly.io apps so the bot itself stays lean (256 MB, no
browser, no font stack):

```
  tentrackule (bot, 256 MB)                     tentrackule-renderer (Browserless)
  ┌──────────────────────────┐  POST /screenshot ┌──────────────────────────────┐
  │ • poll Riot for new match │  { html }  ─────▶ │ Chromium headless            │
  │ • build card HTML (Rust)  │                   │ • loads Google Fonts         │
  │ • inline DDragon assets   │ ◀─────  PNG bytes │ • crops #card-root           │
  │   as data URIs (cache)    │                   │ • scale-to-zero at rest      │
  └──────────────────────────┘                   └──────────────────────────────┘
            │ PNG ─▶ Discord attachment
```

- **Card HTML** is assembled in Rust (`src/discord/scorecard.rs`) from a fully
  resolved view-model — no template engine, every dynamic field HTML-escaped.
- **Assets** (splash, spells, runes, items, opponent icon) are fetched once through
  a disk+memory cache (`ImageCache`) and inlined as `data:` URIs, so the render is
  deterministic and offline.
- **Rendering** POSTs the HTML to a self-hosted [Browserless](https://www.browserless.io/)
  Chromium, which waits for `document.fonts.ready` and screenshots the `#card-root`
  element at `deviceScaleFactor: 2`.
- **Fallback:** if the renderer is unset or unreachable, the match is still announced
  as a compact **text embed** — a render outage never blocks or stalls tracking.

Data comes from Riot **match-v5**, **league-v4** and **champion-mastery-v4**, plus
Data Dragon for asset/version lookups. The performance grade is computed locally
(`src/discord/grade.rs`); the recent-form streak is read from a local SQLite table
(zero extra API calls).

## Commands

| Command | Description |
|---|---|
| `/track <game_name> <tag_line> <region>` | Start tracking a player |
| `/untrack …` | Stop tracking a player |
| `/list` | List players tracked in this server |
| `/config channel <#channel>` | Set the alert channel (needs *Manage Server*) |
| `/dev_test_alert …` | Render a sample card for any queue/outcome (dev, bot owner only) |

## Configuration

Set via environment (a `.env` file is loaded in dev — see `.env.example`):

| Variable | Required | Default | Notes |
|---|---|---|---|
| `DISCORD_TOKEN` | ✅ | — | Bot token |
| `RIOT_API_KEY` | ✅ | — | Riot API key |
| `DATABASE_URL` | | `sqlite:tentrackule.db` | SQLite connection string |
| `POLLING_INTERVAL_SECS` | | `60` | Match-poll cadence |
| `RIOT_RATE_LIMIT_PER_SECOND` | | `20` | Client-side rate limit |
| `DDRAGON_VERSION` | | `16.1.1` | Data Dragon version for assets |
| `RENDERER_URL` | | *(unset)* | Browserless base URL; unset → text-embed fallback |
| `RENDERER_TOKEN` | | *(unset)* | Renderer auth token (secret) |
| `RUST_LOG` / `LOG_FORMAT` | | `info` / — | Tracing filter; `LOG_FORMAT=json` for prod |

## Development

A Nix flake provides the toolchain (`nix develop` / `direnv`), or use a local Rust
toolchain (edition 2024).

```sh
cargo run                 # run the bot (needs DISCORD_TOKEN + RIOT_API_KEY)
cargo test                # unit tests
cargo clippy --all-targets
```

The card builder is unit-testable without any infrastructure. Tests write sample
cards to `target/scorecard-samples/*.html` (open them in a browser, or screenshot
`#card-root`, to review visually). The HTML is also locked by golden snapshots under
`src/discord/snapshots/`:

```sh
UPDATE_SNAPSHOTS=1 cargo test   # regenerate goldens after an intentional markup change
```

## Deployment

Two Fly.io apps:

- **`tentrackule`** — the bot (`fly.toml`, 256 MB, persistent volume for the DB).
- **`tentrackule-renderer`** — Browserless Chromium (`fly.renderer.toml`, ~1 GB,
  `scale-to-zero`, private network only). Deploy + wiring steps are in
  [`docs/renderer-deploy.md`](docs/renderer-deploy.md). Point the bot at it with
  `RENDERER_URL=http://tentrackule-renderer.flycast:3000` and a shared token.

## Docs

- [`docs/html-scorecard-roadmap.md`](docs/html-scorecard-roadmap.md) — design &
  implementation roadmap for the HTML match card.
- [`docs/renderer-deploy.md`](docs/renderer-deploy.md) — renderer deployment guide.
