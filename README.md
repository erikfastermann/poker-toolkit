# Poker equity calculator

Calculate the equity, win- and tie-percentage of a given hand in Texas Hold'em.

## Usage

### Enumerate

Calculates the equity for all card combinations
with the given community cards, hero hand and villain ranges.
Only useful if not that many combinations are possible.
E.g.:

```
cargo run --release -- enumerate AsTd3h      AhTh   AKo+,AKs+,TT+,33 full
#                                ^           ^      ^                ^
#                                community   hero   villain 1        villain 2 ...
# Output:
# hero:      equity=72.80 win=72.58 tie=0.22
# villain 1: equity=21.60 win=21.47 tie=0.13
# villain 2: equity=5.60 win=5.36 tie=0.23
```

### Simulate

TODO: Update for ranges.

Calculate the equity via Monte Carlo simulation
with the given community cards, hero hand, villain count
and number of rounds.
Not exact, but usually close enough (with 1000000+ rounds
about a 0.1% difference should be expected).
Villain ranges are currently not supported.
E.g.:

```
cargo run --release -- simulate  AsTd3h      AhTh   2               1000000
#                                ^           ^      ^               ^
#                                community   hero   villain count   rounds
# Output:
# hero:      equity=87.96 win=87.50 tie=0.46
# villain 1: equity=6.02 win=5.68 tie=0.34
# villain 2: equity=6.02 win=5.68 tie=0.34
```
