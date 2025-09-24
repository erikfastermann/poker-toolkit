from datetime import datetime
from pathlib import Path
import torch
from torch.utils.data import DataLoader, random_split
from tqdm import tqdm

from poker_human_user import ActionDataset, ActionHead, CEWithMask


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
    limit = 1000
    out_model_dir = "action"
    batch_size = 512
    learning_rate = 1e-3 # TODO: Maybe test smaller or dynamic.
    epochs = 100


    start = datetime.now()

    Path(out_model_dir).mkdir(exist_ok=True)

    out_dir = Path(out_model_dir) / start.strftime("%Y-%m-%d_%H-%M-%S")
    out_dir.mkdir()

    device = torch.device("cuda" if torch.cuda.is_available() else "cpu")

    dataset = ActionDataset(db_path, limit=limit)
    train, val, test = random_split(
        dataset,
        [0.98, 0.01, 0.01],
        generator=torch.Generator().manual_seed(42), # deterministic
    )

    train_loader = DataLoader(train, batch_size=batch_size, shuffle=True, num_workers=4)
    val_loader = DataLoader(val, batch_size=batch_size, shuffle=True, num_workers=4)

    model = ActionHead().to(device)
    optimizer = torch.optim.Adam(model.parameters(), lr=learning_rate)
    criterion = CEWithMask()

    for epoch in range(epochs):
        train_loss = train_one_epoch(model, train_loader, optimizer, criterion, device)
        val_loss = evaluate(model, val_loader, criterion, device)
        print(f"Epoch {epoch+1}: train_loss={train_loss:.4f}, val_loss={val_loss:.4f}")

        out_model_path = out_dir / f"action-{epoch+1}.pt"
        torch.save(model.state_dict(), out_model_path)
