import numpy as np
import matplotlib.pyplot as plt
from matplotlib import rcParams
import pickle
import json
import copy


plt.style.use('seaborn-v0_8-paper')
rcParams.update({
    "font.size": 12,
    "axes.labelsize": 13,
    "axes.titlesize": 14,
    "legend.fontsize": 11,
    "xtick.labelsize": 11,
    "ytick.labelsize": 11,
    "axes.grid": True,
    "grid.linestyle": "--",
    "grid.alpha": 0.6,
    "lines.linewidth": 2,
})


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
    plt.close()


def rarity(name, info, values, probs):
    plt.figure(figsize=(10, 7))
    plt.scatter(values, probs, alpha=0.5, s=25, color='royalblue', label='Samples')

    plt.yscale('log')

    log_probs = np.log10(probs)
    z = np.polyfit(values, log_probs, 1)
    p = np.poly1d(z)
    x_range = np.linspace(min(values), max(values), 100)

    plt.title(info['title'])
    plt.xlabel(info['x_label'])
    plt.ylabel(info['y_label'])
    plt.grid(True, which="both", ls="-", alpha=0.2)
    plt.legend()

    plt.savefig(f'dump_{name}_rarity_analysis.png')
    plt.close()


def total_variation_distance(a, b):
    return 0.5 * np.sum(np.abs(a - b))


def mean_squared_error(a, b):
    return np.mean(np.abs(a - b))


def evaluate_single(name, info, data):
    tvd_values = [total_variation_distance(e['expected'], e['got']) for e in data]
    state_probabilities = [e['prob'] for e in data]

    histogram(name, info['histogram'], tvd_values)
    rarity(name, info['rarity'], tvd_values, state_probabilities)


def evaluate_action(data):
    info = {
        'histogram': {
            'title': 'Action Model Evaluation - Histogram',
            'x_label': 'Total Variation Distance (Lower is better)',
            'y_label': 'Frequency in Percent',
        },
        'rarity': {
            'title': 'Action Model Evaluation - Sample Rarity',
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


def swap_entry_got_uniform(entry):
    entry = copy.deepcopy(entry)
    length = len(entry['got'])
    entry['got'] = np.full(length, 1 / length)
    return entry


def evaluate_showdown(range_data, equity_data):
    info = {
        'histogram': {
            'title': 'Showdown Model Evaluation - Histogram',
            'x_label': 'Total Variation Distance (Lower is better)',
            'y_label': 'Frequency in Percent',
        },
        'rarity': {
            'title': 'Showdown Model Evaluation - Sample Rarity',
            'x_label': 'Total Variation Distance (Lower is better)',
            'y_label': 'Occurrence Probability (Log Scale)',
        },
    }

    evaluate_single('showdown', info, range_data)

    mse_histogram_info = {
        'title': 'Showdown Equities Evaluation - Model',
        'x_label': 'Mean Squared Error (Lower is better)',
        'y_label': 'Frequency in Percent',
    }

    mse_histogram_uniform_info = {
        'title': 'Showdown Equities Evaluation - Uniform Range',
        'x_label': 'Mean Squared Error (Lower is better)',
        'y_label': 'Frequency in Percent',
    }

    mse_values = [mean_squared_error(e['expected'], e['got']) for e in equity_data]
    mse_uniform_values = [mean_squared_error(e['expected'], e['uniform']) for e in equity_data]

    histogram('showdown_mse', mse_histogram_info, mse_values)
    histogram('showdown_mse_uniform', mse_histogram_uniform_info, mse_uniform_values)


if __name__ == '__main__':
    action_data_path = 'dump_action_eval.pkl'
    showdown_data_path = 'dump_showdown_eval.pkl'
    equity_data_path = 'dump_showdown_equities.json'


    action_data = load_pickle(action_data_path)
    showdown_data = load_pickle(showdown_data_path)

    # Assumes consistent insertion order of range dicts.
    equity_data = list(preprocess_equity_data(load_json(equity_data_path)))
    assert len(equity_data) == len(showdown_data)

    print('Action:', len(action_data))
    print('Showdown:', len(showdown_data))

    evaluate_action(action_data)
    evaluate_showdown(showdown_data, equity_data)
