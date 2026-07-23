# Roadmap — Optimisation globale (mémoire, API Riot, cache, infra)

> Statut : **en cours** (Phases 1–2 + 3.A/3.B/3.C + **4** + **5** implémentées — clippy/tests verts ; JPEG à valider visuellement via `/dev_test_alert`, cold starts scale-to-zero à observer après `fly deploy -c fly.renderer.toml`, cadence adaptative + baisse de volume à confirmer en prod via les logs ; auto-résolution DDragon + refresh 24 h à observer sur un vrai changement de patch) · issu de l'audit d'architecture du 2026-07-23.
> Périmètre : consommation de ressources, volume d'appels API, stratégie de cache, coût infra. Aucun changement fonctionnel visible côté Discord (hors amélioration de justesse en Phase 1 et latence de détection accrue pour les joueurs inactifs en Phase 4, voulue).
> Dernière mise à jour : 2026-07-24.

---

## 0. Contexte & constats

**Existant.** Bot Rust (poise/serenity) sur Fly.io — VM **256 Mo shared** (~2 $/mois) + renderer Browserless **performance-1x 2 Go always-on** (~30 $/mois). Poller séquentiel toutes les 60 s : 1 appel `match-ids` par joueur par cycle, puis `match` + `league` (+ `mastery` hors ranked) quand un nouveau match apparaît. Assets DDragon inlinés en data URI via `ImageCache` (mémoire + disque, 100 Mo, TTL 7 j).

**Les trois constats qui motivent cette roadmap :**

1. **Risque OOM réel sur la VM 256 Mo** : `ImageCache` garde chaque asset en RAM sous forme de **data-URI base64** (×1,33), borné par un plafond pensé pour le disque (100 Mo → ~133 Mo en RAM), et **précharge tout le cache disque au boot** (`load_from_disk`).
2. **Le renderer est ~90 % du coût infra** pour quelques rendus par jour. Il est always-on uniquement parce qu'un échec de rendu **perd la carte définitivement** (fallback texte + `last_match_id` avancé) — il n'y a pas de retry.
3. **Volume API fixe** : 1 440 × N appels/jour quelle que soit l'activité des joueurs. Une clé Riot personnelle (100 req/2 min) plafonne à ~40 joueurs à cette cadence.

Légende effort : 🟢 trivial (< 1 h) · 🟡 contenu (½–1 j) · 🔴 chantier (> 1 j).

---

## 1. Phase 1 — Quick wins (une PR, risque quasi nul)

Tout est indépendant, livrable en un seul lot.

- [x] 🟢 **gzip sur reqwest** — `Cargo.toml` : ajouter la feature `gzip`. Les payloads match-v5 (~100–300 Ko de JSON) transitent aujourd'hui non compressés. Gain : −80/90 % de bande passante sur chaque nouveau match.
- [x] 🟢 **WAL explicite** — `main.rs` : `SqliteConnectOptions::journal_mode(Wal)`. Vérifié dans les sources sqlx 0.8.6 : `journal_mode` n'est **pas** fixé par défaut (contrairement à `foreign_keys=ON` et `busy_timeout=5s`). En mode rollback, une écriture du poller bloque brièvement les lectures des commandes slash.
- [x] 🟢 **`MissedTickBehavior::Delay`** — `start_polling` (`src/poller/match_poller.rs`) : le comportement Burst par défaut fait rafaler les ticks en retard si un cycle dépasse l'intervalle.
- [x] 🟢 **Skipper league-v4 hors ranked** — `check_player_match` : `fetch_rank_info` est appelé pour *chaque* nouveau match, ARAM/normales comprises, alors que la carte non-classée affiche la maîtrise et pas le rang. Ne fetcher (et ne persister via `update_player_rank`) que pour les queues 420/440. Gain : −1 appel API par partie non classée, zéro changement visible.
- [x] 🟢 **`count=5` + rattrapage ordonné** — `check_player_match` : aujourd'hui `get_match_ids(count=1)` n'annonce que la dernière partie ; après un downtime, les parties intermédiaires sont silencieusement perdues et la streak est faussée. Récupérer 5 IDs (même coût : 1 requête), traiter ceux plus récents que `last_match_id` du plus ancien au plus récent. Amélioration de justesse gratuite.
- [x] 🟢 **Index `guild_players(player_id)`** — `migrations.rs` : `get_guilds_tracking_player` filtre par `player_id`, que le PK `(guild_id, player_id)` ne sert pas.

