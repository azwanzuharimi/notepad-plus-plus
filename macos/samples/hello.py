# Sample file for the slice 1 smoke test
import os


class Greeter:
    """Say hello."""

    def __init__(self, name: str) -> None:
        self.name = name

    @property
    def message(self):
        return f"Hello, {self.name}!"


def main():
    for i in range(3):
        print(Greeter(os.getlogin()).message, i * 2.5)
    raise ValueError('done') if False else None


if __name__ == "__main__":
    main()
