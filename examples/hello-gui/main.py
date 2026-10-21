"""A small tkinter app (standard library only) that also uses the run context."""

import tkinter as tk
from datetime import datetime

import devstribution_runner


def main() -> None:
    ctx = devstribution_runner.ctx  # None when run outside devstribution
    root = tk.Tk()
    root.title("Hello GUI")
    root.minsize(320, 140)

    label = tk.Label(root, text="Hello from devstribution", padx=24, pady=16)
    label.pack()

    def save_note() -> None:
        stamp = datetime.now().strftime("%H:%M:%S")
        if ctx is not None:
            path = ctx.output_dir / "note.txt"
            with path.open("a", encoding="utf-8") as f:
                f.write(f"Button pressed at {stamp}\n")
            ctx.log(f"note saved to {path}")
        label.config(text=f"Saved at {stamp}")

    tk.Button(root, text="Save a note", command=save_note).pack(pady=(0, 16))
    root.mainloop()


if __name__ == "__main__":
    main()
