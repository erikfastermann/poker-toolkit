import torch
from torch.utils.data import DataLoader
from tqdm import tqdm

from poker_human import (
    ShowdownPreprocessedDataset,
    ShowdownHead,
    masked_bce_with_logits_loss,
)


def train_one_epoch(model, dataloader, optimizer, device):
    model.train()
    total_loss = 0.0

    for x, legal_mask, target in tqdm(dataloader):
        x = x.to(device)
        legal_mask = legal_mask.to(device)
        target = target.to(device)

        optimizer.zero_grad()
        logits = model(x)
        loss = masked_bce_with_logits_loss(logits, target, legal_mask)
        loss.backward()
        optimizer.step()

        total_loss += loss.item() * x.size(0)

    return total_loss / len(dataloader.dataset)


def evaluate(model, dataloader, device):
    model.eval()
    total_loss = 0.0

    with torch.no_grad():
        for x, legal_mask, target in tqdm(dataloader):
            x = x.to(device)
            legal_mask = legal_mask.to(device)
            target = target.to(device)

            logits = model(x)
            loss = masked_bce_with_logits_loss(logits, target, legal_mask)
            total_loss += loss.item() * x.size(0)

    return total_loss / len(dataloader.dataset)


if __name__ == "__main__":
    dataset_preprocessed_path = "showdown.pkl"
    out_model_path = "showdown.pt"


    device = torch.device("cuda" if torch.cuda.is_available() else "cpu")

    dataset = ShowdownPreprocessedDataset(dataset_preprocessed_path)
    dataloader = DataLoader(dataset, batch_size=64, shuffle=True)

    model = ShowdownHead().to(device)
    optimizer = torch.optim.Adam(model.parameters(), lr=1e-3)

    for epoch in range(5):
        # TODO: Data splits
        train_loss = train_one_epoch(model, dataloader, optimizer, device)
        val_loss = evaluate(model, dataloader, device)
        print(f"Epoch {epoch+1}: train_loss={train_loss:.4f}, val_loss={val_loss:.4f}")

    torch.save(model.state_dict(), out_model_path)
