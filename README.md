# Poker equity calculator

Calculate the equity, win- and tie-percentage of a given hand in Texas Hold'em.

TODO: Add GUI usage.

## Usage

### Enumerate

Calculates the equity for all card combinations
with the given community cards and player ranges.
Only useful if not that many combinations are possible.
E.g.:

```
cargo run --release -- enumerate AsTd3h      AhTh   AKo+,AKs+,TT+,33 full
#                                ^           ^      ^                ^
#                                community   hero   villain 1        villain 2 ...
# Output:
# player 1: equity=72.80 win=72.58 tie=0.22
# player 2: equity=21.60 win=21.47 tie=0.13
# player 3: equity=5.60 win=5.36 tie=0.23
```

### Simulate

Calculate the equity via Monte Carlo simulation
with the given community cards, player ranges
and number of rounds (use at least 100000 for reasonable results).
Not exact, but usually close enough. With more players
and larger ranges, the precision decreases.
E.g.:

```
cargo run --release -- simulate  1000000 AsTd3h      AhTh   AKo+,AKs+,TT+,33 full        full
#                                ^       ^           ^      ^                ^           ^
#                                rounds  community   hero   villain 1        villain 2   villain 3 ...
# Output:
# player 1: equity=68.88 win=68.52 tie=0.36
# player 2: equity=20.42 win=20.22 tie=0.20
# player 3: equity=5.34 win=5.02 tie=0.31
# player 4: equity=5.36 win=5.05 tie=0.32
```
