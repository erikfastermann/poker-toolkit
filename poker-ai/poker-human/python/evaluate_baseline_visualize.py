import numpy as np
import matplotlib.pyplot as plt
import pickle
import json
import copy


def load_pickle(file_path):
    with open(file_path, 'rb') as f:
        return pickle.load(f)


def load_json(file_path):
    with open(file_path, 'rb') as f:
        return json.load(f)


def histogram(name, info, values):
    mean_val = np.mean(values)
    median_val = np.median(values)
    p95_val = np.percentile(values, 95)

    weights = np.ones_like(values) / len(values) * 100

    plt.figure(figsize=(10, 6))
    plt.hist(values, bins=30, weights=weights, color='skyblue', edgecolor='black', alpha=0.7)

    plt.axvline(mean_val, color='red', linestyle='--', label=f'Mean: {mean_val:.3f}')
    plt.axvline(median_val, color='green', linestyle=':', label=f'Median: {median_val:.3f}')
    plt.axvline(p95_val, color='orange', linestyle='-.', label=f'95th Percentile: {p95_val:.3f}')

    plt.title(info['title'])
    plt.xlabel(info['x_label'])
    plt.ylabel(info['y_label'])
    plt.legend()
    plt.grid(axis='y', alpha=0.3)

    plt.savefig(f'dump_{name}_histogram.png')


def rarity(name, info, values, probs):
    plt.figure(figsize=(10, 7))
    plt.scatter(values, probs, alpha=0.5, s=25, color='royalblue', label='Samples')

    plt.yscale('log')

    log_probs = np.log10(probs)
    z = np.polyfit(values, log_probs, 1)
    p = np.poly1d(z)
    x_range = np.linspace(min(values), max(values), 100)
    plt.plot(x_range, 10**p(x_range), "r--", linewidth=2, label='Log-Linear Trend')

    plt.title(info['title'])
    plt.xlabel(info['x_label'])
    plt.ylabel(info['y_label'])
    plt.grid(True, which="both", ls="-", alpha=0.2)
    plt.legend()

    plt.savefig(f'dump_{name}_rarity_analysis.png')


def evaluate_single(name, info, data):
    tvd_values = [0.5 * np.sum(np.abs(e['expected'] - e['got'])) for e in data]
    state_probabilities = [e['prob'] for e in data]

    histogram(name, info['histogram'], tvd_values)
    rarity(name, info['rarity'], tvd_values, state_probabilities)


def evaluate_action(data):
    info = {
        'histogram': {
            'title': 'Distribution of Total Variation Distance (TVD) for the Action Model',
            'x_label': 'Total Variation Distance (Lower is better)',
            'y_label': 'Frequency in Percent',
        },
        'rarity': {
            'title': 'Relationship between Sample Rarity and TVD for the Action Model',
            'x_label': 'Total Variation Distance (Lower is better)',
            'y_label': 'Occurrence Probability (Log Scale)',
        },
    }

    evaluate_single('action', info, data)

    streets = sorted(set(e['street'] for e in data))
    for street in streets:
        per_street = [e for e in data if e['street'] == street]
        current_name = f'action_{street.lower()}'

        current_info = copy.deepcopy(info)
        current_info['rarity']['title'] += f' ({street})'
        current_info['histogram']['title'] += f' ({street})'

        evaluate_single(current_name, current_info, per_street)


def convert_equities(equities):
    return np.fromiter((v / 10_000 for v in equities.values()), float)


def preprocess_equity_data(equity_data):
    for expected, got, uniform in equity_data.values():
        yield {
            'expected': convert_equities(expected),
            'got': convert_equities(got),
            'uniform': convert_equities(uniform),
        }


def evaluate_showdown(range_data, equity_data):
    info = {
        'histogram': {
            'title': 'Distribution of Total Variation Distance (TVD) for the Showdown Model',
            'x_label': 'Total Variation Distance (Lower is better)',
            'y_label': 'Frequency in Percent',
        },
        'rarity': {
            'title': 'Relationship between Sample Rarity and TVD for the Showdown Model',
            'x_label': 'Total Variation Distance (Lower is better)',
            'y_label': 'Occurrence Probability (Log Scale)',
        },
    }

    evaluate_single('showdown', info, range_data)

    mae_histogram_info = {
        'title': 'Distribution of Mean Absolute Error (MAE) for the Showdown Model Equities',
        'x_label': 'Mean Absolute Error (Lower is better)',
        'y_label': 'Frequency in Percent',
    }

    mae_histogram_uniform_info = {
        'title': 'Distribution of Mean Absolute Error (MAE) for the Uniform Range Showdown Equities',
        'x_label': 'Mean Absolute Error (Lower is better)',
        'y_label': 'Frequency in Percent',
    }

    mae_values = [np.sum(np.abs(e['expected'] - e['got'])) / 1326 for e in equity_data]
    mae_uniform_values = [np.sum(np.abs(e['expected'] - e['uniform'])) / 1326 for e in equity_data]

    histogram('showdown_mae', mae_histogram_info, mae_values)
    histogram('showdown_mae_uniform', mae_histogram_uniform_info, mae_uniform_values)


if __name__ == '__main__':
    action_data_path = 'dump_action_eval_full.pkl'
    showdown_data_path = 'dump_showdown_eval_full.pkl'
    equity_data_path = 'dump_showdown_equities_full.json'

    action_data = load_pickle(action_data_path)
    showdown_data = load_pickle(showdown_data_path)

    # Assumes consistent insertion order of range dicts.
    equity_data = list(preprocess_equity_data(load_json(equity_data_path)))

    evaluate_action(action_data)
    evaluate_showdown(showdown_data, equity_data)
