# Roadmap — Nouveau template de carte de match (HTML → image)

> Statut : **implémenté (Phases 1–5)** · Le template SVG (`resvg`) est **retiré** ; les cartes sont désormais construites en **HTML moderne** (Rust) puis rendues par un **Chromium headless** (Browserless) hébergé à part. Reste opérationnel : le **déploiement Fly du renderer** (manuel, cf. [`renderer-deploy.md`](renderer-deploy.md)).
> Dernière mise à jour : 2026-07-23.

---

## 0. Contexte & décision

**Existant.** Bot Discord Rust (poise/serenity) qui poste le résultat des matchs LoL en image. Aujourd'hui : template **SVG** → substitution `{{var}}` + conditionnels mustache → rasterisation **100 % Rust** (`resvg`/`usvg`/`tiny-skia`) dans `src/discord/image_gen.rs`. Déployé sur **Fly.io, VM 256 Mo / 1 vCPU partagé**, image Docker = binaire statique lean.

**Besoin.** Nouveau template au format **HTML/CSS moderne** (grid, flex, gradients, web-fonts, plus de données). `resvg` ne sait pas rendre du HTML (ni `<foreignObject>`) → il faut un moteur navigateur.

**Décision (arbitrée).**
- CSS moderne **complet** requis → **Chromium** (pas de moteur pur Rust).
- Rendu déporté dans un **service séparé** → le bot reste lean à 256 Mo (Chromium **pas** embarqué).
- Cible : **Browserless auto-hébergé sur Fly** (app séparée, réseau privé `.internal`, **scale-to-zero**).

**POC (fait le 2026-07-22).** Le template a été résolu à la main avec ses données d'exemple et screenshoté en Chromium headless (Playwright) : rendu fidèle — grid/flex/`var()`/gradients/losanges `rotate()`, 3 polices Google (Cinzel, Barlow Semi Condensed, **Noto Sans JP** → noms CJK OK), tous les assets DDragon. → **Le mécanisme est validé.**

---

## 1. Architecture cible

```
                         (réseau privé Fly 6PN, jamais exposé)
┌─────────────────────────┐   HTTP POST /screenshot    ┌────────────────────────────┐
│  tentrackule (bot)      │   { html }  ───────────▶   │  tentrackule-renderer      │
│  VM 256 Mo, lean        │                            │  Browserless (Chromium)    │
│                         │   ◀───────────  PNG bytes  │  VM ~1 Go, scale-to-zero   │
│  • fetch + cache assets │                            └────────────────────────────┘
│  • build_html(ctx)      │
│  • POST au renderer     │   PNG ──▶ attachement Discord (inchangé)
└─────────────────────────┘
```

**Ce qui NE change pas** : `ImageCache` (fetch + cache disque/mémoire des assets DDragon), l'envoi Discord (attachement `Vec<u8>`), le polling.

**Ce qui change** dans `src/discord/image_gen.rs` :
- `build_svg()` → **`build_html()`** (émet le HTML final résolu).
- `render_svg_to_png()` → **`render_html_to_png()`** (POST HTTP au renderer au lieu de `resvg`).

**Le fichier `Scorecard LoL.dc.html` est un _spec_, pas un fichier à rendre.** C'est un export de composant d'un outil de design (`<x-dc>`, `<sc-if>`, `<sc-for>`, classe `DCLogic`, runtime `support.js` absent, `data-props` typés). On le **porte** :
- `themePresets()` → 3 maps de variables CSS (victory / defeat / remake) → constantes Rust.
- `renderVals()` → logique de valeurs dérivées → fonctions Rust.
- structure HTML + `sc-if`/`sc-for` → un builder Rust qui émet le markup final. Le CSS inline est **standard et réutilisable tel quel**.

