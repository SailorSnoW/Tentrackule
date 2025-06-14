# 🐙 Tentrackule

Tentrackule is a Discord bot designed to track and alert when a monitored player has finished a game in **League of Legends** or **Teamfight Tactics**, using the Riot API.  
It is developed in **Rust**, utilizes **SQLite (rusqlite)** for data storage, and relies on **Serenity + Poise** for Discord integration.

The bot aims to be **efficient and optimized** in its use of the Riot API, minimizing unnecessary calls.

## 🚀 Features

- 🔔 **Game Completion Alerts**: Get notified when a tracked player finishes a game.
- 📊 Fetch game statistics for **LoL** and **TFT**.
- 🔍 Track player performance via their **Summoner Name**.
- ⚡ Optimize API calls to reduce quota usage.
- 📚 Local storage with **SQLite** for better efficiency.

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
   DB_PATH=path_to_the_db_storage_writting
   DISCORD_BOT_TOKEN=your_discord_bot_token
   RIOT_API_KEY=your_riot_api_key

   # Optional: configure log verbosity (error, warn, info, debug, trace)
   RUST_LOG=info

   ```

3. Compile and run the bot:
   ```bash
   cargo run --release
   ```

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

---
