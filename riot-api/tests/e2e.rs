use std::env;

use dotenv::dotenv;
use tentrackule_shared::Region;
use tentrackule_shared::traits::api::{AccountApi, LeagueApi, MatchApi};

mod lol {
    use super::*;
    use tentrackule_riot_api::api::lol::LolApiClient;

    #[tokio::test]
    #[ignore = "API Key required"]
    async fn get_account_by_riot_id_returns_expected_account() {
        dotenv().ok();
        let key = env::var("LOL_API_KEY").expect("LOL_API_KEY not set");
        let api = LolApiClient::new(key);

        let account = api
            .get_account_by_riot_id("Le Conservateur".to_string(), "3012".to_string())
            .await
            .unwrap();

        assert_eq!(
            account.puuid,
            "jG0VKFsMuF2aWaQoiDxJ1brhlXyMY7kj4HfIAucciWH_9YVdWVpbQDIRhJWQQGhP89qCrp5EwLxl3Q"
        );
        assert_eq!(account.game_name, "Le Conservateur".to_string());
        assert_eq!(account.tag_line, "3012".to_string());
    }

    #[tokio::test]
    #[ignore = "API Key required"]
    async fn get_last_match_id_and_match_works() {
        dotenv().ok();
        let key = env::var("LOL_API_KEY").expect("LOL_API_KEY not set");
        let api = LolApiClient::new(key);

        let account = api
            .get_account_by_riot_id("Le Conservateur".to_string(), "3012".to_string())
            .await
            .unwrap();

        let last_id = api
            .get_last_match_id(account.puuid.clone(), Region::Euw)
            .await
            .unwrap()
            .expect("should return a match id");

        let match_data = api.get_match(last_id, Region::Euw).await.unwrap();

        assert_eq!(match_data.participants.len(), 10);
    }

    #[tokio::test]
    #[ignore = "API Key required"]
    async fn get_leagues_does_not_error() {
        dotenv().ok();
        let key = env::var("LOL_API_KEY").expect("LOL_API_KEY not set");
        let api = LolApiClient::new(key);

        let account = api
            .get_account_by_riot_id("Le Conservateur".to_string(), "3012".to_string())
            .await
            .unwrap();

        let leagues = api
            .get_leagues(account.puuid.clone(), Region::Euw)
            .await
            .unwrap();

        for league in &leagues {
            assert!(!league.queue_type.is_empty());
        }
    }
}

mod tft {
    use tentrackule_riot_api::api::tft::TftApiClient;

    use super::*;

    #[tokio::test]
    #[ignore = "API Key required"]
    async fn tft_get_last_match_id_and_match_works() {
        dotenv().ok();
        let key = env::var("TFT_API_KEY").expect("TFT_API_KEY not set");
        let api = TftApiClient::new(key);

        let account = api
            .get_account_by_riot_id("RayDragsley".to_string(), "EUW".to_string())
            .await
            .unwrap();

        let last_id = api
            .get_last_match_id(account.puuid.clone(), Region::Euw)
            .await
            .unwrap()
            .expect("should return a match id");

        let match_data = api.get_match(last_id, Region::Euw).await.unwrap();

        assert_eq!(match_data.info.participants.len(), 8);
    }
}
