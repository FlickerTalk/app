use anyhow::Result;
use sqlx::Row;

use crate::Store;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GamePlayer {
    pub who: String,
    pub hair: String,
    pub hair_colour: String,
    pub name: String,
}

impl Store {
    pub async fn game_player(&self, session: Option<&str>) -> Result<Option<GamePlayer>> {
        let row =
            sqlx::query("SELECT who, hair, hair_colour, name FROM game_players WHERE session IS ?")
                .bind(session)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|row| GamePlayer {
            who: row.get("who"),
            hair: row.get("hair"),
            hair_colour: row.get("hair_colour"),
            name: row.get("name"),
        }))
    }

    pub async fn set_game_player(&self, session: Option<&str>, player: &GamePlayer) -> Result<()> {
        sqlx::query(
            "INSERT INTO game_players (session, who, hair, hair_colour, name) VALUES (?, ?, ?, ?, ?)
             ON CONFLICT (ifnull(session, '')) DO UPDATE SET
                 who = excluded.who, hair = excluded.hair, hair_colour = excluded.hair_colour, name = excluded.name",
        )
        .bind(session)
        .bind(&player.who)
        .bind(&player.hair)
        .bind(&player.hair_colour)
        .bind(&player.name)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::store;

    fn player(who: &str, name: &str) -> GamePlayer {
        GamePlayer {
            who: who.to_owned(),
            hair: "curly-puff".to_owned(),
            hair_colour: "copper".to_owned(),
            name: name.to_owned(),
        }
    }

    #[tokio::test]
    async fn keeps_one_player_for_each_place_and_replaces_it_whole() {
        let store = store().await;
        store.add_session("s1", &[1; 32], 1).await.unwrap();
        store.add_session("s2", &[2; 32], 2).await.unwrap();
        assert_eq!(store.game_player(None).await.unwrap(), None);

        store
            .set_game_player(None, &player("juno", "Ana"))
            .await
            .unwrap();
        store
            .set_game_player(Some("s1"), &player("milo", "Leo"))
            .await
            .unwrap();
        assert_eq!(
            store.game_player(None).await.unwrap(),
            Some(player("juno", "Ana"))
        );
        assert_eq!(
            store.game_player(Some("s1")).await.unwrap(),
            Some(player("milo", "Leo"))
        );
        assert_eq!(store.game_player(Some("s2")).await.unwrap(), None);

        let changed = GamePlayer {
            hair: "none".to_owned(),
            hair_colour: "ink".to_owned(),
            ..player("ari", "")
        };
        store.set_game_player(None, &changed).await.unwrap();
        assert_eq!(store.game_player(None).await.unwrap(), Some(changed));
        assert_eq!(
            store.game_player(Some("s1")).await.unwrap(),
            Some(player("milo", "Leo"))
        );
    }

    #[tokio::test]
    async fn a_sessions_player_goes_with_the_session() {
        let store = store().await;
        store.add_session("s1", &[1; 32], 1).await.unwrap();
        store
            .set_game_player(None, &player("juno", "Ana"))
            .await
            .unwrap();
        store
            .set_game_player(Some("s1"), &player("milo", "Leo"))
            .await
            .unwrap();

        store.remove_session("s1").await.unwrap();
        assert_eq!(store.game_player(Some("s1")).await.unwrap(), None);
        assert_eq!(
            store.game_player(None).await.unwrap(),
            Some(player("juno", "Ana"))
        );
    }

    #[tokio::test]
    async fn removing_a_game_leaves_the_player_to_the_other_games() {
        let store = store().await;
        store
            .install_plugin("com.example.game.chess", "1.0.0", "{}")
            .await
            .unwrap();
        store
            .set_game_player(None, &player("juno", "Ana"))
            .await
            .unwrap();
        store.remove_plugin("com.example.game.chess").await.unwrap();
        assert_eq!(
            store.game_player(None).await.unwrap(),
            Some(player("juno", "Ana"))
        );
    }
}
