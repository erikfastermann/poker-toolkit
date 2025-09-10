from concurrent.futures import ProcessPoolExecutor
import pickle
from poker_human_user import ShowdownDataset
from tqdm.contrib.concurrent import process_map


def get_entry(index):
    try:
        x, legal_mask, target = dataset[index]
    except Exception as e:
        print(f'Skipping {index}: {e}')
        return None

    hand_name, info = dataset.info(index)

    entry = {
        'hand_name': hand_name,
        'info': info,
        'x': x.cpu().numpy(),
        'legal_mask': legal_mask.cpu().numpy(),
        'target': target.cpu().numpy(),
    }

    return entry


if __name__ == '__main__':
    db_path = '../../poker-app/phh_full.db'
    limit = 1000
    out_path = 'showdown.pkl'
    max_workers = 10


    dataset = ShowdownDataset(db_path, limit)

    out = process_map(get_entry, range(len(dataset)), max_workers=max_workers, chunksize=1)

    out = [entry for entry in out if entry is not None]

    with open(out_path, 'wb') as out_file:
        pickle.dump(out, out_file)