**Assets vs polices.**
- **Images** (splash, sorts, runes, items, icônes) : on **inline en data URI** via l'`ImageCache` existant (renderer déterministe/offline, cache réutilisé, pas de dépendance DDragon au moment du rendu). `ImageCache::get_or_fetch` est déjà générique (prend une URL quelconque) → seuls de nouveaux builders d'URL à ajouter.
- **Polices** : **fetchées au rendu** par le renderer (`<link>` Google Fonts + attente `document.fonts.ready` avant screenshot). On évite d'embarquer le base64 de Noto Sans JP (CJK lourd).

---

## 2. Modèle de données (le contrat)

Forme cible du `match` (issue du `data-props` tsType + `defaultMatch()` du template), croisée avec les DTOs actuels (`ParticipantDto`, `InfoDto`, `LeagueEntryDto`).

Légende : 🟢 déjà dispo · 🟡 déjà dans un payload récupéré, à désérialiser/calculer · 🔴 vrai neuf (API/état).

| Champ template | Statut | Source Riot / calcul |
|---|---|---|
| `player.name` / `player.tag` | 🟢 | `Player.game_name` / `tag_line` |
| `champion.key` (asset) | 🟢 | `participant.championName` (forme clé DDragon) |
| `champion.name` (affichage) | 🟢 | `championName` (MVP) · 🟡 nom localisé via DDragon `champion.json` |
| `champion.level` | 🟡 | `participant.champLevel` |
| `role` / `roleUpper` | 🟢 | `position_display()` |
| `duration` | 🟢 | `duration_formatted()` |
| `patch` | 🟢 | `patch_version()` |
| `queueLabel` | 🟢 | `queue_name()` |
| `kda.{k,d,a}` / `kdaRatio` | 🟢 | `kills/deaths/assists` + `kda_ratio()` |
| `stats.cs.{v,sub}` | 🟢 | `cs_total()` + `cs_per_minute()` |
| `stats.gold.v` | 🟢 | `gold_formatted()` |
| `stats.gold.sub` (or/min) | 🟡 | `gold_earned / (duration/60)` |
| `stats.dmg.v` | 🟢 | `total_damage_dealt_to_champions` (format) |
| `stats.dmg.share` (% équipe) | 🟡 | `dmg / Σ dmg alliés` (via `teamId`) |
| `stats.vision.v` | 🟢 | `vision_score` |
| `stats.vision.sub` (posé·tué·contrôle) | 🟡 | `wardsPlaced` · `wardsKilled` · `detectorWardsPlaced` |
| `kp` / `kpSub` | 🟡 | `(kills+assists)/Σ kills alliés` · `"{k+a}/{teamKills}"` |
| `multi` / `multiSub` | 🟡 | `penta/quadra/triple/doubleKills` → libellé |
| `spells[]` | 🟡 | `summoner1Id`/`summoner2Id` (num) → clé DDragon (map 4=Flash, 11=Smite, …) |
| `runes.{keystone,secondary}` | 🟡 | `perks.styles[0].selections[0].perk` + `perks.styles[1].style` → chemins `perk-images` (map DDragon `runesReforged.json`) |
| `items[6]` / `trinket` | 🟢 | `item0..item5` / `item6` |
| `matchup.*` | 🟡 | adversaire = participant même `teamPosition`, `teamId` opposé ; rows DMG/CS/Or (`pct` = part du joueur) |
| `team.{kills,oppKills}` | 🟡 | Σ kills par `teamId` |
| `team.objectives[]` | 🟡 | **`info.teams[].objectives.{dragon,baron,tower}.kills`** → nouveau `TeamDto` (absent des DTOs) |
| `rank.{tier,rank,lp,delta}` | 🟢 | `RankInfo` + `calculate_lp_diff()` (existe déjà) |
| `rank.{wins,losses,wr}` | 🟡 | `LeagueEntryDto` : **ajouter `wins`/`losses`** ; `wr = wins/(wins+losses)` |
| `rank.progress` (barre) | 🟡 | `league_points` (0–100, hors apex) |
| `rank.streak[]` | ✅ | 5 derniers résultats → table DB `match_results` (`get_recent_results`) — *Phase 4* |
| `grade` (S+/A/…) | ✅ | formule role-relative maison (`grade.rs`), pas de champ Riot — *Phase 4* |
| `mastery.{level,points}` | ✅ | **champion-mastery-v4** (level, points) ; `games` non fourni par l'API → masqué — *Phase 4* |

