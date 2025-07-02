# 🐙 Tentrackule

Tentrackule is a Discord bot written in **Rust** that tracks players on
**League of Legends** and sends an alert when a new match is completed. It uses
the Riot Games API to fetch match information and stores data in a local
**SQLite** database. Discord integration is handled through **Serenity** and
the **Poise** command framework.

The project aims to be lightweight and respectful of the Riot API rate limits by
caching useful information and only calling the API when required.

[Join the community Discord](https://discord.gg/JbFPpVmaPe)

## 🚀 Features

- 🔔 **Game Completion Alerts**: Get notified when a tracked player finishes a game.
- 📊 Fetch game statistics for **LoL** games directly from Riot.
- 🔍 Track player performance via their **Summoner Name** and tag.
- ⚡ Optimize API calls to reduce quota usage.
- 📚 Local storage with **SQLite** for better efficiency.
- 🌐 Works across multiple guilds and allows a dedicated alert channel per guild.

## 🏗 Architecture Overview

Tentrackule is organized as a Cargo workspace made of several crates:

- **bot** – Discord integration with slash commands via Poise.
- **db** – asynchronous wrapper around SQLite storing tracked accounts and guild settings.
- **riot-api** – typed Riot Games API client with rate limit handling and metrics logging.
- **alert** – utilities to build and dispatch Discord embeds.
- **shared** – common types and traits shared across the workspace.

The main crate spawns the Discord bot and a polling task that regularly checks Riot for new matches. Each component runs in its own Tokio task and failures are logged.

## 📥 Installation and Execution

### Prerequisites

- **Rust** installed ([Rustup](https://rustup.rs/))
- A **Riot Games API key** ([Get an API key](https://developer.riotgames.com/))
- A **Discord bot** with its token

### Configuration

1. Clone the repository:
   ```bash
   git clone https://github.com/SailorSnoW/Tentrackule.git
   cd Tentrackule
   ```
2. Create a `.env` file at the root with:

   ```env
   # Trailing slashes are ignored; '~' expands to $HOME
   DB_PATH=path_to_the_db_storage_writting

   DISCORD_BOT_TOKEN=your_discord_bot_token
   RIOT_API_KEY=your_riot_api_key

   # Optional: configure log verbosity (error, warn, info, debug, trace)
   RUST_LOG=info

   # Optional: directory to store log files
   LOG_DIR=./logs

   # Optional: maximum number of log files to keep
   LOG_MAX_FILES=7

   # Optional: polling interval in seconds for fetching new results
   POLL_INTERVAL_SECONDS=60

   # Optional: version of the ddragon assets
   DDRAGON_VERSION=15.12.1

   ```

3. Compile and run the bot:
   ```bash
   cargo run --release
   ```

### Available Commands

The bot exposes several slash commands once invited to your guild:
| Command | Description |
| ------- | ----------- |
| `/track <name> <tag> <region>` | Start tracking a player. |
| `/untrack <name> <tag>` | Stop tracking a player in the current server. |
| `/show_tracked` | List all tracked players in this server. |
| `/set_alert_channel <channel>` | Choose where alerts should be posted. |
| `/set_queue_alert <queue> <enabled>` | Enable or disable alerts for a specific queue. |
| `/current_alert_channel` | Display the currently configured alert channel. |

## 🛠 Contribution

Contributions are welcome!

- **Bug reports and improvements**: Open an **issue** to report a problem or suggest an idea.
- **Pull Requests**: Fork the repository and submit your changes via a PR.

## 💡 Feature Requests

Have an idea to improve the bot? Feel free to open an **issue** with the `enhancement` label.

## ⚠️ Important Notice

I do **not** host a public version of the bot at this time.  
Riot Games has not granted me access to the **production** API, preventing deployment at a large scale.

If you want to use **Tentrackule**, you will need to host it yourself with your own API key.

## 📄 License

This project is distributed under the terms of the MIT license. See
[`LICENSE.md`](LICENSE.md) for details.

---
