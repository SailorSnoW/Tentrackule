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

- **Cost / cold start.** The machine suspends when idle (`auto_stop_machines =
  'suspend'`, `min_machines_running = 0`), so it's effectively free at rest and
  resumes in ~1–3 s on the next match — invisible for a background post. The
  30 s client timeout in `render_html_to_png` absorbs the cold start.
- **Memory.** `CONCURRENT = 2` on a 1 GB VM keeps Chromium well clear of OOM.
  Raise it only alongside VM memory.
- **Fonts.** The renderer fetches Google Fonts at render time and the card waits
  on `document.fonts.ready` before capture, so text never falls back mid-shot.
