import pickle
import torch
from torch.utils.data import Dataset as TorchDataset

from poker_human import Dataset as InternalDataset


class ActionDataset(TorchDataset):
    def __init__(self, db_path, limit=None):
        self.dataset = InternalDataset(db_path, limit)

    def __len__(self):
        return self.dataset.total_actions_of_interest()

    def __getitem__(self, idx):
        try:
            x, legal_mask, target = self.dataset.get_action_item(idx)
        except Exception as e:
            print(f'Error at index {idx}: {self.info(idx)}')
            raise e

        x = torch.tensor(x, dtype=torch.float32)
        legal_mask = torch.tensor(legal_mask, dtype=torch.int8)
        target = torch.tensor(target, dtype=torch.float32)
        return x, legal_mask, target

    def info(self, idx):
        return self.dataset.action_info(idx)

    def frequencies(self, idx):
        return self.dataset.action_range_info_frequencies(idx)

    def state_probability(self, idx):
        return self.dataset.action_state_probability(idx)


class ShowdownDataset(TorchDataset):
    def __init__(self, db_path, limit=None):
        self.dataset = InternalDataset(db_path, limit)

    def __len__(self):
        return self.dataset.total_showdowns_of_interest()

    def __getitem__(self, idx):
        try:
            x, legal_mask, target = self.dataset.get_showdown_item(idx)
        except Exception as e:
            print(f'Error at index {idx}: {self.info(idx)}')
            raise e

        x = torch.tensor(x, dtype=torch.float32)
        legal_mask = torch.tensor(legal_mask, dtype=torch.int8)
        target = torch.tensor(target, dtype=torch.float32)
        return x, legal_mask, target

    def info(self, idx):
        return self.dataset.showdown_info(idx)

    def frequencies(self, idx):
        return self.dataset.showdown_range_info(idx)

    def state_probability(self, idx):
        return self.dataset.showdown_state_probability(idx)


class ShowdownPreprocessedDataset(TorchDataset):
    def __init__(self, preprocessed_path):
        with open(preprocessed_path, 'rb') as f:
            self.data = pickle.load(f)

    def __len__(self):
        return len(self.data)

    def __getitem__(self, idx):
        entry = self.data[idx]
        x = torch.tensor(entry['x'], dtype=torch.float32)
        legal_mask = torch.tensor(entry['legal_mask'], dtype=torch.int8)
        target = torch.tensor(entry['target'], dtype=torch.float32)
        return x, legal_mask, target

    def info(self, idx):
        entry = self.data[idx]
        return entry['hand_name'], entry['info']
