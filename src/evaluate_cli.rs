use payment_routing::evaluation::{self, Config, Strategy, scenarios};
use std::{
    collections::BTreeSet,
    error::Error,
    io::{self, Write},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    worlds: Vec<String>,
    seeds: Vec<u64>,
    strategies: Vec<String>,
    config: Config,
    format: Format,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Text,
    Csv,
    Payments,
}

pub fn parse(args: &[String]) -> Result<Options, String> {
    let mut result = Options {
        worlds: scenarios::NAMES.iter().map(|s| (*s).into()).collect(),
        seeds: vec![0, 1, 42],
        strategies: vec!["static".into(), "reserved".into()],
        config: Config::default(),
        format: Format::Text,
    };
    let mut seen = BTreeSet::new();
    let mut pairs = args.chunks_exact(2);
    for pair in &mut pairs {
        let key = if pair[0] == "--seed" {
            "--seeds"
        } else {
            &pair[0]
        };
        if !seen.insert(key) {
            return Err(format!("duplicate {key}"));
        }
        match key {
            "--seeds" => {
                result.seeds = pair[1]
                    .split(',')
                    .map(|s| {
                        s.parse::<u64>()
                            .map_err(|_| "seeds must be comma-separated u64 integers")
                    })
                    .collect::<Result<_, _>>()?;
                if result.seeds.iter().collect::<BTreeSet<_>>().len() != result.seeds.len() {
                    return Err("duplicate seed".into());
                }
            }
            "--scenario" => {
                result.worlds = if pair[1] == "all" {
                    scenarios::NAMES.iter().map(|s| (*s).into()).collect()
                } else {
                    pair[1].split(',').map(str::to_string).collect()
                };
                if result
                    .worlds
                    .iter()
                    .any(|name| !scenarios::NAMES.contains(&name.as_str()))
                {
                    return Err(format!(
                        "scenario must be all or comma-separated names: {}",
                        scenarios::NAMES.join(",")
                    ));
                }
                if result.worlds.iter().collect::<BTreeSet<_>>().len() != result.worlds.len() {
                    return Err("duplicate scenario".into());
                }
            }
            "--strategies" => {
                result.strategies = pair[1].split(',').map(str::to_string).collect();
                if result
                    .strategies
                    .iter()
                    .any(|s| Strategy::named(s).is_none())
                {
                    return Err("strategies must be comma-separated static,reserved,preserve,recompute,tight".into());
                }
                if result.strategies.iter().collect::<BTreeSet<_>>().len()
                    != result.strategies.len()
                {
                    return Err("duplicate strategy".into());
                }
            }
            "--ticks" => {
                result.config.arrival_minutes = pair[1]
                    .parse()
                    .map_err(|_| "ticks must be a positive u64 integer")?;
                if result.config.arrival_minutes == 0 {
                    return Err("ticks must be positive".into());
                }
            }
            "--drain" => {
                result.config.drain_minutes =
                    pair[1].parse().map_err(|_| "drain must be a u64 integer")?
            }
            "--format" => {
                result.format = match pair[1].as_str() {
                    "text" => Format::Text,
                    "csv" => Format::Csv,
                    "payments" => Format::Payments,
                    _ => return Err("format must be text, csv or payments".into()),
                }
            }
            _ => return Err(format!("unknown evaluation option {}", pair[0])),
        }
    }
    if !pairs.remainder().is_empty() {
        return Err("every --evaluate option requires a value".into());
    }
    Ok(result)
}

pub fn run(options: Options) -> Result<(), Box<dyn Error>> {
    let worlds = options
        .worlds
        .iter()
        .map(|s| scenarios::named(s).unwrap())
        .collect::<Vec<_>>();
    let strategies = options
        .strategies
        .iter()
        .map(|s| Strategy::named(s).unwrap())
        .collect::<Vec<_>>();
    let result = evaluation::evaluate(&worlds, &options.seeds, &strategies, options.config)?;
    let output = match options.format {
        Format::Text => result.to_text()?,
        Format::Csv => result.to_csv()?,
        Format::Payments => result.payments_csv(),
    };
    io::stdout().lock().write_all(output.as_bytes())?;
    if result.has_incomplete_runs() {
        return Err("evaluation contains censored or errored runs; see output; increase --drain for pending work".into());
    }
    Ok(())
}
