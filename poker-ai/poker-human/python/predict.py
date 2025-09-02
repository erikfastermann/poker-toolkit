import torch

from poker_human import ActionHead, Dataset


if __name__ == "__main__":
    db_path = "../../poker-app/phh_full.db"
    limit = 10_000
    model_path = "poker.pt"


    device = "cuda" if torch.cuda.is_available() else "cpu"

    dataset = Dataset(db_path, limit=limit)

    checkpoint = torch.load(model_path, map_location=device)
    model = ActionHead(in_dim=dataset.in_dim, n_actions=dataset.n_actions).to(device)
    model.load_state_dict(checkpoint)
    model.eval()

    with torch.no_grad():
        for index in range(len(dataset)):
            x, legal_mask, target = dataset[index]

            probs, _ = model(x, legal_mask)

            print("Model inputs:", x.cpu().numpy())
            print("Legal mask:", legal_mask.cpu().numpy())
            print("Expected target:", target.cpu().numpy())
            print("Predicted action probabilities:", probs.cpu().numpy())
            print("Sum over legal actions:", probs.sum().item())
            print()

            input()