**Bilan : ~80 % de la « plus de data » est déjà dans les réponses match-v5 / league-v4 déjà récupérées**, juste pas désérialisée. Seuls **grade / mastery / streak** demandent du neuf — et sont **tous optionnels** (gardés par `sc-if` dans le template).

---

## 3. Phases

### Phase 1 — Builder HTML en Rust *(pur Rust, testable sans infra)*
**But** : produire le HTML final résolu à partir d'un `MatchImageContext`, validable en local exactement comme la POC.

- [x] Nouveau module `src/discord/scorecard.rs` :
  - [x] Builder programmatique typé (choisi plutôt qu'un template à trous, vu la densité des styles inline).
  - [x] `struct ScorecardVm { … }` = port de la sortie de `renderVals()` (valeurs déjà formatées : `meta_line`, styles de barres, libellés d'objectifs, flags `has_*`, etc.).
  - [x] `MatchResult::theme_vars()` = port de `themePresets()` (victory/defeat/remake). *(méthode d'enum plutôt que fn libre — équivalent.)*
  - [x] `ScorecardVm::build_html(&self) -> String`.
  - [x] **Échappement HTML** maison de tous les champs dynamiques (`escape_html`, couvre `< > & " '`).
- [x] `ScorecardVm::from_context` branche la data déjà dispo (🟢) + 🟡 immédiats : or/min, dmg share, KP, **matchup** et kills d'équipe (dérivés en groupant les participants par `win`, avec garde-fou 5v5 → skip propre en remake). **grade / mastery / streak / combat / multi / sorts / runes / sub-vision / niveau champion / WR** restent éteints (leurs blocs sont omis, comme un `sc-if` non satisfait).
- [x] Tests `#[cfg(test)]` : échappement, omission des blocs absents, chip multi masquée en remake, dérivations `from_context` (5v5 + remake), et écriture des 3 cartes témoins dans `target/scorecard-samples/`.

**Definition of done** : ✅ `build_html` génère une carte **visuellement identique à la POC** pour victory/defeat/remake (validé par screenshot Chromium des cartes témoins), avec la data actuellement disponible, sans réseau ni Chromium au moment de la génération HTML.

---

### Phase 2 — Infra renderer + branchement bout-en-bout
**But** : le bot produit de vraies cartes en prod.

- [x] Nouvelle app Fly `tentrackule-renderer` :
  - [x] `fly.renderer.toml` : image `ghcr.io/browserless/chromium`, `internal_port = 3000`, `auto_stop_machines = 'suspend'`, `min_machines_running = 0`, VM `1024mb`, env `TOKEN`, `CONCURRENT` (+ `TIMEOUT`).
  - [ ] **Déploiement + vérif joignable** = étape manuelle (`fly`), documentée dans [`renderer-deploy.md`](renderer-deploy.md). ⚠️ Adresse = **`http://tentrackule-renderer.flycast:3000`** (et non `.internal` : le scale-to-zero ne se réveille qu'à travers le proxy Fly, que `.flycast` emprunte et `.internal` contourne).
- [x] Côté bot (`image_gen.rs`) :
  - [x] `render_html_to_png(&self, html) -> Result<Vec<u8>>` : POST `/screenshot?token=…`, body `{ html, selector:"#card-root", options:{ type:"png" }, viewport:{ width:820, deviceScaleFactor:2 }, gotoOptions:{ waitUntil:"networkidle0" }, waitForFunction:{ fn:"…document.fonts.ready…" } }`. Structs `Serialize` maison (via la feature `json` de `reqwest`, pas de dépendance `serde_json` directe).
  - [x] `generate_match_image` : `build_html` → `render_html_to_png`.
  - [x] Config : `RENDERER_URL`, `RENDERER_TOKEN` (env + `config.rs` + `.env.example` + `fly.toml`).
  - [x] **Timeout** (30 s, absorbe le cold start) + **fallback** si renderer KO/absent : le poller poste un **embed texte** (résultat, champion, KDA, file, rang) au lieu de la carte, et avance `last_match_id` — le post du match n'est jamais bloqué.

**Definition of done** : un vrai match tracké poste la nouvelle carte sur Discord, bot toujours en 256 Mo, renderer éteint au repos. → **Code bout-en-bout prêt ; reste le déploiement Fly manuel + vérif sur un vrai match.**

**Note d'implémentation** *(historique)*. En Phase 2, l'ancien pipeline SVG (`build_svg`, `render_svg_to_png`, `fontdb`) restait compilé mais **inutilisé** (`#![allow(dead_code)]` temporaire), les images étant des **URLs DDragon directes** (Chromium les fetch). La Phase 3 a ré-branché l'`ImageCache` (inline data-URI) ; la **Phase 5 a supprimé le pipeline SVG et ses dépendances** (`resvg`/`usvg`/`tiny-skia`, polices système, `assets/match_template.svg`), et levé l'`allow(dead_code)`.

---

### Phase 3 — Allumer la data (🟡, incrémental, un `sc-if` à la fois) ✅
Chaque item = petit, indépendant, livrable seul.

- [x] `ParticipantDto` : ajouter `champLevel`, `doubleKills`/`tripleKills`/`quadraKills`/`pentaKills`, `summoner1Id`/`summoner2Id`, `perks`, `wardsPlaced`/`wardsKilled`/`detectorWardsPlaced`, `teamId`. *(nouveaux champs `#[serde(default)]` + helpers `multi_kill`/`vision_breakdown`/`keystone_perk_id`/`secondary_style_id`.)*
- [x] `InfoDto` : ajouter `teams: Vec<TeamDto>` (+ `TeamDto { team_id, objectives }`, `ObjectivesDto`, `ObjectiveCountDto`) + `InfoDto::team(id)`. *(le `win` de `TeamDto` non retenu — inutilisé, le résultat vient de `participant.win`.)*
- [x] `LeagueEntryDto` : ajouter `wins`, `losses` → propagés dans `RankInfo` (`Option`, `None` quand reconstruit depuis la DB) par le poller.
- [x] Dérivations dans `scorecard.rs`/`riot` : KP, dmg share, or/min, vision sub, multi/multiSub, WR (`win_rate`), progress (`division_progress`, masquée hors-division apex).
- [x] **Matchup** : adversaire de lane (`teamPosition` égal, `teamId` opposé) + rows DMG/CS/Or.
- [x] **Team + objectifs** : Σ kills par `teamId` + drakes/nashor/tours depuis `teams[].objectives`.
- [x] Mappings DDragon : `summoner1Id`→clé sort, perks→chemins `perk-images`. Nouveau module `src/discord/ddragon.rs` (`DdragonData`) qui **charge `runesReforged.json` / `summoner.json` au boot** (best-effort, tables vides si échec).
- [x] Builders d'URL + cache : splash (`/cdn/img/champion/loading/{key}_0.jpg`, versionless), sorts, runes, icône adversaire — **inline data URI** via `ImageCache` ré-branché (`inline_assets`), avec sniff MIME PNG/JPEG (splash JPEG).

**Definition of done** : ✅ matchup + team + stats étendues affichés à partir de vraies données (validé par screenshot Chromium d'une carte `from_context`) ; ARAM (positions vides → matchup + role chip masqués) / normal gérés. **grade / mastery / streak** restent éteints (Phase 4).

---

### Phase 4 — Fonctionnalités neuves ✅
- [x] **Grade** : formule de score **role-relative** (nouveau module `src/discord/grade.rs`). KDA + KP + CS/min + part de dégâts + vision/min, chacun rapporté à une **baseline par rôle** puis pondéré (poids par rôle, somme = 1), avec un plafond anti-« une stat monstre » (`OVERPERFORM_CEIL`) → buckets S+/S/A/B/C/D. **Objectifs volontairement exclus** : match-v5 ne donne pas de *participation* aux objectifs par joueur (seulement le total d'équipe, déjà affiché), et le KP capture déjà le jeu d'équipe. Calculé dans `from_context` quand le contexte d'équipe existe — donc **éteint en remake / non-5v5**, exactement comme KP et part de dégâts.
- [x] **Mastery** : endpoint **champion-mastery-v4** (`RiotClient::get_champion_mastery`, `src/riot/endpoints/mastery.rs`) + map `championName`→`championId` chargée au boot depuis `champion.json` (`DdragonData::champion_id`). Récupérée par le poller **uniquement en file non-classée** (là où la carte montre la maîtrise à la place du rang), best-effort (404 « jamais joué » / champion inconnu → bloc omis). `games` non exposé par l'API → **masqué**.
- [x] **Streak** : nouvelle table `match_results` (résultat stocké à chaque match **décisif** traité — remakes ignorés — insert idempotent sur `(player_id, match_id)`) → `Repository::get_recent_results` lit les 5 derniers **de la file**. **Zéro appel API supplémentaire.** Affichée dans le bloc rang (classé), la plus récente à droite (inclut la partie courante).

**Definition of done** : ✅ grade / mastery / streak alimentés depuis de vraies données et validés par **screenshot Chromium** — carte classée : badge **S** + barre de streak à côté du rang ; carte non-classée : badge **S+** + bloc **Maîtrise** à la place du rang. Couvert par tests unitaires (`grade.rs`, dérivation `from_context` classée/non-classée, mapping `champion_id`).

---

### Phase 5 — Polish & nettoyage ✅
- [x] Variantes de queue (§4) : ranked→bloc rang, normal/ARAM→bloc maîtrise, matchup masqué en ARAM. *(Déjà porté par les dérivations Phases 3–4 ; verrouillé par un test `from_context` ARAM — positions vides → matchup + role chip omis, bloc maîtrise affiché — et vérifié au screenshot Chromium.)*
- [x] Retirer l'ancien pipeline : `assets/match_template.svg` supprimé, deps `resvg`/`usvg`/`tiny-skia` retirées de `Cargo.toml`, chargement `fontdb`/system fonts supprimé d'`image_gen.rs`, paquets `fonts-*` retirés du **Dockerfile du bot**. `image_gen.rs` ne garde que l'`ImageCache` + le POST au renderer (plus d'`#![allow(dead_code)]`).
- [x] **Tests** : snapshot HTML par thème (goldens sous `src/discord/snapshots/`, régénérables via `UPDATE_SNAPSHOTS=1 cargo test`) ; dérivations (KP, share, WR, matchup, multi libellés) + échappement HTML déjà couverts.
- [x] **Robustesse** : polices CJK — Noto Sans **JP** toujours chargé, **KR/SC** ajoutés au `<link>` *à la demande* selon le script détecté dans le pseudo (hangul → KR, idéogrammes Han → SC) ; **fallback icône manquante** via un handler `error` global (phase de capture) qui masque toute `<img>` en échec au lieu d'un glyphe cassé (vérifié : un 404 → `visibility:hidden`).
- [x] **Docs** : `README.md` (créé) + ce fichier.
- [~] *(Option)* cache du PNG **final** par `matchId+puuid` — **non retenu** : l'image est déjà générée **une seule fois** par match détecté (le poller réutilise les mêmes octets pour toutes les guildes), donc le risque de re-render est marginal et ne justifie pas un cache disque de PNG lourds.

---

## 4. Décisions & valeurs par défaut

| Sujet | Défaut retenu | Note |
|---|---|---|
| Moteur de rendu | Browserless (Chromium) auto-hébergé, app Fly séparée | scale-to-zero, réseau privé |
| Embarquer Chromium dans le bot | **Non** | protège les 256 Mo |
| Images (assets) | **Inline data URI** via `ImageCache` | déterministe, cache réutilisé |
| Polices | **Fetch Google Fonts au rendu** + `fonts.ready` | Noto Sans JP CJK trop lourd à embarquer ; licences SIL OFL OK |
| Dimensions | largeur **820 px**, `deviceScaleFactor: 2` | net « retina », Discord affiche bien |
| Variantes queue | ranked→rang · normal/ARAM→maîtrise · matchup si adversaire de lane | déjà porté par les `sc-if` du template |
| Échappement | **HTML-escape systématique** des champs dynamiques | bug garanti sinon (noms Riot) |
| `serde_json` | struct `Serialize` maison | évite une dépendance directe |
| Grade | **à concevoir** (formule) ou masqué au départ | pas de champ Riot |
| Sécurité renderer | réseau privé + `TOKEN` + timeout + cap page | HTML généré par le bot (pas d'entrée arbitraire) |

---

## 5. Risques & mitigations

| Risque | Impact | Mitigation |
|---|---|---|
| Cold start renderer (scale-to-zero) | latence ~1–3 s au réveil | invisible pour un post en tâche de fond ; `suspend` accélère |
| OOM si Chromium mal dimensionné | crash renderer | VM 1 Go, `CONCURRENT` bas, renderer isolé du bot |
| Coût VM renderer | € | éteint 99 % du temps + cache assets → quasi nul |
| `championName` ≠ clé DDragon (Kha'Zix/Wukong) | asset 404 / nom moche | `championName` = déjà la clé pour les URLs ; nom d'affichage via `champion.json` en Phase 3 |
| Course au chargement des polices | texte en fallback sur le screenshot | attendre `document.fonts.ready` avant capture |
| Injection/casse HTML via nom Riot | layout cassé / markup injecté | échappement HTML (Phase 1) |
| Rate limit Riot (mastery/streak) | throttle | mastery = 1 appel caché ; streak = état local, 0 appel |
| Renderer indisponible | pas d'image | fallback embed/skip, le match se poste quand même |

---

## 6. Checklist condensée

**Phase 1 — Builder** ✅ · `scorecard.rs`, `theme_vars`, `ScorecardVm`, `build_html`, échappement, test visuel.
**Phase 2 — Infra** ✅ *(code)* · `fly.renderer.toml`, `render_html_to_png`, config env, timeout + fallback embed, `generate_match_image` bout-en-bout. **Reste : déploiement Fly manuel** (`renderer-deploy.md`).
**Phase 3 — Data** ✅ · DTOs étendus (`TeamDto`, perks, spells, wards, league W/L), dérivations, matchup, objectifs, mappings DDragon (`ddragon.rs`), assets inline data-URI. *(grade/mastery/streak → Phase 4.)*
**Phase 4 — Neuf** ✅ · grade (`grade.rs`, role-relative, objectifs exclus), mastery (champion-mastery-v4 + map `champion.json`, non-classé), streak (table `match_results`, 0 appel API). Validé par screenshot Chromium.
**Phase 5 — Polish** ✅ · pipeline SVG supprimé (`resvg`/`usvg`/`tiny-skia`, `fontdb`, `match_template.svg`, fonts Dockerfile) ; variantes queue verrouillées (test ARAM) ; robustesse CJK (Noto KR/SC à la demande) + fallback icône manquante (`onerror`) ; snapshot HTML + tests dérivations ; `README.md`. *(Cache PNG final : non retenu — image générée une seule fois par match.)* **Reste : déploiement Fly du renderer.**

**Ordre recommandé : 1 → 2 → 3 → (4 au fil de l'eau) → 5.** La Phase 1 est le plus fort levier et se valide sans infra. **Toutes les phases de code sont faites ; seul le déploiement Fly du renderer reste manuel.**
