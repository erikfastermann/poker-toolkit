import torch
import torch.nn as nn
import torch.nn.functional as F

from .poker_human import Dataset


class ActionHead(nn.Module):
    def __init__(self):
        super().__init__()
        self.net = nn.Sequential(
            nn.Linear(Dataset.ACTION_INPUT_LEN, 1024), nn.ReLU(),
            nn.Linear(1024, 1024), nn.ReLU(),
            nn.Dropout(0.2),
            nn.Linear(1024, 1024), nn.ReLU(),
            nn.Dropout(0.2),
            nn.Linear(1024, Dataset.ACTION_TARGET_LEN)  # raw logits
        )

    def forward(self, x, legal_mask):
        logits = self.net(x)  # (B, n_actions)
        neg_inf = torch.finfo(logits.dtype).min
        masked_logits = torch.where(
            legal_mask.bool(), logits, torch.full_like(logits, neg_inf)
        )
        probs = F.softmax(masked_logits, dim=-1)
        return probs, masked_logits


class ShowdownHead(nn.Module):
    def __init__(self):
        super().__init__()
        self.net = nn.Sequential(
            nn.Linear(Dataset.SHOWDOWN_INPUT_LEN, 2048), nn.ReLU(),
            nn.Linear(2048, 2048), nn.ReLU(),
            nn.Dropout(0.2),
            nn.Linear(2048, 2048), nn.ReLU(),
            nn.Dropout(0.2),
            nn.Linear(2048, Dataset.SHOWDOWN_TARGET_LEN)  # raw logits
        )

    def forward(self, x):
        return self.net(x)


class CEWithMask(nn.Module):
    def forward(self, masked_logits, target_idx):
        return F.cross_entropy(masked_logits, target_idx, reduction="mean")


def masked_bce_with_logits_loss(logits, targets, mask):
    loss_fn = nn.BCEWithLogitsLoss(reduction="none")
    loss = loss_fn(logits, targets)  # (batch, n_classes)
    masked_loss = (loss * mask).sum() / mask.sum()
    return masked_loss
