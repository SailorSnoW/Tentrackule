mod lol;

pub mod client;
pub mod metrics;
pub mod types {
    pub use super::client::AccountDto;
    pub use super::lol::league_v4::LeagueEntryDto;
    pub use super::lol::match_v5::{MatchDto, ParticipantDto};
    pub use super::lol::MatchDtoWithLeagueInfo;

    #[cfg(test)]
    pub use super::lol::match_v5::InfoDto;
}
pub use lol::LolApi;
