import torch
import torch.nn as nn
import torch.nn.functional as F
from torch.utils.data import Dataset, DataLoader
import poker_human
from tqdm import tqdm


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


class PokerDataset(Dataset):
    def __init__(self, db_path, limit=None):
        self.dataset = poker_human.Dataset(db_path, limit)
        self.in_dim = self.dataset.INPUT_LEN
        self.n_actions = self.dataset.TARGET_LEN

    def __len__(self):
        return self.dataset.total_actions_of_interest()

    def __getitem__(self, idx):
        x, legal_mask, target = self.dataset.get_item(idx)
        x = torch.tensor(x, dtype=torch.float32)
        legal_mask = torch.tensor(legal_mask, dtype=torch.int8)
        target = torch.tensor(target, dtype=torch.float32)
        return x, legal_mask, target


def train_one_epoch(model, dataloader, optimizer, criterion, device):
    model.train()
    total_loss = 0.0

    for x, legal_mask, target in tqdm(dataloader):
        x = x.to(device)
        legal_mask = legal_mask.to(device)
        target = target.to(device)

        optimizer.zero_grad()
        probs, masked_logits = model(x, legal_mask)
        loss = criterion(masked_logits, target)
        loss.backward()
        optimizer.step()

        total_loss += loss.item() * x.size(0)

    return total_loss / len(dataloader.dataset)


def evaluate(model, dataloader, criterion, device):
    model.eval()
    total_loss = 0.0

    with torch.no_grad():
        for x, legal_mask, target in tqdm(dataloader):
            x = x.to(device)
            legal_mask = legal_mask.to(device)
            target = target.to(device)

            probs, masked_logits = model(x, legal_mask)
            loss = criterion(masked_logits, target)
            total_loss += loss.item() * x.size(0)

    return total_loss / len(dataloader.dataset)


if __name__ == "__main__":
    db_path = "../../poker-app/phh_full.db"
    limit = 10_000

    device = torch.device("cuda" if torch.cuda.is_available() else "cpu")

    dataset = PokerDataset(db_path, limit=limit)
    dataloader = DataLoader(dataset, batch_size=512, shuffle=True, num_workers=4)

    model = ActionHead(in_dim=dataset.in_dim, n_actions=dataset.n_actions).to(device)
    optimizer = torch.optim.Adam(model.parameters(), lr=1e-3)
    criterion = CEWithMask()

    for epoch in range(5):
        # TODO: Data splits
        train_loss = train_one_epoch(model, dataloader, optimizer, criterion, device)
        val_loss = evaluate(model, dataloader, criterion, device)
        print(f"Epoch {epoch+1}: train_loss={train_loss:.4f}, val_loss={val_loss:.4f}")
