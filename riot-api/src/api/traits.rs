use std::fmt::Debug;

pub trait RegionEndpoint: Debug + Send + Sync {
    fn to_global_endpoint(&self) -> String;
    fn to_endpoint(&self) -> String;
}

pub trait LeagueExt {
    fn league_points(&self) -> u16;
}
