# Renderer deployment (Phase 2)

The match card is HTML rendered to PNG by a **separate** headless-Chromium
service ([Browserless](https://www.browserless.io/)), so the bot VM stays lean at
256 MB. This is the app defined in [`fly.renderer.toml`](../fly.renderer.toml).

```
 tentrackule (bot, 256 MB)  ──POST /screenshot { html }──▶  tentrackule-renderer
   build_html(ctx)                                          Browserless / Chromium
   render_html_to_png()  ◀──────────── PNG bytes ────────   1 GB VM, scale-to-zero
                    (private Fly network, never exposed publicly)
```

## 1. Deploy the renderer

```sh
# One-off: create the app (no public IP is allocated — keep it private).
fly apps create tentrackule-renderer

# Auth token shared with the bot. Generate a strong random value.
fly secrets set -a tentrackule-renderer TOKEN="$(openssl rand -hex 32)"

# Deploy the Browserless image and allocate a Flycast (private) address.
fly deploy -c fly.renderer.toml --flycast
```

`--flycast` gives the app a **private** IP served through the Fly proxy. We reach
it at `tentrackule-renderer.flycast:3000`.

> **Why Flycast and not `.internal`?** Scale-to-zero autostart is driven by the
> Fly proxy. `.flycast` routes through the proxy (so the first request wakes the
> suspended machine); `.internal` connects straight to the instance, bypasses the
> proxy, and would never autostart a stopped machine. This is the one deviation
> from the roadmap's `…​.internal:3000`.

## 2. Verify it's reachable (from the bot machine)

```sh
fly ssh console -a tentrackule            # shell into the bot
curl -s -o /tmp/out.png -w '%{http_code}\n' \
  -X POST "http://tentrackule-renderer.flycast:3000/screenshot?token=$RENDERER_TOKEN" \
  -H 'content-type: application/json' -H 'accept: image/png' \
  -d '{"html":"<div id=card-root>hi</div>","selector":"#card-root","options":{"type":"png"}}'
# → 200, and /tmp/out.png is a valid PNG. First call may take ~1–3 s (cold start).
```

## 3. Point the bot at it

```sh
# Same value as the renderer's TOKEN secret.
fly secrets set -a tentrackule RENDERER_TOKEN="<the token from step 1>"
# RENDERER_URL is already set in fly.toml [env]; redeploy to pick up the secret.
fly deploy
```

If `RENDERER_URL` / `RENDERER_TOKEN` are unset (e.g. local dev), the bot logs a
warning at startup and posts a **text embed** for each match instead of a card —
match announcements are never blocked by the renderer being down or absent.

## Notes

- **Scale-to-zero, with a render retry as the safety net.** `min_machines_running
  = 0` lets the single machine stop when idle — the renderer does only a few cards
  a day, so an always-warm host is ~90% of the infra bill for almost no work. The
  Fly proxy (`.flycast`) autostarts the stopped machine on the first request.
  Chromium's first launch on a freshly-woken machine can overrun Browserless's
  fixed 30 s launch timeout (WS endpoint never appears → that render fails); the
  timeout is **not** configurable and the OSS image has no pre-boot, so the bot's
  render path **retries once ~25 s later**, by which point Chrome is warm (1-6 s) —
  see `render_with_retry` (Phase 3.A). A render that still fails degrades to a text
  embed. Trade-off: ~free at rest instead of one VM 24/7. If cold starts prove too
  flaky, revert to `min_machines_running = 1` + `fly scale count 1` (always-warm),
  or move to Fly Machines API orchestration.
- **VM sizing.** A **dedicated** CPU (`performance-1x`, 2 GB) is required: on a
  shared vCPU, Chromium's cold launch overran the 30 s WS-endpoint timeout and
  every render fell back to a text embed. `CONCURRENT = 2` stays well clear of
  OOM at 2 GB; raise it only alongside VM memory.
- **Fonts.** For Latin names the bot inlines the card's font faces as data URIs, so
  the render needs no network and waits only on `load` + `document.fonts.ready`.
  Names in CJK scripts (or any inlining failure) keep the Google Fonts `<link>`,
  fetched at render time under `networkidle0`. Either way text never falls back
  mid-shot.
