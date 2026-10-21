# devstribution-runner

The run context (`ctx`) that [devstribution](https://github.com/Devstribution/devstribution)
gives to apps. The launcher ships its own copy of this code and starts every app with it;
this package exists so that app authors get type hints and can call their code in tests:

```python
from pathlib import Path
from devstribution_runner import local_context

from main import run

result = run(Path("orders.xlsx"), group_by="Customer", sum_column=None, ctx=local_context())
```

Add it as a dev dependency only (`uv add --dev devstribution-runner`); production environments
are installed with `--no-dev` and never contain it.

Layout: `bootstrap.py` is the entry point the launcher starts with `python -I`;
`devstribution_runner/_runner.py` documents the JSON protocol between launcher and runner;
`devstribution_tools/check_app.py` holds the static checks used by `devstribution dev` and `pack`.
