mod app;
mod ui;

use std::{
    env,
    error::Error,
    io::{self, IsTerminal},
    process::ExitCode,
};

use payment_routing::demo::demo_network;
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event},
};

use app::App;

const USAGE: &str = "payment-routing --demo\n\nLoad the deterministic synthetic USD payment network.\nRequires an interactive terminal of at least 80 columns by 18 rows.\n\nUsage: cargo run --locked -- --demo\n       cargo run --locked -- --help\n\nKeys: Tab / Shift-Tab or Left / Right: views; 1-4: jump\n      Up / Down or j / k: rows; Home / End: first / last\n      q / Esc / Ctrl-C: quit";

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Demo,
    Help,
}

fn parse_args(args: &[String]) -> Result<Command, String> {
    match args {
        [] => Ok(Command::Help),
        [arg] if arg == "--help" || arg == "-h" => Ok(Command::Help),
        [arg] if arg == "--demo" => Ok(Command::Demo),
        _ => Err("expected --demo or --help".into()),
    }
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
        Command::Demo => {
            let network = demo_network();
            network.validate()?;
            if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
                return Err("--demo requires an interactive terminal on stdin and stdout; use --help for usage".into());
            }
            let mut app = App::new(network);
            // try_init installs Ratatui's restoration hook for panics. Always clean up
            // partial initialization and ordinary draw/read errors as well.
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
    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;
        // Blocking reads keep the static explorer idle until input or a resize.
        if let Event::Key(key) = event::read()?
            && app.handle_key(key)
        {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_accepts_demo_and_help_and_rejects_unexpected_arguments() {
        assert_eq!(parse_args(&[]), Ok(Command::Help));
        assert_eq!(parse_args(&["--help".into()]), Ok(Command::Help));
        assert_eq!(parse_args(&["-h".into()]), Ok(Command::Help));
        assert_eq!(parse_args(&["--demo".into()]), Ok(Command::Demo));
        assert!(parse_args(&["--dem".into()]).is_err());
        assert!(parse_args(&["--demo".into(), "unexpected".into()]).is_err());
    }
}
