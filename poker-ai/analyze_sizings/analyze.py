import json
from pathlib import Path
import matplotlib.pyplot as plt
import numpy as np
from sklearn.cluster import KMeans

DIST_JSON_PATH = 'dist.json'

def simplify_bet_raise(bet_raise):
    out = {}

    for p, c in bet_raise.items():
        p = int(p) // 5

        if p not in out:
            out[p] = 0

        out[p] += c

    return sorted(((p+1)*5 - 2.5, c) for p, c in out.items())

def merge_all_bet_raise(dist, street_range=range(0, 1_000_000)):
    out = {}

    for street in dist:
        for i in street_range:
            if i >= len(street):
                break

            _, bet_raise = street[i]

            for p, c in bet_raise.items():
                p = int(p)

                if p not in out:
                    out[p] = 0

                out[p] += c

    return sorted(out.items())

def kmeans(merged, n_clusters=10, max_p=300):
    if len(merged) == 0:
        return

    percentages = np.array([p for p, _ in merged if p < max_p]).reshape(-1, 1)
    weights = np.array([c for p, c in merged if p < max_p])

    kmeans = KMeans(n_clusters=n_clusters, random_state=0)
    kmeans.fit(percentages, sample_weight=weights)

    centers = sorted(kmeans.cluster_centers_.flatten())
    print("Cluster centers:", centers)

if __name__ == "__main__":
    dist = json.loads(Path(DIST_JSON_PATH).read_text())

    print('Total:')
    kmeans(merge_all_bet_raise(dist))

    print()

    for i in range(4):
        print(f'Street {i}:')
        kmeans(merge_all_bet_raise(dist[i:i+1]))

        print('Bets only:')
        kmeans(merge_all_bet_raise(dist[i:i+1], street_range=range(1)))

        print('Raises only:')
        kmeans(merge_all_bet_raise(dist[i:i+1], street_range=range(1, 1_000_000)))

        print()

    print('Post Flop only:')
    kmeans(merge_all_bet_raise(dist[1:]))

    print('Post Flop Bets only:')
    kmeans(merge_all_bet_raise(dist[1:], street_range=range(1)))

    print('Post Flop Raises only:')
    kmeans(merge_all_bet_raise(dist[1:], street_range=range(1, 1_000_000)))

    print()

    print('Streets and bets')
    print([(len(street), [c for c, _ in street]) for street in dist])

    exit()

    # ---

    count, bet_raise = dist[1][1]

    x, y = zip(*[(p, c) for p, c in simplify_bet_raise(bet_raise) if p < 200])

    plt.figure(figsize=(6,4))
    plt.scatter(x, y, marker='o')

    plt.xlabel("%Pot")
    plt.ylabel("Frequency")
    plt.title(f"Bet/Raise ({count})")

    plt.show()
