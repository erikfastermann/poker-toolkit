import torch
from torch.utils.data import Dataset as TorchDataset

from .poker_human import Dataset as InternalDataset


class Dataset(TorchDataset):
    def __init__(self, db_path, limit=None):
        self.dataset = InternalDataset(db_path, limit)
        self.in_dim = self.dataset.ACTION_INPUT_LEN
        self.n_actions = self.dataset.ACTION_TARGET_LEN

    def __len__(self):
        return self.dataset.total_actions_of_interest()

    def __getitem__(self, idx):
        x, legal_mask, target = self.dataset.get_action_item(idx)
        x = torch.tensor(x, dtype=torch.float32)
        legal_mask = torch.tensor(legal_mask, dtype=torch.int8)
        target = torch.tensor(target, dtype=torch.float32)
        return x, legal_mask, target
