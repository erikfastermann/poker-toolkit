import torch
import torch.nn as nn
import torch.nn.functional as F


class ActionHead(nn.Module):
    def __init__(self, in_dim, n_actions):
        super().__init__()
        self.mlp = nn.Sequential(
            nn.Linear(in_dim, 1024), nn.ReLU(),
            nn.Linear(1024, 1024), nn.ReLU(),
            nn.Dropout(0.2),
            nn.Linear(1024, 1024), nn.ReLU(),
            nn.Dropout(0.2),
            nn.Linear(1024, n_actions)  # raw logits
        )

    def forward(self, x, legal_mask):
        logits = self.mlp(x)  # (B, n_actions)
        neg_inf = torch.finfo(logits.dtype).min
        masked_logits = torch.where(
            legal_mask.bool(), logits, torch.full_like(logits, neg_inf)
        )
        probs = F.softmax(masked_logits, dim=-1)
        return probs, masked_logits


class CEWithMask(nn.Module):
    def forward(self, masked_logits, target_idx):
        return F.cross_entropy(masked_logits, target_idx, reduction="mean")
