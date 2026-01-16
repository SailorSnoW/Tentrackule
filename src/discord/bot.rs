use std::sync::Arc;

use tracing::{error, info, warn};

use crate::db::Repository;
use crate::error::AppError;
use crate::riot::RiotClient;

use super::commands;
use super::image_gen::ImageGenerator;

/// Shared data accessible in all commands
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
            let _ = ctx.say(format!("Error: {}", error)).await;
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
        other => {
            error!(error = ?other, "🎮 ❌ Unhandled framework error");
        }
    }
}
