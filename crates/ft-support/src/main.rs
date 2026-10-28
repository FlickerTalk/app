//! `ft-support`: the FlickerTalk support contact. Configured by the environment (`--help`).

use std::process::ExitCode;

use ft_support::config::{Config, USAGE};

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Run,
    Init,
    Help,
}

/// What the command line asks for.
fn command(args: &[String]) -> Command {
    match args.get(1).map(String::as_str) {
        None => Command::Run,
        Some("init") => Command::Init,
        Some(_) => Command::Help,
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let command = command(&args);
    if command == Command::Help {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let outcome = match Config::from_env() {
        Ok(config) if command == Command::Init => ft_support::init(&config).await.map(|card| {
            println!("Identity ready: key in {}, bundle in {}.", config.key_file.display(), config.identity_file.display());
            println!("{card}");
        }),
        Ok(config) => ft_support::run(&config).await,
        Err(error) => Err(error),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ft-support: {error:#}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn the_command_is_the_first_argument() {
        assert_eq!(command(&args(&["ft-support"])), Command::Run);
        assert_eq!(command(&args(&["ft-support", "init"])), Command::Init);
        assert_eq!(command(&args(&["ft-support", "--help"])), Command::Help);
        assert_eq!(command(&args(&["ft-support", "-h"])), Command::Help);
        assert_eq!(command(&args(&["ft-support", "launch"])), Command::Help, "anything else gets the help");
    }
}
