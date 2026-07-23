//! Phase 3 of the HTML scorecard roadmap: boot-time Data Dragon lookup tables.
//!
//! The match card needs to turn Riot's *numeric* summoner-spell keys and rune
//! ids into DDragon asset paths (e.g. `4` → `SummonerFlash`, keystone `8010` →
//! `perk-images/Styles/.../Conqueror.png`). Those mappings only live in DDragon's
//! `summoner.json` / `runesReforged.json`, so we fetch and index them once at
//! start-up (alongside the configured `DDRAGON_VERSION`) and keep them in memory.
//!
//! Fetching is best-effort: on any failure we log and fall back to empty tables,
//! which just omits the spell/rune icons (an unmet `sc-if`) rather than breaking
//! the whole card.

use std::collections::HashMap;

use reqwest::Client;
use serde::Deserialize;
use tracing::{info, warn};

/// Data Dragon CDN root (no trailing slash).
pub const DDRAGON_CDN: &str = "https://ddragon.leagueoflegends.com/cdn";

/// In-memory Data Dragon lookup tables + the CDN version they were built for.
#[derive(Debug, Clone, Default)]
pub struct DdragonData {
    version: String,
    /// Summoner-spell numeric key → DDragon id (e.g. `4` → `SummonerFlash`).
    spells: HashMap<i32, String>,
    /// Rune id → icon path (e.g. `8010` → `perk-images/Styles/.../Conqueror.png`).
    rune_icons: HashMap<i32, String>,
    /// Rune *style* id → icon path (e.g. `8000` → `perk-images/Styles/7201_Precision.png`).
    style_icons: HashMap<i32, String>,
    /// Champion DDragon key → numeric championId (e.g. `Ahri` → `103`), for the
    /// champion-mastery-v4 lookup.
    champion_ids: HashMap<String, i64>,
}

impl DdragonData {
    /// Fetch and index `summoner.json` + `runesReforged.json` for `version`.
    /// Never fails: missing tables just disable the corresponding icons.
    pub async fn load(http: &Client, version: &str) -> Self {
        let spells = match fetch_spells(http, version).await {
            Ok(map) => map,
            Err(e) => {
                warn!(error = %e, "🖼️ ⚠️ Failed to load summoner spells from DDragon");
                HashMap::new()
            }
        };

        let (rune_icons, style_icons) = match fetch_runes(http, version).await {
            Ok(maps) => maps,
            Err(e) => {
                warn!(error = %e, "🖼️ ⚠️ Failed to load rune index from DDragon");
                (HashMap::new(), HashMap::new())
            }
        };

        let champion_ids = match fetch_champions(http, version).await {
            Ok(map) => map,
            Err(e) => {
                warn!(error = %e, "🖼️ ⚠️ Failed to load champion index from DDragon");
                HashMap::new()
            }
        };

        info!(
            spells = spells.len(),
            runes = rune_icons.len(),
            styles = style_icons.len(),
            champions = champion_ids.len(),
            "🖼️ Data Dragon lookup tables loaded"
        );

        Self {
            version: version.to_string(),
            spells,
            rune_icons,
            style_icons,
            champion_ids,
        }
    }

    /// The CDN version these tables were built for (also used for versioned URLs).
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Versioned summoner-spell icon URL for a numeric key (`4` → Flash), if known.
    pub fn spell_src(&self, key: i32) -> Option<String> {
        let id = self.spells.get(&key)?;
        Some(format!("{DDRAGON_CDN}/{}/img/spell/{id}.png", self.version))
    }

    /// Versionless keystone icon URL for a rune id, if known.
    pub fn keystone_src(&self, perk_id: i32) -> Option<String> {
        let icon = self.rune_icons.get(&perk_id)?;
        Some(format!("{DDRAGON_CDN}/img/{icon}"))
    }

    /// Versionless secondary-tree icon URL for a rune style id, if known.
    pub fn secondary_src(&self, style_id: i32) -> Option<String> {
        let icon = self.style_icons.get(&style_id)?;
        Some(format!("{DDRAGON_CDN}/img/{icon}"))
    }

    /// Numeric championId for a DDragon champion key (`Ahri` → `103`), if known.
    /// Used to query champion-mastery-v4. Resolved through [`Self::champion_key`]
    /// so match-v5 casing quirks (e.g. `FiddleSticks`) still find their id.
    pub fn champion_id(&self, champion_name: &str) -> Option<i64> {
        self.champion_ids
            .get(&self.champion_key(champion_name))
            .copied()
    }

    /// Canonical Data Dragon champion key for a match-v5 `championName`.
    ///
    /// match-v5 almost always returns the exact DDragon key, but some diverge —
    /// famously `FiddleSticks`, where Data Dragon uses `Fiddlesticks`
    /// (dev-relations #580) — which 404s every splash / square / mastery lookup
    /// built from the raw name. We reconcile case-insensitively against the loaded
    /// champion index, falling back to the input untouched when the index is empty
    /// (best-effort load failed) or the champion is genuinely unknown.
    pub fn champion_key(&self, champion_name: &str) -> String {
        if self.champion_ids.contains_key(champion_name) {
            return champion_name.to_string();
        }
        self.champion_ids
            .keys()
            .find(|k| k.eq_ignore_ascii_case(champion_name))
            .cloned()
            .unwrap_or_else(|| champion_name.to_string())
    }
}

// ============================================================================
// summoner.json
// ============================================================================

#[derive(Deserialize)]
struct SummonerFile {
    data: HashMap<String, SummonerSpell>,
}

