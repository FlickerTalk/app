use serde::{Deserialize, Serialize};

// Phones keep these ids and the SDK and the games kit list them too, so an id is only ever added.
const WHO: [&str; 12] = [
    "ari", "noa", "milo", "juno", "kit", "lou", "rio", "sam", "umi", "val", "zuri", "nico",
];
const HAIR: [&str; 19] = [
    "none",
    "long-fringe",
    "ponytail",
    "braids",
    "long-braid",
    "bun",
    "bob",
    "curly-puff",
    "two-puffs",
    "wavy-headband",
    "messy-fringe",
    "short-crop",
    "side-part",
    "spiky",
    "curls",
    "quiff",
    "undercut",
    "cap",
    "mini-twists",
];
const HAIR_COLOURS: [&str; 6] = ["ink", "chestnut", "copper", "golden", "cream", "silver"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Player {
    pub who: String,
    pub hair: String,
    pub hair_colour: String,
    pub name: String,
}

impl Player {
    pub fn in_the_cast(&self) -> bool {
        WHO.contains(&self.who.as_str())
            && HAIR.contains(&self.hair.as_str())
            && HAIR_COLOURS.contains(&self.hair_colour.as_str())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde_json::json;

    use super::*;

    const SDK_AVATARS: &str = r#"{
  "who": ["ari", "noa", "milo", "juno", "kit", "lou", "rio", "sam", "umi", "val", "zuri", "nico"],
  "hair": [
    "none",
    "long-fringe",
    "ponytail",
    "braids",
    "long-braid",
    "bun",
    "bob",
    "curly-puff",
    "two-puffs",
    "wavy-headband",
    "messy-fringe",
    "short-crop",
    "side-part",
    "spiky",
    "curls",
    "quiff",
    "undercut",
    "cap",
    "mini-twists"
  ],
  "hairColour": ["ink", "chestnut", "copper", "golden", "cream", "silver"]
}
"#;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Cast {
        who: Vec<String>,
        hair: Vec<String>,
        hair_colour: Vec<String>,
    }

    fn look(who: &str, hair: &str, hair_colour: &str) -> Player {
        Player {
            who: who.to_owned(),
            hair: hair.to_owned(),
            hair_colour: hair_colour.to_owned(),
            name: String::new(),
        }
    }

    #[test]
    fn the_cast_is_the_sdks_avatars_json_word_for_word() {
        let cast: Cast = serde_json::from_str(SDK_AVATARS).expect("the SDK's list");
        assert_eq!(cast.who, WHO);
        assert_eq!(cast.hair, HAIR);
        assert_eq!(cast.hair_colour, HAIR_COLOURS);
    }

    #[test]
    fn every_id_is_unique_short_lowercase_and_never_flick() {
        for list in [&WHO[..], &HAIR[..], &HAIR_COLOURS[..]] {
            assert_eq!(
                list.iter().collect::<BTreeSet<_>>().len(),
                list.len(),
                "{list:?}"
            );
            for id in list {
                assert!(
                    !id.is_empty()
                        && id.len() <= 24
                        && id
                            .bytes()
                            .all(|byte| byte.is_ascii_lowercase() || byte == b'-'),
                    "{id}"
                );
                assert_ne!(*id, "flick");
            }
        }
    }

    #[test]
    fn every_look_made_from_the_cast_is_in_it() {
        let mut looks = 0;
        for who in WHO {
            for hair in HAIR {
                for hair_colour in HAIR_COLOURS {
                    assert!(
                        look(who, hair, hair_colour).in_the_cast(),
                        "{who} {hair} {hair_colour}"
                    );
                    looks += 1;
                }
            }
        }
        assert_eq!(looks, 12 * 19 * 6);
    }

    #[test]
    fn a_look_with_one_id_from_outside_the_cast_is_not_in_it() {
        for (who, hair, hair_colour) in [
            ("flick", "none", "ink"),
            ("bob", "none", "ink"),
            ("Ari", "none", "ink"),
            ("", "none", "ink"),
            (" ari", "none", "ink"),
            ("ari", "mohawk", "ink"),
            ("ari", "ink", "ink"),
            ("ari", "", "ink"),
            ("ari", "none", "blue"),
            ("ari", "none", "none"),
            ("ari", "none", "Ink"),
        ] {
            assert!(
                !look(who, hair, hair_colour).in_the_cast(),
                "{who:?} {hair:?} {hair_colour:?}"
            );
        }
    }

    #[test]
    fn a_player_reads_and_writes_as_the_plugin_api_names_it() {
        let player = Player {
            name: "Ana".to_owned(),
            ..look("juno", "curly-puff", "copper")
        };
        let written = serde_json::to_value(&player).expect("writes");
        assert_eq!(
            written,
            json!({ "who": "juno", "hair": "curly-puff", "hairColour": "copper", "name": "Ana" })
        );
        assert_eq!(
            serde_json::from_value::<Player>(written).expect("reads"),
            player
        );
    }

    #[test]
    fn a_record_with_a_field_more_or_less_or_one_that_is_not_text_is_no_player() {
        for wrong in [
            json!({ "who": "ari", "hair": "none", "hairColour": "ink", "name": "", "vehicle": "bus" }),
            json!({ "who": "ari", "hair": "none", "hairColour": "ink" }),
            json!({ "who": "ari", "hair": "none", "name": "" }),
            json!({ "who": "ari", "hair": "none", "hair_colour": "ink", "name": "" }),
            json!({ "who": "ari", "hair": "none", "hairColour": "ink", "name": 7 }),
            json!({ "who": "ari", "hair": "none", "hairColour": "ink", "name": null }),
            json!({ "who": ["ari"], "hair": "none", "hairColour": "ink", "name": "" }),
            json!("ari"),
            json!(null),
        ] {
            assert!(
                serde_json::from_value::<Player>(wrong.clone()).is_err(),
                "{wrong}"
            );
        }
    }
}
