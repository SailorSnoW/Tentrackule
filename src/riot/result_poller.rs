use super::{
    ApiRequest,
    types::{LeagueEntryDto, MatchDto, Region},
};
use crate::{
    db::{DbRequest, types::Account},
    discord::{AlertSenderMessage, AlertSenderTx},
};
use log::{debug, info};
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

    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
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
        self.db_sender
            .send(DbRequest::GetAllAccounts { respond_to: tx })
            .await
            .unwrap();
        rx.await.unwrap().unwrap()
    }

    async fn process_account(&self, account: Account) {
        debug!(
            "🏅 Processing player: {}#{}",
            account.game_name, account.tag_line
        );
        let new_match_id = self
            .get_new_match_id(account.puuid.clone(), account.region.clone())
            .await;

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
        let match_data = self
            .get_match_data(new_match_id, account.region.clone())
            .await;

        // This check ensure we do not alert on old match if any match was already cached in DB.
        if self.start_time > match_data.info.game_creation {
            debug!(
                "🏅 {}#{}: New fetched game result is older than the startup time of this bot, ignoring.",
                account.game_name, account.tag_line
            );
            return;
        }

        // And the new league data
        let league = self
            .get_ranked_solo_duo_league(account.puuid.clone(), account.region)
            .await;

        let _ = self
            .bot_sender
            .send(AlertSenderMessage::AlertNewMatchResult {
                puuid: account.puuid,
                match_data,
                league_data: league,
            })
            .await;
    }

    async fn get_new_match_id(&self, puuid: String, region: Region) -> String {
        let (tx, rx) = oneshot::channel();
        self.api_sender
            .send(ApiRequest::LastMatch {
                puuid,
                region,
                respond_to: tx,
            })
            .await
            .unwrap();
        rx.await.unwrap().unwrap().unwrap()
    }

    async fn set_new_match_id(&self, puuid: String, match_id: String) {
        let (tx, rx) = oneshot::channel();
        self.db_sender
            .send(DbRequest::SetLastMatchId {
                puuid,
                match_id,
                respond_to: tx,
            })
            .await
            .unwrap();
        let _ = rx.await;
    }

    async fn get_match_data(&self, match_id: String, region: Region) -> MatchDto {
        let (tx, rx) = oneshot::channel();
        self.api_sender
            .send(ApiRequest::LastMatchData {
                match_id,
                region,
                respond_to: tx,
            })
            .await
            .unwrap();
        rx.await.unwrap().unwrap()
    }

    async fn get_ranked_solo_duo_league(
        &self,
        puuid: String,
        region: Region,
    ) -> Option<LeagueEntryDto> {
        let (tx, rx) = oneshot::channel();
        self.api_sender
            .send(ApiRequest::Leagues {
                puuid,
                region,
                respond_to: tx,
            })
            .await
            .unwrap();
        let leagues = rx.await.unwrap().unwrap();
        leagues
            .into_iter()
            .find(|league| league.is_ranked_solo_duo())
    }
}
