import torch
from torch.utils.data import DataLoader
from tqdm import tqdm

from poker_human import ActionDataset, ActionHead, CEWithMask


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
    out_model_path = "action.pt"


    device = torch.device("cuda" if torch.cuda.is_available() else "cpu")

    dataset = ActionDataset(db_path, limit=limit)
    dataloader = DataLoader(dataset, batch_size=512, shuffle=True, num_workers=4)

    model = ActionHead().to(device)
    optimizer = torch.optim.Adam(model.parameters(), lr=1e-3)
    criterion = CEWithMask()

    for epoch in range(5):
        # TODO: Data splits
        train_loss = train_one_epoch(model, dataloader, optimizer, criterion, device)
        val_loss = evaluate(model, dataloader, criterion, device)
        print(f"Epoch {epoch+1}: train_loss={train_loss:.4f}, val_loss={val_loss:.4f}")

    torch.save(model.state_dict(), out_model_path)
