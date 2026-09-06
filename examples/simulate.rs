//! Headless simulator and replay demonstration. No terminal initialization.
use payment_routing::{demo::demo_network, simulation::*};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let ticks = args
        .next()
        .map(|s| s.parse::<u64>())
        .transpose()?
        .unwrap_or(10_000);
    let seed = args
        .next()
        .map(|s| s.parse::<u64>())
        .transpose()?
        .unwrap_or(42);
    let strategy = match args.next().as_deref() {
        None | Some("static") => RoutingStrategy::CheapestStatic,
        Some("reserved") => RoutingStrategy::Reserved {
            limits: Default::default(),
        },
        _ => return Err("strategy must be static or reserved".into()),
    };
    if args.next().is_some() {
        return Err(
            "usage: cargo run --locked --example simulate -- [ticks] [seed] [static|reserved]"
                .into(),
        );
    }
    let mut network = demo_network();
    // Explicit, accelerated synthetic timings for this simulation example only.
    // demo_network(), the static examples and the viewer keep their inputs.
    for rail in &mut network.rails {
        rail.settlement_minutes = match rail.id.as_str() {
            "ACH" => 5,
            "FEDWIRE" => 2,
            _ => 0,
        };
    }
    let flows = network
        .institutions
        .iter()
        .flat_map(|sender| {
            network
                .institutions
                .iter()
                .filter(|receiver| receiver.id != sender.id)
                .map(|receiver| PaymentFlow {
                    sender: sender.id.clone(),
                    receiver: receiver.id.clone(),
                })
        })
        .collect();
    let services = network
        .rails
        .iter()
        .map(|rail| {
            let (period, open, capacity) = match rail.id.as_str() {
                "ACH" => (8, 3, 200_000),
                "FEDWIRE" => (4, 3, 200_000),
                _ => (1, 1, 100_000),
            };
            RailService {
                rail_id: rail.id.clone(),
                period_minutes: period,
                offset_minutes: 0,
                open_minutes: open,
                capacity_per_minute_cents: Some(capacity),
            }
        })
        .collect();
    let scenario = Scenario {
        network,
        services,
        arrivals: ArrivalProcess {
            attempts_per_minute: 3,
            probability_per_million: 650_000,
            flows,
            min_amount_cents: 10_000,
            max_amount_cents: 150_000,
            min_sla_minutes: 0,
            max_sla_minutes: 12,
        },
        strategy,
        max_active_payments: 64,
        retained_events: 32,
    };
    let mut manual = Simulator::new(scenario, seed)?;
    let mut paced = manual.clone();
    let mut peak_active = 0;
    for minute in 0..ticks {
        // A host can schedule these calls at any speed. A paused driver consumes
        // no time/RNG; every actual step must match the manually advanced model.
        if minute % 7 == 0 {
            paced.pause();
            assert!(paced.tick()?.is_none());
        }
        paced.start();
        assert_eq!(manual.step()?, paced.tick()?.unwrap());
        peak_active = peak_active.max(manual.active_payments().len());
    }
    paced.pause();
    assert_eq!(manual, paced);
    manual.check_invariants()?;
    let metrics = manual.metrics();
    println!("Synthetic USD simulation; accelerated example timings; seed {seed}; {ticks} minutes");
    println!("Strategy: {strategy:?}");
    println!("Replay: every event and final state identical with manual vs paced/paused stepping");
    println!(
        "Generated: {}; completed: {}; expired: {}; rejected: {}; active: {}",
        metrics.generated,
        metrics.completed,
        metrics.expired,
        metrics.rejected,
        manual.active_payments().len()
    );
    println!(
        "Completed principal: {} cents; actual routing cost: {} cents",
        metrics.completed_volume_cents, metrics.routing_cost_cents
    );
    println!(
        "SLA failures: {}; late completions: {}; completed elapsed: {} minutes",
        metrics.sla_failures, metrics.completed_late, metrics.completed_elapsed_minutes
    );
    println!(
        "Events emitted: {}; retained: {}; peak active: {peak_active}/64",
        manual.event_count(),
        manual.recent_events().len()
    );
    for rail in manual.rail_states() {
        println!(
            "{}: departed {} cents; settled {} cents; fees {} cents",
            rail.rail_id,
            rail.departed_principal_cents,
            rail.settled_principal_cents,
            rail.routing_cost_cents
        );
    }
    let final_state = manual.clone();
    manual.restart(seed);
    manual.advance_ticks(ticks)?;
    assert_eq!(manual, final_state);
    println!("Restart: identical state after replay from seed");
    println!(
        "Routing diagnostics: {:?}; live reservation entries: {}",
        manual.routing_diagnostics(),
        manual.reservation_entries()
    );
    Ok(())
}
