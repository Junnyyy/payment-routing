//! Paired disruption decisions and actual outcomes on identical original cohorts.
#[path = "../benchmarks/disruptions.rs"]
mod fixtures;
use payment_routing::simulation::*;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "case,policy,seed,eligible,planned,unplanned,remaining_fee_cents,remaining_elapsed_minutes,remaining_hops,churn,previously_planned,withdrawn,same_planned_cohort,original_completed,original_expired,original_actual_fee_cents,exact_complete_fee_cents"
    );
    for (name, c, seed) in fixtures::cases() {
        for (label, policy) in [
            ("preserve", ReoptimizationPolicy::Preserve),
            ("recompute", ReoptimizationPolicy::Recompute),
            ("adaptive-zero", ReoptimizationPolicy::default()),
            (
                "adaptive-one-hop",
                ReoptimizationPolicy::Adaptive {
                    max_extra_fee_cents: 0,
                    max_extra_elapsed_minutes: 0,
                    max_extra_hops: 1,
                },
            ),
            (
                "adaptive-2376c",
                ReoptimizationPolicy::Adaptive {
                    max_extra_fee_cents: 2376,
                    max_extra_elapsed_minutes: 0,
                    max_extra_hops: 0,
                },
            ),
        ] {
            let mut sim = Simulator::new(c.clone(), seed)?;
            sim.set_reoptimization_policy(policy);
            let first = sim.step()?;
            let cohort = sim.active_payments().to_vec();
            let last_id = sim.metrics().generated;
            let last_deadline = cohort.iter().map(|p| p.deadline).max().unwrap();
            let mut events = first.events;
            events.extend(sim.step()?.events);
            let r = sim.last_reoptimization().unwrap().clone();
            let exact = if cohort.len() <= 4 {
                match fixtures::exact_at_one(sim.effective_scenario(), &cohort)? {
                    Some(plan) => {
                        assert_eq!(r.recompute.planned, cohort.len());
                        assert_eq!(r.recompute.remaining_fee_cents, plan.total_fee_cents);
                        assert_eq!(
                            r.recompute.remaining_elapsed_minutes,
                            plan.total_elapsed_minutes
                        );
                        plan.total_fee_cents.to_string()
                    }
                    None => {
                        assert!(r.recompute.unplanned > 0);
                        "infeasible-complete-batch".into()
                    }
                }
            } else {
                "not-run".into()
            };
            while sim.next_minute() <= last_deadline {
                events.extend(sim.step()?.events);
            }
            let (mut done, mut expired, mut fee) = (0, 0, 0u128);
            for e in events {
                match e.kind {
                    EventKind::Completed {
                        sequence,
                        late: false,
                        ..
                    } if sequence <= last_id => done += 1,
                    EventKind::Expired { sequence } if sequence <= last_id => expired += 1,
                    EventKind::HopDeparted {
                        sequence,
                        fee_cents,
                        ..
                    } if sequence <= last_id => fee += u128::from(fee_cents),
                    _ => {}
                }
            }
            let a = r.selected_assessment();
            println!(
                "{name},{label},{seed},{},{},{},{},{},{},{},{},{},{},{done},{expired},{fee},{exact}",
                a.eligible_payments,
                a.planned,
                a.unplanned,
                a.remaining_fee_cents,
                a.remaining_elapsed_minutes,
                a.remaining_hops,
                a.changed_assignments,
                a.previously_planned,
                a.withdrawn,
                r.same_planned_cohort
            );
        }
    }
    Ok(())
}