**Critère de sortie :** comportement identique à l'existant (hors rattrapage post-downtime, voulu), clippy/tests verts.

---

## 2. Phase 2 — Cache d'images : plafond mémoire

Objectif : éliminer le chemin d'OOM. Le disque reste la vraie couche de cache ; la mémoire devient un petit accélérateur borné. Tout se joue dans `src/discord/image_gen.rs` (`ImageCache`).

- [x] 🟡 **Supprimer le préchargement au boot** — `load_from_disk` lit *tout* le cache disque en RAM au démarrage. Le chemin « disk hit → promotion en mémoire » existe déjà dans `get_or_fetch` : passer en chargement 100 % lazy (ne garder au boot que la purge des fichiers expirés). *→ `purge_expired` ne fait plus que balayer les fichiers TTL-expirés ; promotion lazy conservée.*
- [x] 🟡 **Borner la couche mémoire séparément** — plafond dédié **16–32 Mo** avec éviction LRU, indépendant du plafond disque (100 Mo). Aujourd'hui l'unique plafond borne les octets *bruts* alors que la map stocke du base64 (×1,33). *→ `MEMORY_MAX_SIZE_MB = 24` (LRU par tick monotone) vs `DISK_MAX_SIZE_MB = 100` (éviction par mtime, scan disque).*
- [x] 🟢 **Stocker les octets bruts en mémoire** — encoder en base64 à la demande au moment du rendu (−25 % de RAM sur la couche mémoire). Optionnel si le plafond 16–32 Mo est en place. *→ `MemoryEntry.bytes: Arc<[u8]>`, `to_data_uri` appelé à chaque hit.*
- [x] 🟢 **Hash de clé 128 bits** — la clé de cache actuelle est un hash maison 64 bits (multiplication par 31) : une collision servirait silencieusement la mauvaise image. Remplacer par un hash large (ex. SHA-256 tronqué) ; invalide le cache existant une fois (acceptable, il se reconstruit). *→ SHA-256 tronqué à 128 bits (`sha2`), 32 hex chars.*
- [x] 🟢 **Fetch concurrent dans `inline_assets`** — les ~15 assets d'une carte à froid sont fetchés séquentiellement ; `join_all` borné (~4) pour les rendus à froid. *→ `buffer_unordered(4)` (`futures`) après dédup des URLs.*

**Critère de sortie :** RSS du bot stable (< ~120 Mo) avec un cache disque plein à 100 Mo ; aucun re-téléchargement d'assets déjà sur disque.

---

## 3. Phase 3 — Renderer : coût & latence

Objectif : sortir du « performance-1x 2 Go allumé 24/7 » (~25–30 $/mois économisables) et accélérer chaque rendu. L'always-on actuel ne compense qu'une seule chose : l'absence de retry.

### 3.A — Retry de rendu (prérequis de tout le reste)

- [x] 🟡 **2 tentatives espacées de 20–30 s** dans le chemin de rendu (`render_html_to_png` / `generate_match_image`). Aujourd'hui : échec → fallback texte → `last_match_id` avancé → **la carte est perdue à jamais**. L'annonce d'un match qui vient de finir tolère très bien +30 s. Le fallback texte reste le filet final. *→ `render_with_retry` : `RENDER_ATTEMPTS = 2`, `RENDER_RETRY_DELAY = 25 s`. Court-circuité si le renderer n'est pas configuré (erreur permanente, pas de sleep). Le fallback texte reste géré côté poller.*

### 3.B — Retour au scale-to-zero

