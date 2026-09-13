mod app;
mod evaluate_cli;
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

const USAGE: &str =
    "payment-routing --demo [--seed N] [--scenario NAME] [--strategy static|reserved]

Watch a synthetic payment network. Compare strategies. Inspect payments.
Starts paused; both strategies share demand. Requires an 80 x 18 terminal.

Run: cargo run --locked -- --demo
Scenarios: balanced, pressure, outage, limited, disruptions
Keys: Space run/pause; Tab watch/compare; Enter inspect; ? help; q quit

payment-routing --evaluate [--scenario all|NAME,...] [--seed N | --seeds N,...]
    [--strategies static,reserved,preserve,recompute,tight]
    [--ticks N] [--drain N] [--format text|csv|payments]

Headless paired evaluation with exact replay verification.
Defaults: all 9 worlds; seeds 0,1,42; static/reserved; 60 arrival + 60 drain minutes.
Censored or errored runs are unranked and exit nonzero.
Additional worlds: missed-connection, reservation-trap, disconnected, capacity-cliff
See docs/evaluation.md for metrics and reproduction.";

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Evaluate(evaluate_cli::Options),
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
    if args[0] == "--evaluate" {
        return evaluate_cli::parse(&args[1..]).map(Command::Evaluate);
    }
    if args[0] != "--demo" {
        return Err("expected --demo or --help (or --evaluate)".into());
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
                    .ok_or("scenario must be balanced, pressure, outage, limited or disruptions")?
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
        Command::Evaluate(options) => evaluate_cli::run(options),
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
    run_loop(
        app,
        |app| terminal.draw(|frame| ui::draw(frame, app)).map(|_| ()),
        event::poll,
        event::read,
        Instant::now,
        App::step,
    )
}

// Keep timing and I/O injectable so tests can verify the last frame before a
// blocking read without sleeping or changing terminal/simulation behavior.
fn run_loop(
    app: &mut App,
    mut draw: impl FnMut(&mut App) -> io::Result<()>,
    mut poll: impl FnMut(Duration) -> io::Result<bool>,
    mut read: impl FnMut() -> io::Result<Event>,
    mut now: impl FnMut() -> Instant,
    mut step: impl FnMut(&mut App),
) -> io::Result<()> {
    let mut next_tick = now() + app.period();
    let mut next_draw = now();
    let mut dirty = true;
    loop {
        if app.running && now() >= next_tick {
            step(app);
            // A failed tick pauses execution. Show the failure before the
            // paused path blocks on input, even between scheduled redraws.
            dirty |= !app.running;
            next_tick = now() + app.period();
        }
        if dirty || now() >= next_draw {
            draw(app)?;
            next_draw = now() + Duration::from_millis(50);
            dirty = false;
        }
        // A paused console blocks until input/resize. No clock enters the model.
        let ready = if app.running {
            poll(next_tick.min(next_draw).saturating_duration_since(now()))?
        } else {
            true
        };
        if ready {
            let before = (app.running, app.speed, app.run().simulator.next_minute());
            if let Event::Key(key) = read()?
                && app.handle_key(key)
            {
                return Ok(());
            }
            if before != (app.running, app.speed, app.run().simulator.next_minute()) {
                next_tick = now() + app.period();
            }
            dirty = true;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_error_is_drawn_before_paused_read_even_between_refreshes() {
        use ratatui::{
            Terminal,
            backend::TestBackend,
            crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
        };
        use std::cell::{Cell, RefCell};
        let mut app = App::new(Preset::Balanced, 42).unwrap();
        app.running = true;
        app.speed = App::SPEEDS.len() - 1; // 10ms ticks, 50ms redraws.
        let clock = Cell::new(Instant::now());
        let ticks = Cell::new(0);
        let frames = RefCell::new(Vec::<String>::new());
        let mut terminal = Terminal::new(TestBackend::new(80, 18)).unwrap();
        run_loop(
            &mut app,
            |app| {
                terminal.draw(|frame| ui::draw(frame, app)).unwrap();
                frames.borrow_mut().push(
                    terminal
                        .backend()
                        .buffer()
                        .content
                        .iter()
                        .map(|cell| cell.symbol())
                        .collect(),
                );
                Ok(())
            },
            |timeout| {
                assert!(ticks.get() < 2, "failed tick must return to paused input");
                clock.set(clock.get() + timeout);
                Ok(false)
            },
            || {
                let frames = frames.borrow();
                assert!(frames[0].contains("Running"));
                assert!(
                    frames
                        .last()
                        .unwrap()
                        .contains("Error: injected tick failure"),
                    "error hidden at blocking read: {}",
                    frames.last().unwrap()
                );
                assert!(!frames.last().unwrap().contains("Running"));
                Ok(Event::Key(KeyEvent::new(
                    KeyCode::Char('q'),
                    KeyModifiers::NONE,
                )))
            },
            || clock.get(),
            |app| {
                ticks.set(ticks.get() + 1);
                if ticks.get() == 2 {
                    // Inject the same transition as a transactional tick error.
                    app.running = false;
                    app.error = Some("injected tick failure".into());
                } else {
                    app.step();
                }
            },
        )
        .unwrap();
        assert_eq!(ticks.get(), 2);
        assert_eq!(
            frames.borrow().len(),
            2,
            "successful ticks retain the draw throttle"
        );
        assert_eq!(app.run().simulator.next_minute(), 1);
    }

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
