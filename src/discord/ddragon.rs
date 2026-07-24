use std::collections::HashMap;

use reqwest::Client;
use serde::Deserialize;
use tracing::{info, warn};

pub const DDRAGON_CDN: &str = "https://ddragon.leagueoflegends.com/cdn";

const VERSIONS_URL: &str = "https://ddragon.leagueoflegends.com/api/versions.json";

pub const DEFAULT_DDRAGON_VERSION: &str = "16.1.1";

pub async fn resolve_version(http: &Client, override_version: Option<&str>) -> String {
    if let Some(version) = override_version {
        info!(version, "🖼️ Using pinned DDRAGON_VERSION override");
        return version.to_string();
    }
    match fetch_latest_version(http).await {
        Some(version) => {
            info!(version, "🖼️ Resolved live Data Dragon patch");
            version
        }
        None => {
            warn!(
                version = DEFAULT_DDRAGON_VERSION,
                "🖼️ ⚠️ Version lookup failed; falling back to the compiled-in patch"
            );
            DEFAULT_DDRAGON_VERSION.to_string()
        }
    }
}

pub async fn fetch_latest_version(http: &Client) -> Option<String> {
    match fetch_versions(http).await {
        Ok(versions) => {
            let latest = versions.into_iter().next();
            if latest.is_none() {
                warn!("🖼️ ⚠️ Data Dragon versions.json was empty");
            }
            latest
        }
        Err(e) => {
            warn!(error = %e, "🖼️ ⚠️ Failed to fetch Data Dragon versions.json");
            None
        }
    }
}

async fn fetch_versions(http: &Client) -> Result<Vec<String>, reqwest::Error> {
    http.get(VERSIONS_URL)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
}

#[derive(Debug, Clone, Default)]
pub struct DdragonData {
    version: String,
    spells: HashMap<i32, String>,
    rune_icons: HashMap<i32, String>,
    style_icons: HashMap<i32, String>,
    champion_ids: HashMap<String, i64>,
}

impl DdragonData {
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

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn spell_src(&self, key: i32) -> Option<String> {
        let id = self.spells.get(&key)?;
        Some(format!("{DDRAGON_CDN}/{}/img/spell/{id}.png", self.version))
    }

    pub fn keystone_src(&self, perk_id: i32) -> Option<String> {
        let icon = self.rune_icons.get(&perk_id)?;
        Some(format!("{DDRAGON_CDN}/img/{icon}"))
    }

    pub fn secondary_src(&self, style_id: i32) -> Option<String> {
        let icon = self.style_icons.get(&style_id)?;
        Some(format!("{DDRAGON_CDN}/img/{icon}"))
    }

    pub fn champion_id(&self, champion_name: &str) -> Option<i64> {
        self.champion_ids
            .get(&self.champion_key(champion_name))
            .copied()
    }

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

#[derive(Deserialize)]
struct SummonerFile {
    data: HashMap<String, SummonerSpell>,
}

#[derive(Deserialize)]
struct SummonerSpell {
    key: String,
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

#[derive(Deserialize)]
struct ChampionFile {
    data: HashMap<String, ChampionEntry>,
}

#[derive(Deserialize)]
struct ChampionEntry {
    id: String,
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
        assert_eq!(d.spell_src(999), None);
        assert_eq!(d.keystone_src(1), None);
    }

    #[test]
    fn resolves_champion_ids() {
        let d = DdragonData::stub("14.24.1");
        assert_eq!(d.champion_id("Ahri"), Some(103));
        assert_eq!(d.champion_id("Viego"), Some(234));
        assert_eq!(d.champion_id("Unknown"), None);
    }

    #[test]
    fn champion_key_reconciles_matchv5_casing() {
        let d = DdragonData::stub("14.24.1");
        assert_eq!(d.champion_key("FiddleSticks"), "Fiddlesticks");
        assert_eq!(d.champion_id("FiddleSticks"), Some(9));
        assert_eq!(d.champion_key("Ahri"), "Ahri");
        assert_eq!(d.champion_key("Unknown"), "Unknown");
    }

    #[tokio::test]
    async fn resolve_version_prefers_explicit_override() {
        let http = Client::new();
        assert_eq!(resolve_version(&http, Some("99.9.9")).await, "99.9.9");
    }

    #[tokio::test]
    #[ignore = "hits the network (Data Dragon versions.json)"]
    async fn resolves_live_version() {
        let http = Client::new();
        let version = fetch_latest_version(&http).await.expect("a live patch");
        assert!(
            version.split('.').count() >= 2,
            "unexpected version shape: {version}"
        );
        assert_eq!(resolve_version(&http, None).await, version);
    }
}
