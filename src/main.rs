mod app;
mod ui;

use app::App;
use payment_routing::operations::Preset;
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event},
};
use std::{
    env,
    error::Error,
    io::{self, IsTerminal},
    process::ExitCode,
    time::{Duration, Instant},
};

const USAGE: &str = "payment-routing --demo [--seed N] [--scenario balanced|pressure|outage|limited] [--strategy static|reserved]\n\nContinuous synthetic USD payment-network operations console.\nBoth strategies run on identical seeded demand. Starts paused before minute 0.\nRequires an interactive terminal of at least 80 columns by 18 rows.\n\nUsage: cargo run --locked -- --demo\n       cargo run --locked -- --demo --seed 42 --scenario pressure\n       cargo run --locked -- --help\n\nKeys: Space run/pause; . step; +/- speed; r restart; n next seed\n      c scenario; s inspected strategy; 1-6 views; Tab next view\n      j/k rows; PgUp/PgDn; Home/End; Enter inspect; f filter; / search\n      ? help; q / Ctrl-C quit; Esc back or quit";
#[derive(Debug, PartialEq, Eq)]
enum Command {
    Demo {
        seed: u64,
        preset: Preset,
        strategy: usize,
    },
    Help,
}
fn parse_args(args: &[String]) -> Result<Command, String> {
    if args.is_empty() || matches!(args, [arg] if arg == "--help" || arg == "-h") {
        return Ok(Command::Help);
    }
    if args[0] != "--demo" {
        return Err("expected --demo or --help".into());
    }
    let mut seed = 42;
    let mut preset = Preset::Balanced;
    let mut strategy = 1;
    let mut seen = std::collections::BTreeSet::new();
    let mut options = args[1..].chunks_exact(2);
    for pair in &mut options {
        if !seen.insert(pair[0].as_str()) {
            return Err(format!("duplicate {}", pair[0]));
        }
        match pair[0].as_str() {
            "--seed" => seed = pair[1].parse().map_err(|_| "seed must be a u64 integer")?,
            "--scenario" => {
                preset = *Preset::ALL
                    .iter()
                    .find(|p| p.name() == pair[1])
                    .ok_or("scenario must be balanced, pressure, outage or limited")?
            }
            "--strategy" => {
                strategy = match pair[1].as_str() {
                    "static" => 0,
                    "reserved" => 1,
                    _ => return Err("strategy must be static or reserved".into()),
                }
            }
            _ => return Err(format!("unknown option {}", pair[0])),
        }
    }
    if !options.remainder().is_empty() {
        return Err("every --demo option requires a value".into());
    }
    Ok(Command::Demo {
        seed,
        preset,
        strategy,
    })
}
fn main() -> ExitCode {
    match execute() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("payment-routing: {error}");
            ExitCode::FAILURE
        }
    }
}
fn execute() -> Result<(), Box<dyn Error>> {
    match parse_args(&env::args().skip(1).collect::<Vec<_>>())? {
        Command::Help => {
            println!("{USAGE}");
            Ok(())
        }
        Command::Demo {
            seed,
            preset,
            strategy,
        } => {
            let mut app = App::new(preset, seed)?;
            app.strategy = strategy;
            if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
                return Err("--demo requires an interactive terminal on stdin and stdout; use --help for usage".into());
            }
            let result = match ratatui::try_init() {
                Ok(mut terminal) => run(&mut terminal, &mut app),
                Err(error) => Err(error),
            };
            let cleanup = ratatui::try_restore();
            result?;
            cleanup?;
            Ok(())
        }
    }
}
fn run(terminal: &mut DefaultTerminal, app: &mut App) -> io::Result<()> {
    let mut next_tick = Instant::now() + app.period();
    let mut next_draw = Instant::now();
    let mut dirty = true;
    loop {
        if app.running && Instant::now() >= next_tick {
            app.step();
            next_tick = Instant::now() + app.period();
        }
        if dirty || Instant::now() >= next_draw {
            terminal.draw(|frame| ui::draw(frame, app))?;
            next_draw = Instant::now() + Duration::from_millis(50);
            dirty = false;
        }
        // A paused console blocks until input/resize. No clock enters the model.
        let ready = if app.running {
            event::poll(
                next_tick
                    .min(next_draw)
                    .saturating_duration_since(Instant::now()),
            )?
        } else {
            true
        };
        if ready {
            let before = (app.running, app.speed, app.run().simulator.next_minute());
            if let Event::Key(key) = event::read()?
                && app.handle_key(key)
            {
                return Ok(());
            }
            if before != (app.running, app.speed, app.run().simulator.next_minute()) {
                next_tick = Instant::now() + app.period();
            }
            dirty = true;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arguments_are_validated_before_terminal_initialization() {
        assert_eq!(parse_args(&[]), Ok(Command::Help));
        assert_eq!(
            parse_args(&["--demo".into()]),
            Ok(Command::Demo {
                seed: 42,
                preset: Preset::Balanced,
                strategy: 1
            })
        );
        assert_eq!(
            parse_args(
                &[
                    "--demo",
                    "--seed",
                    "9",
                    "--scenario",
                    "outage",
                    "--strategy",
                    "static"
                ]
                .map(str::to_string)
            ),
            Ok(Command::Demo {
                seed: 9,
                preset: Preset::Outage,
                strategy: 0
            })
        );
        for args in [
            vec!["--demo", "--seed"],
            vec!["--demo", "--seed", "-1"],
            vec!["--demo", "--scenario", "bad"],
            vec!["--demo", "--strategy", "bad"],
            vec!["--demo", "--seed", "1", "--seed", "2"],
        ] {
            assert!(parse_args(&args.into_iter().map(str::to_string).collect::<Vec<_>>()).is_err());
        }
    }
}
