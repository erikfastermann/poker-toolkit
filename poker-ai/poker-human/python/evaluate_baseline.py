import torch
from torch.utils.data import random_split
import numpy as np
from tqdm import tqdm
import pickle

from poker_human_user import ActionDataset, ActionHead, ShowdownDataset, ShowdownHead


def dump_results_pickle(name, data):
    with open(f'dump_{name}_eval.pkl', 'wb') as out_file:
        pickle.dump(data, out_file)


def evaluate(name, dataset, model):
    train, val, test = random_split(
        dataset,
        [0.98, 0.01, 0.01],
        generator=torch.Generator().manual_seed(42), # deterministic
    )

    data = []

    for idx in tqdm(test.indices):
        try:
            x, legal_mask, _ = dataset[idx]
        except Exception as e:
            print(f'Skipping {idx}: {e}')
            continue

        expected = dataset.frequencies(idx)
        got = model.predict(x, legal_mask)

        prob = dataset.state_probability(idx)
        street = dataset.street(idx)
        board = dataset.board(idx)
        hands = dataset.hands(idx)
        actions = dataset.actions(idx)
        info = dataset.info(idx)

        entry = {
            'idx': idx,
            'expected': np.array(expected),
            'got': np.array(got),
            'prob': prob,
            'street': street,
            'board': board,
            'hands': hands,
            'actions': actions,
            'info': info,
        }

        data.append(entry)

    dump_results_pickle(name, data)


if __name__ == "__main__":
    db_path = "../generate-hands/equity.db"
    limit = None
    action_model_path = "action_baseline.pt"
    showdown_model_path = "showdown_baseline.pt"


    evaluate(
        "action",
        ActionDataset(db_path, limit=limit),
        ActionHead.for_predict(action_model_path),
    )

    evaluate(
        "showdown",
        ShowdownDataset(db_path, limit=limit),
        ShowdownHead.for_predict(showdown_model_path),
    )
