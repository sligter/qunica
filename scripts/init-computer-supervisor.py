"""Create the local control-plane secret without printing or replacing it."""
import os
from pathlib import Path
import secrets


def main():
    root = Path(__file__).resolve().parent.parent
    destination = root / ".qunica" / "computer-supervisor.token"
    destination.parent.mkdir(exist_ok=True)
    try:
        descriptor = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    except FileExistsError:
        value = destination.read_text(encoding="ascii").strip()
        if len(value) < 32 or any(character.isspace() for character in value):
            raise SystemExit("Existing Supervisor token is invalid; restore it explicitly.")
        print("Existing Supervisor token preserved.")
        return
    with os.fdopen(descriptor, "w", encoding="ascii") as token_file:
        token_file.write(secrets.token_hex(32) + "\n")
        token_file.flush()
        os.fsync(token_file.fileno())
    print("Supervisor token created in the ignored .qunica directory.")


if __name__ == "__main__":
    main()
