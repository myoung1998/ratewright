# ratewright

Every service seems to write down its rate limits differently. Nginx
config uses `limit_req_zone ... rate=10r/s;`. API docs and the
draft `RateLimit-Limit` header describe the same thing as `10/1s` or
`600/60s`. When you're porting a limit from one system to another
(or just trying to check that two services actually agree on what
"reasonable" means) you end up doing the arithmetic by hand.

ratewright converts between the two notations:

- **shorthand** — nginx style, `<count>r/<unit>`, unit is `s`, `m`, `h`, or `d`.
  The unit can also carry a magnitude (`7r/2m`) — this isn't valid nginx
  config syntax, but it's how a window that isn't a whole multiple of a
  single unit round-trips without rounding the count
- **window** — `<count>/<seconds>[unit]`, e.g. `10/60s` or `10/60`

Both parse into the same internal `RateLimit { limit, window_secs }`,
so a value written either way converts losslessly.

It also understands **token-bucket descriptors** — `<shorthand-rate>;burst=<n>`,
e.g. `10r/s;burst=20`, matching nginx's `limit_req zone=...; burst=20;`.
A token bucket is mathematically the same thing as GCRA (the generic
cell rate algorithm used by redis-cell, Envoy, and similar limiters), so
`gcra_emission_interval` and `gcra_delay_variation_tolerance` convert one
to the other.

## Usage

As a library:

```rust
use ratewright::{parse_shorthand, format_window, requests_per_second};

let rate = parse_shorthand("600r/m").unwrap();
assert_eq!(format_window(&rate), "600/60s");
assert_eq!(requests_per_second(&rate), 10.0);
```

From the command line:

```
$ cargo run -- to-window 10r/s
10/1s

$ cargo run -- to-shorthand 600/60s
600r/m

$ cargo run -- to-shorthand 10/13s
10r/13s   # falls back to seconds since 13 isn't a clean minute/hour/day

$ cargo run -- gcra "10r/s;burst=20"
emission_interval=0.1s delay_variation_tolerance=2s
```

## Design

Every function in `src/lib.rs` is pure: given the same string input it
always returns the same result, with no clock, filesystem, or network
access. `main.rs` is the only place that touches argv or stdio. That
split is what makes the parsing and formatting logic trivial to unit
test (see the `tests` module in `lib.rs`) without any test harness
beyond the standard library.

## Status

Early skeleton. Handles shorthand, window, and token-bucket/GCRA
notation; a `--check` mode for comparing two specs is next.

## License

MIT, see `LICENSE`.
