use super::{
    types::{LeagueEntryDto, LeaguePoints, MatchDto, Region},
    ApiRequest,
};
use crate::{
    db::{types::Account, DbRequest},
    discord::{AlertSenderMessage, AlertSenderTx},
    riot::types::{MatchDtoWithLeagueInfo, QueueType},
};
use log::{debug, error, info, warn};
use poise::serenity_prelude::Timestamp;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

/// Poller responsible for automatically fetching new results of tracked player from Riot servers, parsing results data and sending it to the discord receiver when alerting is needed.
pub struct ResultPoller {
    api_sender: mpsc::Sender<ApiRequest>,
    db_sender: mpsc::Sender<DbRequest>,
    bot_sender: AlertSenderTx,
    start_time: u64,
}

impl ResultPoller {
    pub fn new(
        api_sender: mpsc::Sender<ApiRequest>,
        db_sender: mpsc::Sender<DbRequest>,
        bot_sender: AlertSenderTx,
    ) -> Self {
        Self {
            api_sender,
            db_sender,
            bot_sender,
            start_time: Timestamp::now().timestamp_millis() as u64,
        }
    }

    pub fn start(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            self.run().await;
        })
    }

    async fn run(&self) {
        info!("🏅 Starting the Result Poller...");

        // Each time we will poll for new results.
        let mut interval = tokio::time::interval(Duration::from_secs(60));

        loop {
            interval.tick().await;

            info!("🏅 New fetching session is starting.");
            // We fetch all registered accounts to get the results.
            let accounts = self.get_all_accounts().await;
            for account in accounts {
                self.process_account(account).await
            }
        }
    }

    async fn get_all_accounts(&self) -> Vec<Account> {
        let (tx, rx) = oneshot::channel();
        if let Err(e) = self
            .db_sender
            .send(DbRequest::GetAllAccounts { respond_to: tx })
            .await
        {
            error!("Failed to send DB request: {}", e);
            return Vec::new();
        }
        match rx.await {
            Ok(Ok(accounts)) => accounts,
            Ok(Err(e)) => {
                error!("Database error while fetching accounts: {}", e);
                Vec::new()
            }
            Err(e) => {
                error!("DB channel error: {}", e);
                Vec::new()
            }
        }
    }

    async fn process_account(&self, account: Account) {
        debug!(
            "🏅 Processing player: {}#{}",
            account.game_name, account.tag_line
        );
        let Some(new_match_id) = self
            .get_new_match_id(account.puuid.clone(), account.region.clone())
            .await
        else {
            return;
        };

        // If this is equal, no new game was ended for this account, we do nothing and just return.
        if new_match_id == account.last_match_id {
            debug!(
                "🏅 {}#{}: No new result detected, ignoring.",
                account.game_name, account.tag_line
            );
            return;
        }

        // Else we first register the new game ID in the database
        debug!(
            "🏅 {}#{}: Caching new match ID {} in database.",
            account.game_name, account.tag_line, new_match_id
        );
        self.set_new_match_id(account.puuid.clone(), new_match_id.clone())
            .await;

        // Then we fetch the game data.
        let Some(match_data) = self
            .get_match_data(new_match_id, account.region.clone())
            .await
        else {
            return;
        };

        // This check ensure we do not alert on old match if any match was already cached in DB.
        if self.start_time > match_data.info.game_creation {
            debug!(
                "🏅 {}#{}: New fetched game result is older than the startup time of this bot, ignoring.",
                account.game_name, account.tag_line
            );
            return;
        }

        match match_data.queue_type() {
            QueueType::SoloDuo => {
                // Get the cached league points data
                let cached_league_points = account.cached_league_points;
                // Get the new league data
                let league = self
                    .get_ranked_solo_duo_league(account.puuid.clone(), account.region)
                    .await;

                match &league {
                    Some(x) => {
                        debug!(
                            "Caching new fetched league points ({}) for {}#{}.",
                            x.league_points, account.game_name, account.tag_line
                        );
                        self.update_league_points(
                            account.puuid.clone(),
                            QueueType::SoloDuo,
                            x.league_points,
                        )
                        .await
                    }
                    None => warn!("Something went wrong and no league data was fetched !"),
                }

                debug!(
                    "🏅 Dispatch new alert for: {}#{}",
                    account.game_name, account.tag_line
                );
                let _ = self
                    .bot_sender
                    .send(AlertSenderMessage::DispatchNewAlert {
                        puuid: account.puuid,
                        match_data: MatchDtoWithLeagueInfo::new(
                            match_data,
                            league,
                            cached_league_points,
                        ),
                    })
                    .await;
            }
            QueueType::Unhandled => {
                debug!(
                    "🏅 {}#{}: Unsupported queue type, ignoring.",
                    account.game_name, account.tag_line
                );
            }
        }
    }

    async fn get_new_match_id(&self, puuid: String, region: Region) -> Option<String> {
        let (tx, rx) = oneshot::channel();
        if let Err(e) = self
            .api_sender
            .send(ApiRequest::LastMatch {
                puuid,
                region,
                respond_to: tx,
            })
            .await
        {
            error!("Failed to send API request: {}", e);
            return None;
        }
        match rx.await {
            Ok(Ok(maybe_id)) => maybe_id,
            Ok(Err(err)) => {
                error!("Riot API error while fetching last match id: {:?}", err);
                None
            }
            Err(e) => {
                error!("API channel error: {}", e);
                None
            }
        }
    }

    async fn set_new_match_id(&self, puuid: String, match_id: String) {
        let (tx, rx) = oneshot::channel();
        if let Err(e) = self
            .db_sender
            .send(DbRequest::SetLastMatchId {
                puuid,
                match_id,
                respond_to: tx,
            })
            .await
        {
            error!("Failed to send DB request: {}", e);
            return;
        }
        let _ = rx.await;
    }

    async fn update_league_points(
        &self,
        puuid: String,
        queue_type: QueueType,
        league_points: LeaguePoints,
    ) {
        let (tx, rx) = oneshot::channel();
        if let Err(e) = self
            .db_sender
            .send(DbRequest::UpdateLeaguePoints {
                puuid,
                queue_type,
                league_points,
                respond_to: tx,
            })
            .await
        {
            error!("Failed to send DB request: {}", e);
            return;
        }
        let _ = rx.await;
    }

    async fn get_match_data(&self, match_id: String, region: Region) -> Option<MatchDto> {
        let (tx, rx) = oneshot::channel();
        if let Err(e) = self
            .api_sender
            .send(ApiRequest::LastMatchData {
                match_id,
                region,
                respond_to: tx,
            })
            .await
        {
            error!("Failed to send API request: {}", e);
            return None;
        }

        match rx.await {
            Ok(Ok(match_data)) => Some(match_data),
            Ok(Err(err)) => {
                error!("Riot API error while fetching match data: {:?}", err);
                None
            }
            Err(e) => {
                error!("API channel error: {}", e);
                None
            }
        }
    }

    async fn get_ranked_solo_duo_league(
        &self,
        puuid: String,
        region: Region,
    ) -> Option<LeagueEntryDto> {
        let (tx, rx) = oneshot::channel();

        if let Err(e) = self
            .api_sender
            .send(ApiRequest::Leagues {
                puuid,
                region,
                respond_to: tx,
            })
            .await
        {
            error!("Failed to send API request: {}", e);
            return None;
        }

        let leagues = match rx.await {
            Ok(Ok(leagues)) => leagues,
            Ok(Err(err)) => {
                error!("Riot API error while fetching leagues: {:?}", err);
                return None;
            }
            Err(e) => {
                error!("API channel error: {}", e);
                return None;
            }
        };

        leagues
            .into_iter()
            .find(|league| league.is_ranked_solo_duo())
    }
}
