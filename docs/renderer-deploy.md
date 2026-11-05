# Renderer setup (Cloudflare Browser Run)

The match card is HTML rendered to JPEG by
[Cloudflare Browser Run](https://developers.cloudflare.com/browser-run/) (formerly
Browser Rendering), so the bot VM stays lean at 256 MB and there is no renderer
to host.

```
 tentrackule (bot, 256 MB)  ──POST /screenshot { html }──▶  Cloudflare Browser Run
   build_html(ctx)            Authorization: Bearer …       managed headless Chromium
   Renderer::render()  ◀──────────── JPEG bytes ─────────   billed per browser second
```

## 1. Create the API token

1. Note your **account ID** (Cloudflare dashboard, account home).
2. Create a custom API token with only the **`Browser Run - Edit`** permission.

Workers Free is enough to start; see [Limits](#limits-and-failure-modes).

## 2. Check it from your machine

Add both values to `.env` (git-ignored):

```sh
CLOUDFLARE_ACCOUNT_ID=<id>
CLOUDFLARE_API_TOKEN=<token>
```

Then render real cards through the real code path:

```sh
cargo test renders_real_cards_on_cloudflare -- --ignored --nocapture
```

The test covers Latin, Latin Extended, kana, hangul, Han and Cyrillic names plus the
Google Fonts `<link>` fallback. Review the JPEGs in `target/scorecard-samples/`. On
Workers Free, expect pauses of up to about 10 s between cards: that is the rate limit
being honoured.

## 3. Point the bot at it

Set the secrets **before** deploying, because a push to `master` deploys automatically:

```sh
fly secrets set -a tentrackule \
  CLOUDFLARE_ACCOUNT_ID=<id> \
  CLOUDFLARE_API_TOKEN=<token>
```

At startup the bot logs `renderer=true`. If either value is missing it logs a warning
and posts a **text embed** for each match instead of a card.

## Limits and failure modes

| | Workers Free | Workers Paid ($5/month minimum) |
|---|---|---|
| Browser time | 10 minutes/day, resets at 00:00 UTC | 10 hours/month, then $0.09/hour |
| Requests | 1 every 10 seconds | 30 per second |

A card measured 325–1058 ms of billed browser time, so Workers Free covers about 560
cards a day at the slowest measured time. The hourly `📊 hourly stats` log line
reports `render_browser_ms` so you can follow consumption.

| Response | Behaviour |
|---|---|
| `429` rate limit | Waits for `Retry-After` (10 s if absent), up to 30 s |
| `429` daily quota, or a `Retry-After` above 30 s | No retry: text embed until the quota resets |
| `422`, `5xx`, network error, non-image `200` | Retries after 5 s |
| `400`, `401`, `403`, `413` | No retry: text embed. Check the token and the logs |

A render gets at most 3 attempts. Whatever happens, the match is still announced.

## Notes

- **Fonts.** The bot inlines the Latin faces as data URIs, so the render needs no
  network and waits only on `load`, then on the `html[data-fonts-ready]` marker set
  after `document.fonts.ready`. CJK names use the browser's pre-installed Noto CJK.
  If inlining fails, the HTML keeps a Google Fonts `<link>` and waits on
  `networkidle0`, which costs at least 500 ms more browser time.
- **Billing header.** Each response carries `X-Browser-Ms-Used`. Failed
  `waitForTimeout` requests are not charged.
- **Data.** The card HTML, including Riot IDs, is sent to Cloudflare. Cloudflare's
  Quick Actions response cache does not apply to submitted HTML.
- **Local development.** Leave the variables unset to get the text embed, or open
  `target/scorecard-samples/*.html` (written by `cargo test`) in a browser.

## Retiring the old Fly renderer

Once cards are confirmed on Cloudflare, stop the old Browserless app and keep it for a
few days as a rollback:

```sh
fly machine list -a tentrackule-renderer
fly machine stop -a tentrackule-renderer <machine-id>
```

A stopped machine only bills its root filesystem. To finish:

```sh
fly apps destroy tentrackule-renderer
fly secrets unset -a tentrackule RENDERER_TOKEN
```

To roll back before that, revert the migration commit and restart the machine.
