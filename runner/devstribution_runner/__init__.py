"""The run context that devstribution gives to apps.

In production this package is provided by the launcher (it is never installed in the app's
environment). For development it is published on PyPI as `devstribution-runner`, to be added
as a dev dependency for type hints and local tests:

    from devstribution_runner import local_context
    result = run(Path("orders.xlsx"), ctx=local_context())

`form` apps receive the context as their `ctx` argument; apps of every other kind read it from
`devstribution_runner.ctx`.
"""

from ._context import Cancelled, Context, local_context

__all__ = ["Cancelled", "Context", "ctx", "local_context"]
__version__ = "0.1.0"

#: The context of the current run, set by the runner before the app starts (None otherwise).
ctx: "Context | None" = None
