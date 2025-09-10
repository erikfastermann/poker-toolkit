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
        x, legal_mask, target = self.dataset.get_action_item(idx)
        x = torch.tensor(x, dtype=torch.float32)
        legal_mask = torch.tensor(legal_mask, dtype=torch.int8)
        target = torch.tensor(target, dtype=torch.float32)
        return x, legal_mask, target


class ShowdownDataset(TorchDataset):
    def __init__(self, db_path, limit=None):
        self.dataset = InternalDataset(db_path, limit)

    def __len__(self):
        return self.dataset.total_showdowns_of_interest()

    def __getitem__(self, idx):
        x, legal_mask, target = self.dataset.get_showdown_item(idx)
        x = torch.tensor(x, dtype=torch.float32)
        legal_mask = torch.tensor(legal_mask, dtype=torch.int8)
        target = torch.tensor(target, dtype=torch.float32)
        return x, legal_mask, target

    def info(self, idx):
        return self.dataset.showdown_info(idx)


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
