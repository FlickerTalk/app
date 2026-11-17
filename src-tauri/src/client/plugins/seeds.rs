pub(super) const BUNDLED_PLUGINS: &[&[u8]] = &[
    include_bytes!("../../../resources/plugins/markdown.ftplugin"),
    include_bytes!("../../../resources/plugins/images.ftplugin"),
    include_bytes!("../../../resources/plugins/pdf.ftplugin"),
    include_bytes!("../../../resources/plugins/redact.ftplugin"),
    include_bytes!("../../../resources/plugins/sketch.ftplugin"),
    include_bytes!("../../../resources/plugins/scanner.ftplugin"),
    include_bytes!("../../../resources/plugins/countdown.ftplugin"),
    include_bytes!("../../../resources/plugins/board.ftplugin"),
    include_bytes!("../../../resources/plugins/clean.ftplugin"),
    include_bytes!("../../../resources/plugins/drive.ftplugin"),
    include_bytes!("../../../resources/plugins/list.ftplugin"),
    include_bytes!("../../../resources/plugins/location.ftplugin"),
    include_bytes!("../../../resources/plugins/notes.ftplugin"),
    include_bytes!("../../../resources/plugins/poll.ftplugin"),
    include_bytes!("../../../resources/plugins/sign.ftplugin"),
    include_bytes!("../../../resources/plugins/split.ftplugin"),
    include_bytes!("../../../resources/plugins/pdfviewer.ftplugin"),
    include_bytes!("../../../resources/plugins/translate.ftplugin"),
    include_bytes!("../../../resources/plugins/remind.ftplugin"),
    include_bytes!("../../../resources/plugins/game.tictactoe.ftplugin"),
    include_bytes!("../../../resources/plugins/game.fourinarow.ftplugin"),
    include_bytes!("../../../resources/plugins/game.chess.ftplugin"),
    include_bytes!("../../../resources/plugins/game.backgammon.ftplugin"),
    include_bytes!("../../../resources/plugins/game.checkers.ftplugin"),
    include_bytes!("../../../resources/plugins/game.dotsandboxes.ftplugin"),
    include_bytes!("../../../resources/plugins/game.eights.ftplugin"),
    include_bytes!("../../../resources/plugins/game.gomoku.ftplugin"),
    include_bytes!("../../../resources/plugins/game.mancala.ftplugin"),
    include_bytes!("../../../resources/plugins/game.reversi.ftplugin"),
    include_bytes!("../../../resources/plugins/game.seabattle.ftplugin"),
    include_bytes!("../../../resources/plugins/game.wordduel.ftplugin"),
    include_bytes!("../../../resources/plugins/game.wordgrid.ftplugin"),
];

#[cfg(test)]
const SEED_LIMIT: u64 = 4 * 1024 * 1024;
#[cfg(test)]
const SEEDS_LIMIT: u64 = 8 * 1024 * 1024;

#[cfg(test)]
fn seeds_weight_problem(weights: impl IntoIterator<Item = u64>) -> Option<String> {
    let mut total = 0;
    for weight in weights {
        if weight > SEED_LIMIT {
            return Some(format!(
                "a plugin of {weight} bytes is a download, not a seed"
            ));
        }
        total += weight;
    }
    (total > SEEDS_LIMIT)
        .then(|| format!("the seeds weigh {total} bytes, which is no longer little"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_travels_inside_the_app_stays_tiny() {
        for package in BUNDLED_PLUGINS {
            let plugin = ft_plugins::open(package, &ft_plugins::catalogue())
                .expect("a seed is not signed for us");
            assert!(
                !plugin.manifest.components.is_empty(),
                "{} shows nothing",
                plugin.manifest.id
            );
            assert!(
                plugin.file("dist/index.js").is_some(),
                "{} has no code",
                plugin.manifest.id
            );
        }
        let weights = BUNDLED_PLUGINS.iter().map(|package| package.len() as u64);
        assert_eq!(seeds_weight_problem(weights), None);
    }

    #[test]
    fn a_heavy_seed_is_a_download() {
        assert!(
            seeds_weight_problem([5 * 1024 * 1024]).is_some(),
            "a seed of 5 MiB is refused"
        );
        assert_eq!(
            seeds_weight_problem([3 * 1024 * 1024 + 512 * 1024]),
            None,
            "the PDF viewer travels"
        );
        assert!(
            seeds_weight_problem([SEED_LIMIT + 1]).is_some(),
            "a seed just over the limit is refused"
        );
        assert_eq!(
            seeds_weight_problem([SEED_LIMIT]),
            None,
            "a seed at the limit travels"
        );
        let many = vec![SEED_LIMIT; (SEEDS_LIMIT / SEED_LIMIT) as usize + 1];
        assert!(
            seeds_weight_problem(many).is_some(),
            "seeds that are light one by one but heavy together are refused"
        );
    }
}