- [x] 🟡 **`min_machines_running = 0`** dans `fly.renderer.toml` une fois 3.A livré : la 1ʳᵉ requête réveille la machine (échec toléré si Chrome dépasse le timeout de lancement Browserless de 30 s), la 2ᵉ passe sur un Chrome chaud (1–6 s). Garder `performance-1x` : on ne le paie plus que quelques minutes par jour. *→ `min_machines_running = 0` (commentaires `fly.renderer.toml` + `renderer-deploy.md` réécrits). Flippé **avant** la mesure Phase 6 (choix assumé, gain immédiat ~25–30 $/mois) — nécessite `fly deploy -c fly.renderer.toml` et une fenêtre d'observation des cold starts. Réversible : `min_machines_running = 1` + `fly scale count 1`.*
- [ ] 🔴 *(fallback si 3.B reste trop aléatoire)* **Orchestration Fly Machines API** : le bot démarre la machine dès qu'un nouveau match est détecté (avant build HTML + inline des assets, ce qui donne à Chrome son temps de warmup) et l'arrête après ~10 min d'idle. Nécessite `FLY_API_TOKEN` + machine ID en secrets.
- [ ] 🔴 *(alternative exploratoire)* **Chrome persistant sur shared-cpu 1 Go** (~6 $/mois) : le burst CPU est au *lancement* de Chrome, pas au screenshot d'une carte statique de 820 px. À valider selon les options de session/keep-alive du build Browserless utilisé.

### 3.C — Rendu plus rapide et déterministe

- [x] 🟡 **Inliner les polices en data URI** via l'`ImageCache` (comme les images) au lieu du `<link>` Google Fonts fetché à chaque rendu dans un profil Chromium vierge. Permet `waitUntil:"load"` au lieu de `networkidle0` + `fonts.ready` → ~300–500 ms de moins par rendu, zéro dépendance réseau au moment du screenshot. *Nuance héritée de la roadmap scorecard : les subsets CJK (Noto Sans KR/SC) sont lourds — inline systématique du latin, CJK inliné à la demande (le cache disque absorbe le one-time).* *→ Faces latin/latin-ext de Cinzel+Barlow inlinées **une fois** (invariantes, ~307 Ko réutilisés sur chaque carte), parsées depuis le CSS Google Fonts réel (`font_face_segments`/`extract_css_urls`, test réseau opt-in). `build_html` émet un `<style>` auto-porté → `waitUntil:"load"`. Les noms CJK (`needs_cjk_fonts`) et tout échec d'inline retombent sur le `<link>` + `networkidle0` (comportement inchangé). CJK **pas** inliné (subsets numérotés) : la voie `<link>` reste correcte pour ces noms.*
- [x] 🟢 **Screenshot JPEG (q≈90) ou WebP** au lieu de PNG — champ `quality` à ajouter à `ScreenshotOptions`. La carte est dominée par le splash photographique : 3–6× plus léger, upload Discord plus rapide. **À valider visuellement sur le texte fin** (snapshots + `dev_test_alert`). *→ `SCREENSHOT_TYPE = "jpeg"` + `SCREENSHOT_QUALITY = Some(90)` (un seul point de bascule, `quality` omis en PNG). Pièce jointe `match_result.jpg` (`ImageGenerator::ATTACHMENT_FILENAME`). ⚠️ Validation visuelle du texte fin encore à faire via `/dev_test_alert`.*

**Critère de sortie :** facture renderer ≈ 0 au repos ; aucun match sans carte sur un mois de fonctionnement normal (le retry absorbe les cold starts) ; rendu p50 mesurablement plus court.

---

## 4. Phase 4 — API Riot : scaling & résilience

Objectif : décorréler le volume d'appels du nombre de joueurs suivis, et rendre le client auto-régulé face aux limites Riot.

