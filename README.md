# Tentrackule

A Discord bot that tracks League of Legends players and posts a rich **match card**
to your server whenever a tracked player finishes a game.

The card is a modern HTML/CSS layout — champion splash, KDA, per-role performance
**grade**, KP / damage share / vision breakdown, the lane matchup, team objectives,
and either the ranked block (rank, LP delta, win-rate, recent-form streak) or a
champion-**mastery** block for unranked queues.

## How it works

Rendering is delegated to Cloudflare so the bot itself stays lean (256 MB, no
browser, no font stack):

```
  tentrackule (bot, 256 MB)                     Cloudflare Browser Run
  ┌──────────────────────────┐  POST /screenshot ┌──────────────────────────────┐
  │ • poll Riot for new match │  { html }  ─────▶ │ Managed headless Chromium    │
  │ • build card HTML (Rust)  │                   │ • pre-installed CJK fonts    │
  │ • inline assets & fonts   │ ◀──── JPEG bytes  │ • crops #card-root           │
  │   as data URIs (cache)    │                   │ • billed per browser second  │
  └──────────────────────────┘                   └──────────────────────────────┘
            │ JPEG ─▶ Discord attachment
```

- **Card HTML** is assembled in Rust (`src/discord/scorecard.rs`) from a fully
  resolved view-model — no template engine, every dynamic field HTML-escaped.
- **Assets** (splash, spells, runes, items, opponent icon) are fetched once through
  a disk+memory cache (`ImageCache`) and inlined as `data:` URIs, like the Latin web
  fonts, so the render needs no network. CJK names use the fonts pre-installed in the
  browser.
- **Rendering** POSTs the HTML to [Cloudflare Browser Run](https://developers.cloudflare.com/browser-run/),
  which waits for `load` and `document.fonts.ready`, then screenshots the `#card-root`
  element at `deviceScaleFactor: 2`. Rate limits honour `Retry-After`.
- **Fallback:** if the renderer is unset, out of quota or unreachable, the match is
  still announced as a compact **text embed**, so a render outage never blocks tracking.

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
| `POLLING_INTERVAL_SECS` | | `60` | Base poll tick / fastest cadence; idle players are polled less often (activity-adaptive) |
| `RIOT_RATE_LIMIT_PER_SECOND` | | `20` | Client-side rate limit (fast window) |
| `RIOT_RATE_LIMIT_PER_TWO_MIN` | | `100` | Client-side rate limit (2-minute window) |
| `DDRAGON_VERSION` | | *(auto)* | Data Dragon patch. Unset → resolved from `versions.json` at boot and refreshed every 24h; set to pin a patch |
| `CLOUDFLARE_ACCOUNT_ID` | | *(unset)* | Cloudflare account running Browser Run; unset → text-embed fallback |
| `CLOUDFLARE_API_TOKEN` | | *(unset)* | API token with the `Browser Run - Edit` permission (secret) |
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

To render real cards on Cloudflare (Latin, CJK and Cyrillic names), add the two
`CLOUDFLARE_*` variables to `.env`, then review the JPEGs written to
`target/scorecard-samples/`:

```sh
cargo test renders_real_cards_on_cloudflare -- --ignored --nocapture
```

## Deployment

- **`tentrackule`** — the bot on Fly.io (`fly.toml`, 256 MB, persistent volume for the DB).
- **Cloudflare Browser Run** — the managed renderer, with nothing to deploy. Setup,
  limits and failure modes are in [`docs/renderer-deploy.md`](docs/renderer-deploy.md).

## Docs

- [`docs/html-scorecard-roadmap.md`](docs/html-scorecard-roadmap.md) — design &
  implementation roadmap for the HTML match card.
- [`docs/renderer-deploy.md`](docs/renderer-deploy.md) — renderer setup guide.
- [`docs/optimization-roadmap.md`](docs/optimization-roadmap.md) — resource, API
  and infra-cost optimization roadmap.
