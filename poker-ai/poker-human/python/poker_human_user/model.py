import torch
import torch.nn as nn
import torch.nn.functional as F

from poker_human import Dataset


class ActionHead(nn.Module):
    @classmethod
    def for_predict(cls, model_path):
        device = 'cuda' if torch.cuda.is_available() else 'cpu'

        checkpoint = torch.load(model_path, map_location=device)

        model = cls().to(device)
        model.load_state_dict(checkpoint)
        model.eval()

        return model

    def __init__(self):
        super().__init__()
        self.net = nn.Sequential(
            nn.Linear(Dataset.ACTION_INPUT_LEN, 4096), nn.ReLU(),
            nn.Linear(4096, 4096), nn.ReLU(),
            nn.Dropout(0.2),
            nn.Linear(4096, 4096), nn.ReLU(),
            nn.Dropout(0.2),
            nn.Linear(4096, Dataset.ACTION_TARGET_LEN)  # raw logits
        )

    def forward(self, x, legal_mask):
        logits = self.net(x)  # (B, n_actions)
        neg_inf = torch.finfo(logits.dtype).min
        masked_logits = torch.where(
            legal_mask.bool(), logits, torch.full_like(logits, neg_inf)
        )
        probs = F.softmax(masked_logits, dim=-1)
        return probs, masked_logits

    def predict(self, x, legal_mask):
        x = torch.tensor(x)
        legal_mask = torch.tensor(legal_mask)

        with torch.no_grad():
            probs, _ = self(x, legal_mask)

        return probs.cpu().numpy()


class ShowdownHead(nn.Module):
    @classmethod
    def for_predict(cls, model_path):
        device = 'cuda' if torch.cuda.is_available() else 'cpu'

        checkpoint = torch.load(model_path, map_location=device)

        model = cls().to(device)
        model.load_state_dict(checkpoint)
        model.eval()

        return model

    def __init__(self):
        super().__init__()
        self.net = nn.Sequential(
            nn.Linear(Dataset.SHOWDOWN_INPUT_LEN, 10_000), nn.ReLU(),
            nn.Linear(10_000, 10_000), nn.ReLU(),
            nn.Dropout(0.2),
            nn.Linear(10_000, 10_000), nn.ReLU(),
            nn.Dropout(0.2),
            nn.Linear(10_000, Dataset.SHOWDOWN_TARGET_LEN)  # raw logits
        )

    def forward(self, x, legal_mask):
        logits = self.net(x)  # (B, n_actions)
        neg_inf = torch.finfo(logits.dtype).min
        masked_logits = torch.where(
            legal_mask.bool(), logits, torch.full_like(logits, neg_inf)
        )
        probs = F.softmax(masked_logits, dim=-1)
        return probs, masked_logits

    def predict(self, x, legal_mask):
        x = torch.tensor(x)
        legal_mask = torch.tensor(legal_mask)

        with torch.no_grad():
            probs, _ = self(x, legal_mask)

        return probs.cpu().numpy()


class CEWithMask(nn.Module):
    def forward(self, masked_logits, target_idx):
        return F.cross_entropy(masked_logits, target_idx, reduction="mean")
