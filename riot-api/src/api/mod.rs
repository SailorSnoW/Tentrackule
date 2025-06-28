mod lol;
pub use lol::match_v5::init_ddragon_version;

pub mod client;
pub mod metrics;
pub mod traits;
pub mod types {
    pub use super::client::AccountDto;
    pub use super::lol::league_v4::{LeagueApi, LeagueEntryDto};
    pub use super::lol::match_v5::{MatchApi, MatchDto, ParticipantDto};
    pub use super::lol::MatchDtoWithLeagueInfo;

    #[cfg(test)]
    pub use super::lol::match_v5::InfoDto;
}
pub use lol::{LolApiClient, LolApiFull};