- [x] 🔴 **Polling adaptatif par activité** — le levier principal (volume ÷ 3–10). Cadence dérivée de l'ancienneté de la dernière partie vue : partie < 2 h → 60 s · sinon → 3–5 min · inactif > 7 j → 15 min. L'ancienneté peut se dériver de `match_results.created_at` (aucun appel supplémentaire). Toute nouvelle partie détectée ramène immédiatement le joueur à la cadence rapide. *→ `cadence_for`/`poll_due` (`match_poller.rs`) : tiers `ACTIVE_WINDOW = 2 h` → tick (60 s), `< DORMANT_WINDOW = 7 j` → `IDLE_CADENCE = 5 min`, sinon `DORMANT_CADENCE = 15 min` (le tick sert de plancher). `PollSchedule` (map en mémoire `player_id → Instant`, éphémère : re-sync complet au boot) sélectionne les joueurs dus par tick. Ancienneté = `get_tracked_players_with_activity` → `COALESCE(MAX(match_results.created_at), players.created_at)` (aucun appel Riot ; un joueur fraîchement suivi lit « actif » via son `created_at`). Une nouvelle partie insère une ligne `match_results` (created_at ≈ now) → retour tier actif au cycle suivant, sans reset explicite.*
- [x] 🟡 **Honorer `Retry-After` sur 429** — `client.rs` logge le 429 mais ne lit pas le header et ne retente pas. Retry borné (1–2 tentatives) après le délai indiqué ; idem retry simple sur 5xx transitoires. *→ `RiotClient::get` est une boucle de retry (`MAX_ATTEMPTS = 3` = 1 + 2 retries). 429 : `retry_after` lit le header (secondes), plafonné à `MAX_RETRY_DELAY = 120 s`, défaut `DEFAULT_RETRY_DELAY = 2 s`. 5xx : `SERVER_ERROR_RETRY_DELAY = 1 s`. 404/403/autres 4xx restent terminaux au 1ᵉʳ coup. Les deux limiters sont re-franchis avant chaque tentative.*
- [x] 🟡 **Deuxième fenêtre de rate limit (100 req / 2 min)** — le governor n'applique que 20/s alors que les clés personnelles ont aussi une fenêtre 2 min. Second `Quota` combiné (ou, plus ambitieux, pilotage par les headers `X-App-Rate-Limit-Count`). *→ `limiter_per_window` = `Quota::with_period(120 s / N).allow_burst(N)` (N = `RIOT_RATE_LIMIT_PER_TWO_MIN`, défaut 100) en plus de `limiter_per_second`. `get` franchit les deux (fenêtre lente d'abord). Config `riot_rate_limit_per_two_min` + `.env.example`/README.*
- [x] 🟡 **Boucle joueurs concurrente** — `poll_players` est séquentiel ; `buffer_unordered(3–5)` raccourcit les cycles quand N grandit. Le governor borne déjà le débit sortant, la concurrence ne fait que masquer la latence réseau. *→ `for_each_concurrent(POLL_CONCURRENCY = 5)` sur les joueurs dus (`futures::stream`). Erreurs par joueur contenues (loggées, jamais propagées) ; le rate limiter borne le débit réel.*

**Critère de sortie :** volume d'appels/jour publié dans les logs (cf. Phase 6) en baisse nette à joueurs constants ; zéro erreur 429 non gérée sur un mois. *(Mesure fine en attente de la ligne de stats Phase 6 ; le retry 429 borné couvre déjà le second critère.)*

---

## 5. Phase 5 — DDragon : zéro maintenance de patch

Objectif : supprimer le redeploy manuel à chaque patch LoL et les 404 en cours de run.

