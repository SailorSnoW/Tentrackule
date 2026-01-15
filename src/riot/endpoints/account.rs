use crate::error::AppError;
use crate::riot::client::RiotClient;
use crate::riot::region::Region;
use crate::riot::types::AccountDto;

impl RiotClient {
    /// Get account by Riot ID (game name + tag line)
    /// Uses regional routing (americas, europe, asia, sea)
    pub async fn get_account_by_riot_id(
        &self,
        region: Region,
        game_name: &str,
        tag_line: &str,
    ) -> Result<AccountDto, AppError> {
        let url = format!(
            "{}/riot/account/v1/accounts/by-riot-id/{}/{}",
            region.base_url(),
            urlencoding::encode(game_name),
            urlencoding::encode(tag_line)
        );

        self.get(&url).await.map_err(|e| {
            if matches!(&e, AppError::RiotApi { status: 404, .. }) {
                AppError::PlayerNotFound {
                    game_name: game_name.to_string(),
                    tag_line: tag_line.to_string(),
                }
            } else {
                e
            }
        })
    }
}
