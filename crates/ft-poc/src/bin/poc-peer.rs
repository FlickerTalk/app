//! PoC 0 peer for the Mac: joins a room on the relay; the caller says "hello", the callee answers
//! every message with "<message> back".
//!
//! cargo run -p ft-poc --bin poc-peer -- [--relay ws://127.0.0.1:8787] [--room demo]
//!                                          [--role caller|callee] [--offline]

use anyhow::{bail, Result};
use ft_webrtc::{Role, SessionConfig};

const GOOGLE_STUN: &str = "stun:stun.l.google.com:19302";

#[derive(Debug, PartialEq, Eq)]
struct Args {
    relay: String,
    room: String,
    role: Role,
    offline: bool,
}

fn parse(args: impl IntoIterator<Item = String>) -> Result<Args> {
    let mut parsed =
        Args { relay: "ws://127.0.0.1:8787".to_owned(), room: "demo".to_owned(), role: Role::Callee, offline: false };
    let mut args = args.into_iter();
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| anyhow::anyhow!("{flag} needs a value"));
        match flag.as_str() {
            "--relay" => parsed.relay = value()?,
            "--room" => parsed.room = value()?,
            "--role" => {
                parsed.role = match value()?.as_str() {
                    "caller" => Role::Caller,
                    "callee" => Role::Callee,
                    other => bail!("unknown role {other}: use caller or callee"),
                }
            }
            "--offline" => parsed.offline = true,
            other => bail!("unknown argument {other}"),
        }
    }
    Ok(parsed)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = parse(std::env::args().skip(1))?;
    let config = if args.offline {
        SessionConfig::offline()
    } else {
        SessionConfig::with_stun([GOOGLE_STUN.to_owned()])
    };

    let (session, mut inbox) = ft_poc::connect(&args.relay, &args.room, args.role, config).await?;
    println!("joined room {:?} on {} as {:?}; waiting for the other peer…", args.room, args.relay, args.role);

    session.wait_open().await?;
    println!("✓ data channel open");

    if args.role == Role::Caller {
        session.send("hello").await?;
        println!("→ hello");
    }

    while let Some(message) = inbox.next().await {
        println!("← {message}");
        if args.role == Role::Callee {
            let reply = format!("{message} back");
            session.send(&reply).await?;
            println!("→ {reply}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn answers_on_the_local_relay_by_default() {
        let parsed = parse(args(&[])).expect("parses");
        assert_eq!(parsed.relay, "ws://127.0.0.1:8787");
        assert_eq!(parsed.room, "demo");
        assert_eq!(parsed.role, Role::Callee);
        assert!(!parsed.offline);
    }

    #[test]
    fn reads_every_option() {
        let parsed = parse(args(&["--relay", "ws://10.0.2.2:8787", "--room", "r1", "--role", "caller", "--offline"]))
            .expect("parses");
        assert_eq!(parsed.relay, "ws://10.0.2.2:8787");
        assert_eq!(parsed.room, "r1");
        assert_eq!(parsed.role, Role::Caller);
        assert!(parsed.offline);
    }

    #[test]
    fn rejects_an_unknown_role() {
        assert!(parse(args(&["--role", "boss"])).is_err());
    }
}
