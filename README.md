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

Calculate the equity via Monte Carlo simulation
with the given community cards, hero hand, villain ranges
and number of rounds (use at least 100000 for reasonable results).
Not exact, but usually close enough. With more villains
and larger ranges, the precision decreases.
E.g.:

```
cargo run --release -- simulate  1000000 AsTd3h      AhTh   AKo+,AKs+,TT+,33 full        full
#                                ^       ^           ^      ^                ^           ^
#                                rounds  community   hero   villain 1        villain 2   villain 3 ...
# Output:
# hero:      equity=68.82 win=68.47 tie=0.35
# villain 1: equity=20.48 win=20.29 tie=0.19
# villain 2: equity=5.37 win=5.05 tie=0.31
# villain 3: equity=5.33 win=5.01 tie=0.32
```
