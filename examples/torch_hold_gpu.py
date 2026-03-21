import argparse
import time


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Allocate GPU memory with torch and hold it for a while."
    )
    parser.add_argument(
        "--mb",
        type=int,
        default=2000,
        help="Approximate GPU memory to allocate in megabytes.",
    )
    parser.add_argument(
        "--seconds",
        type=int,
        default=30,
        help="How many seconds to hold the allocation.",
    )
    args = parser.parse_args()

    import torch

    if not torch.cuda.is_available():
        raise SystemExit("CUDA is not available")

    device = torch.device("cuda:0")
    total_bytes = args.mb * 1024 * 1024
    num_elements = total_bytes // 4

    print(f"Allocating about {args.mb} MB on {device} for {args.seconds} seconds")
    tensor = torch.empty(num_elements, dtype=torch.float32, device=device)
    tensor.fill_(1.0)
    torch.cuda.synchronize()
    print("Allocation complete")
    time.sleep(args.seconds)
    print("Done")


if __name__ == "__main__":
    main()
