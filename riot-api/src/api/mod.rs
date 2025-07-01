mod lol;

pub mod client;
pub mod metrics;
pub mod traits;
pub mod types {
    pub use super::client::AccountDto;
    pub use super::lol::match_v5::{MatchDto, ParticipantDto};

    #[cfg(test)]
    pub use super::lol::match_v5::InfoDto;
}
pub use lol::LolApiClient;
