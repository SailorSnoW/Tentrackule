use std::fmt::Debug;

pub trait RegionEndpoint: Debug + Send + Sync {
    fn to_global_endpoint(&self) -> String;
    fn to_endpoint(&self) -> String;
}

/// An access to essentials league data for an external league implementation
pub trait LeagueExt {
    fn league_points(&self) -> u16;
}
