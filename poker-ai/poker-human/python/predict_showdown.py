import numpy as np
import torch
import sys

from poker_human import ShowdownHead, ShowdownDataset


if __name__ == "__main__":
    db_path = "../../poker-app/phh_full.db"
    limit = 10_000
    model_path = "showdown.pt"


    np.set_printoptions(threshold=sys.maxsize)

    device = "cuda" if torch.cuda.is_available() else "cpu"

    dataset = ShowdownDataset(db_path, limit=limit)

    checkpoint = torch.load(model_path, map_location=device)
    model = ShowdownHead().to(device)
    model.load_state_dict(checkpoint)
    model.eval()

    with torch.no_grad():
        for index in range(len(dataset)):
            x, legal_mask, target = dataset[index]

            probs = torch.sigmoid(model(x))

            print("Model inputs:", x.cpu().numpy())
            print("Legal mask:", legal_mask.cpu().numpy())
            print("Expected target:", target.cpu().numpy())
            print("Predicted showdown probabilities:", probs.cpu().numpy())
            print()

            input()
