#[macro_export]
macro_rules! impl_result_poller_traits {
    ($poller:ty, $puuid_field:ident, $last_match_id_field:ident, $set_fn:ident) => {
        impl $crate::WithPuuid for $poller {
            fn puuid_of(account: &tentrackule_shared::Account) -> Option<String> {
                account.$puuid_field.clone()
            }
        }

        #[async_trait::async_trait]
        impl $crate::WithLastMatchId for $poller {
            fn cache(&self) -> tentrackule_db::SharedDatabase {
                self.cache.clone()
            }

            fn last_match_id(account: &tentrackule_shared::Account) -> Option<String> {
                account.$last_match_id_field.clone()
            }

            async fn set_last_match_id(
                &self,
                account: &tentrackule_shared::Account,
                match_id: String,
            ) -> Result<(), tentrackule_shared::traits::CachedSourceError> {
                self.cache().$set_fn(account.id, match_id).await
            }
        }
    };
}
