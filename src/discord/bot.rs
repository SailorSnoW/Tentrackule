use std::sync::Arc;

use tracing::{error, info, warn};

use crate::db::Repository;
use crate::error::AppError;
use crate::riot::RiotClient;

use super::commands;
use super::image_gen::ImageGenerator;

pub struct Data {
    pub db: Repository,
    pub riot: RiotClient,
    pub image_gen: Arc<ImageGenerator>,
}

impl std::fmt::Debug for Data {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Data")
            .field("db", &self.db)
            .field("riot", &self.riot)
            .field("image_gen", &"<ImageGenerator>")
            .finish()
    }
}

pub type Context<'a> = poise::Context<'a, Data, AppError>;

pub fn create_framework(data: Data) -> poise::Framework<Data, AppError> {
    poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![
                commands::track(),
                commands::untrack(),
                commands::list(),
                commands::config(),
                commands::dev_test_alert(),
            ],
            on_error: |error| {
                Box::pin(async move {
                    handle_error(error).await;
                })
            },
            ..Default::default()
        })
        .setup(|ctx, ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                info!(
                    bot_name = %ready.user.name,
                    guild_count = ready.guilds.len(),
                    "🎮 Bot is ready"
                );
                Ok(data)
            })
        })
        .build()
}

async fn handle_error(error: poise::FrameworkError<'_, Data, AppError>) {
    match error {
        poise::FrameworkError::Command { error, ctx, .. } => {
            let command_name = ctx.command().name.as_str();
            error!(
                error = ?error,
                command = command_name,
                user_id = %ctx.author().id,
                "🎮 ❌ Command execution failed"
            );
            let _ = ctx.say(user_message(&error)).await;
        }
        poise::FrameworkError::ArgumentParse { error, ctx, .. } => {
            warn!(
                error = %error,
                command = ctx.command().name.as_str(),
                "🎮 ⚠️ Invalid command argument"
            );
            let _ = ctx.say(format!("Invalid argument: {}", error)).await;
        }
        poise::FrameworkError::MissingBotPermissions {
            missing_permissions,
            ctx,
            ..
        } => {
            warn!(
                permissions = %missing_permissions,
                command = ctx.command().name.as_str(),
                "🎮 ⚠️ Bot missing permissions"
            );
            let _ = ctx
                .say(format!("Missing permissions: {}", missing_permissions))
                .await;
        }
        poise::FrameworkError::MissingUserPermissions {
            missing_permissions,
            ctx,
            ..
        } => {
            if let Some(perms) = missing_permissions {
                warn!(
                    permissions = %perms,
                    user_id = %ctx.author().id,
                    command = ctx.command().name.as_str(),
                    "🎮 ⚠️ User missing permissions"
                );
                let _ = ctx
                    .say(format!("You need these permissions: {}", perms))
                    .await;
            }
        }
        poise::FrameworkError::CooldownHit {
            remaining_cooldown,
            ctx,
            ..
        } => {
            let wait_s = remaining_cooldown.as_secs_f64().ceil() as u64;
            let _ = ctx
                .send(
                    poise::CreateReply::default()
                        .content(format!("Slow down: try again in {}s.", wait_s.max(1)))
                        .ephemeral(true),
                )
                .await;
        }
        poise::FrameworkError::NotAnOwner { ctx, .. } => {
            warn!(
                user_id = %ctx.author().id,
                command = ctx.command().name.as_str(),
                "🎮 ⚠️ Owner-only command refused"
            );
            let _ = ctx
                .say("This command is restricted to the bot owner.")
                .await;
        }
        other => {
            error!(error = ?other, "🎮 ❌ Unhandled framework error");
        }
    }
}

fn user_message(error: &AppError) -> String {
    match error {
        AppError::PlayerNotFound { .. }
        | AppError::PlayerNotTracked
        | AppError::InvalidRegion(_) => {
            format!("Error: {error}")
        }
        AppError::RiotApi { status: 429, .. } => {
            "The Riot API is busy right now. Please try again in a minute.".to_string()
        }
        _ => "Something went wrong. Please try again later.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::user_message;
    use crate::error::AppError;

    #[test]
    fn expected_errors_are_shown_verbatim() {
        let not_found = AppError::PlayerNotFound {
            game_name: "Foo".into(),
            tag_line: "EUW".into(),
        };
        assert_eq!(user_message(&not_found), "Error: Player not found: Foo#EUW");
        assert_eq!(
            user_message(&AppError::PlayerNotTracked),
            "Error: Player not tracked in this server"
        );
    }

    #[test]
    fn internal_details_are_not_echoed() {
        let forbidden = AppError::RiotApi {
            status: 403,
            message: r#"{"status":{"message":"Forbidden","status_code":403}}"#.into(),
        };
        let shown = user_message(&forbidden);
        assert!(!shown.contains("403") && !shown.contains("Forbidden"));

        let db = AppError::Database(sqlx::Error::PoolTimedOut);
        assert!(!user_message(&db).contains("pool"));

        let render = AppError::ImageGeneration {
            message: "renderer returned 401".into(),
        };
        assert!(!user_message(&render).contains("401"));
    }

    #[test]
    fn rate_limits_get_a_retry_hint() {
        let limited = AppError::RiotApi {
            status: 429,
            message: "Rate limit exceeded".into(),
        };
        assert!(user_message(&limited).contains("try again"));
    }
}
