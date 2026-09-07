//! Reproduce the console's twin scenarios without terminal initialization.
use payment_routing::operations::{Operations, Preset};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let ticks = args
        .next()
        .map(|s| s.parse::<u64>())
        .transpose()?
        .unwrap_or(80);
    let seed = args
        .next()
        .map(|s| s.parse::<u64>())
        .transpose()?
        .unwrap_or(42);
    if args.next().is_some() {
        return Err("usage: --example operations -- [ticks] [seed]".into());
    }
    println!("Synthetic USD; accelerated scenarios; seed {seed}; {ticks} ticks; fees in cents");
    println!(
        "scenario,strategy,generated,completed,late,expired,rejected,active,sla_failures,fees,completed_cents,truncated,rail_changes,churn,assignment_comparisons"
    );
    for preset in Preset::ALL {
        let mut ops = Operations::new(preset, seed)?;
        // Verify the observation session against ordinary terminal-free simulators.
        let mut plain = [ops.runs[0].simulator.clone(), ops.runs[1].simulator.clone()];
        for _ in 0..ticks {
            ops.step()?;
            for (run, unobserved) in ops.runs.iter().zip(&mut plain) {
                unobserved.step()?;
                assert_eq!(&run.simulator, unobserved);
            }
        }
        for (i, run) in ops.runs.iter().enumerate() {
            run.simulator.check_invariants()?;
            let m = run.simulator.metrics();
            println!(
                "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                preset.name(),
                if i == 0 { "static" } else { "reserved" },
                m.generated,
                m.completed,
                m.completed_late,
                m.expired,
                m.rejected,
                run.simulator.active_payments().len(),
                m.sla_failures,
                m.routing_cost_cents,
                m.completed_volume_cents,
                run.simulator.routing_diagnostics().truncated_searches,
                run.simulator.adaptation_metrics().rail_changes,
                run.simulator.adaptation_metrics().changed_assignments,
                run.simulator.adaptation_metrics().assignment_comparisons
            );
        }
    }
    println!("Verified: both observed runs match ordinary simulators after every tick.");
    Ok(())
}