#[derive(Deserialize)]
struct SummonerSpell {
    /// Numeric key as a string (e.g. `"4"`).
    key: String,
    /// DDragon id (e.g. `"SummonerFlash"`).
    id: String,
}

async fn fetch_spells(
    http: &Client,
    version: &str,
) -> Result<HashMap<i32, String>, reqwest::Error> {
    let url = format!("{DDRAGON_CDN}/{version}/data/en_US/summoner.json");
    let file: SummonerFile = http
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(file
        .data
        .into_values()
        .filter_map(|s| s.key.parse::<i32>().ok().map(|k| (k, s.id)))
        .collect())
}

// ============================================================================
// runesReforged.json
// ============================================================================

#[derive(Deserialize)]
struct RuneStyle {
    id: i32,
    icon: String,
    #[serde(default)]
    slots: Vec<RuneSlot>,
}

#[derive(Deserialize)]
struct RuneSlot {
    #[serde(default)]
    runes: Vec<RuneEntry>,
}

#[derive(Deserialize)]
struct RuneEntry {
    id: i32,
    icon: String,
}

type RuneMaps = (HashMap<i32, String>, HashMap<i32, String>);

async fn fetch_runes(http: &Client, version: &str) -> Result<RuneMaps, reqwest::Error> {
    let url = format!("{DDRAGON_CDN}/{version}/data/en_US/runesReforged.json");
    let styles: Vec<RuneStyle> = http
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let mut rune_icons = HashMap::new();
    let mut style_icons = HashMap::new();
    for style in styles {
        style_icons.insert(style.id, style.icon);
        for slot in style.slots {
            for rune in slot.runes {
                rune_icons.insert(rune.id, rune.icon);
            }
        }
    }
    Ok((rune_icons, style_icons))
}

// ============================================================================
// champion.json
// ============================================================================

#[derive(Deserialize)]
struct ChampionFile {
    data: HashMap<String, ChampionEntry>,
}

#[derive(Deserialize)]
struct ChampionEntry {
    /// DDragon key (e.g. `"Ahri"`) — matches `participant.championName`.
    id: String,
    /// Numeric championId as a string (e.g. `"103"`).
    key: String,
}

async fn fetch_champions(
    http: &Client,
    version: &str,
) -> Result<HashMap<String, i64>, reqwest::Error> {
    let url = format!("{DDRAGON_CDN}/{version}/data/en_US/champion.json");
    let file: ChampionFile = http
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(file
        .data
        .into_values()
        .filter_map(|c| c.key.parse::<i64>().ok().map(|k| (c.id, k)))
        .collect())
}

#[cfg(test)]
impl DdragonData {
    /// Build a fixed table for tests (no network).
    pub fn stub(version: &str) -> Self {
        Self {
            version: version.to_string(),
            spells: HashMap::from([
                (4, "SummonerFlash".to_string()),
                (11, "SummonerSmite".to_string()),
            ]),
            rune_icons: HashMap::from([(
                8010,
                "perk-images/Styles/Precision/Conqueror/Conqueror.png".to_string(),
            )]),
            style_icons: HashMap::from([
                (8000, "perk-images/Styles/7201_Precision.png".to_string()),
                (8100, "perk-images/Styles/7200_Domination.png".to_string()),
            ]),
            champion_ids: HashMap::from([
                ("Ahri".to_string(), 103),
                ("Viego".to_string(), 234),
                ("Fiddlesticks".to_string(), 9),
            ]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_versioned_and_versionless_urls() {
        let d = DdragonData::stub("14.24.1");
        assert_eq!(
            d.spell_src(4).as_deref(),
            Some("https://ddragon.leagueoflegends.com/cdn/14.24.1/img/spell/SummonerFlash.png")
        );
        assert_eq!(
            d.keystone_src(8010).as_deref(),
            Some(
                "https://ddragon.leagueoflegends.com/cdn/img/perk-images/Styles/Precision/Conqueror/Conqueror.png"
            )
        );
        assert_eq!(
            d.secondary_src(8000).as_deref(),
            Some(
                "https://ddragon.leagueoflegends.com/cdn/img/perk-images/Styles/7201_Precision.png"
            )
        );
        // Unknown ids resolve to nothing (icon simply omitted).
        assert_eq!(d.spell_src(999), None);
        assert_eq!(d.keystone_src(1), None);
    }

    #[test]
    fn resolves_champion_ids() {
        let d = DdragonData::stub("14.24.1");
        assert_eq!(d.champion_id("Ahri"), Some(103));
        assert_eq!(d.champion_id("Viego"), Some(234));
        // Unknown champion → no id (mastery lookup simply skipped).
        assert_eq!(d.champion_id("Unknown"), None);
    }

    #[test]
    fn champion_key_reconciles_matchv5_casing() {
        let d = DdragonData::stub("14.24.1");
        // The notorious case: match-v5 sends "FiddleSticks", DDragon key is
        // "Fiddlesticks" (dev-relations #580). The splash/square/mastery lookups
        // must land on the canonical key, not 404 on the raw name.
        assert_eq!(d.champion_key("FiddleSticks"), "Fiddlesticks");
        assert_eq!(d.champion_id("FiddleSticks"), Some(9));
        // Exact keys pass straight through; unknowns fall back untouched.
        assert_eq!(d.champion_key("Ahri"), "Ahri");
        assert_eq!(d.champion_key("Unknown"), "Unknown");
    }
}