- [x] 🟡 **Résoudre la version au boot** via `https://ddragon.leagueoflegends.com/api/versions.json` (fallback : `DDRAGON_VERSION` env, qui devient un override de secours). *→ `ddragon::resolve_version` : un `DDRAGON_VERSION` non vide force le patch (override d'urgence, court-circuite le réseau) ; sinon `fetch_latest_version` prend la 1ʳᵉ entrée de `versions.json` ; en dernier recours `DEFAULT_DDRAGON_VERSION` compilé en dur. `Config.ddragon_version` → `ddragon_version_override: Option<String>` ; `DDRAGON_VERSION` retiré de `fly.toml`/`.env.example` pour que l'auto-résolution soit le chemin vivant.*
- [x] 🟡 **Refresh périodique (24 h) des tables `DdragonData`** — chargées une seule fois au boot aujourd'hui : un champion sorti en cours de run 404 sur tous ses assets/mastery. Nécessite de rendre les tables remplaçables à chaud (`ArcSwap` ou `RwLock`). *→ `ImageGenerator.ddragon: ArcSwap<DdragonData>` (lecture lock-free côté rendu/mastery). `run_ddragon_refresh` (tick 24 h, `Delay`, 1ᵉʳ tick immédiat consommé) → `refresh_ddragon` : re-résout le patch vivant et ne recharge/hot-swap **que si** le patch a changé. Override épinglé = no-op ; échec de lookup = on garde les tables courantes (jamais de downgrade sur un blip). Spawn dans `main.rs`.*
- [x] 🟢 **TTL par classe d'URL dans `ImageCache`** — les URLs DDragon **versionnées** (`/16.1.1/img/…`) sont immuables : pas de TTL (éviction par taille uniquement) au lieu de re-télécharger tout le set chaque semaine. Garder un TTL court uniquement pour CommunityDragon `latest`. *→ `ImageCache::ttl_for` : tout le CDN DDragon (`ddragon.leagueoflegends.com/cdn` — versionné **et** versionless splash/runes, y compris le gros splash) → `None` (immuable, éviction par taille seule) ; le reste (CommunityDragon `latest`, pointeur roulant) → `MUTABLE_TTL = 24 h`. `is_fresh(created_at, Option<Duration>)` remplace `expired`. `purge_expired` au boot supprimé : la fraîcheur des mutables est gérée à la lecture (re-fetch passé le TTL), le disque reste borné par le plafond de taille. Le cache vit sur le volume Fly `/data` (persiste entre deploys), donc le gain est réel.*

**Critère de sortie :** un patch LoL passe sans aucune intervention ; plus aucun re-téléchargement hebdomadaire d'assets versionnés. *(Rempli : auto-résolution + refresh 24 h ; le splash et tous les assets DDragon ne réexpirent plus par le temps.)*

---

## 6. Phase 6 — Polish (opportuniste, au fil de l'eau)

- [ ] 🟢 **Ligne de stats périodique** (1×/h dans les logs : appels API par cycle, hit-rate cache, latence rendu p50/p95, RSS). *À faire idéalement avant les Phases 3.B et 4 pour mesurer leurs gains.*
- [ ] 🟡 **Réutilisation de l'attachment multi-guilde** — le même PNG est ré-uploadé pour chaque guilde qui suit le joueur : poster une fois, puis référencer l'URL CDN du premier message dans les suivants.
- [ ] 🟢 **Arrêt gracieux** — SIGTERM (deploy Fly) : finir le check joueur en cours, fermer la pool proprement.
- [ ] 🟢 **Config stricte** — les valeurs d'env invalides retombent silencieusement sur les défauts (`parse().ok()`) : logger un warning explicite.

---

## 7. Hors scope (arbitré)

- **Rendu local (retour resvg/SVG)** : décision HTML/Chromium actée dans [`html-scorecard-roadmap.md`](html-scorecard-roadmap.md), on ne revient pas dessus.
- **ETag/If-None-Match sur l'API Riot** : non supporté côté Riot.
- **Sharding du rate limiter par région** : inutile tant que le volume reste sous les fenêtres d'une seule clé ; à reconsidérer seulement si le bot devient multi-communautés.

## 8. Ordre recommandé

```
Phase 1 (quick wins)  ──►  Phase 2 (mémoire)  ──►  Phase 3.A+3.C (retry + fonts/JPEG)
                                                        │
                                    Phase 6 (stats) ◄───┤  (mesurer avant/après)
                                                        ▼
                                                   Phase 3.B (scale-to-zero)
                                                        ▼
                                    Phase 4 (API scaling) · Phase 5 (DDragon) — indépendantes
```

Les Phases 1–2 sont sans risque et traitent les deux urgences (OOM, gaspillage) ; la Phase 3 porte l'essentiel du gain financier ; les Phases 4–5 préparent la croissance et suppriment la maintenance récurrente.
