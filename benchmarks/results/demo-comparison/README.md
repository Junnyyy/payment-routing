# Untimed integration comparison

Captured on 2026-09-06 from the final implementation (functional code measured at
6d6a28f; reporting/comment-only checkpoint 63d9bc6). These are functional example
outputs, not benchmark timing observations. Commands:

```
cargo run --locked --example simulate -- 10000 42 reserved
cargo run --locked --example simulate -- 10000 42 static
```

Both generate the identical 19,520 arrival attempts admitted by the seeded process,
pass every-event paced/manual and restart replay checks, and leave six active
payments at the observation boundary. Reserved completes 12,409 and expires
7,105, versus 12,849 completed and 6,665 expired under CheapestStatic. Both have
zero late completions and zero admission rejections. Reserved actual fees are
3,242,640 cents versus 4,694,565 cents; these differently completed workloads do
not define an optimality gap. The reserved policy's peak active count is 18,
versus 15. Future capacity commitments and lower-fee waiting routes can harm later
arrivals; no claim of universally better online completion throughput is made.
