# Instructions for coding agents

This folder is a **devstribution app**: a Python app distributed as a signed `.dsapp` package
and started by the devstribution launcher. Keep it that way.

## Rules
- The manifest is `[tool.devstribution]` in `pyproject.toml`. Dependencies go only in
  `[project].dependencies`; after changing them run `uv lock`. Never remove `[tool.uv] package = false`.
- Only dependencies that publish **wheels** for every platform in `platforms` can be used
  (no source builds, no git or path dependencies).
- `kind = "form"`: `entry = "main:run"` names a function whose parameters are exactly the
  `[[tool.devstribution.inputs]]` names plus `ctx`, and which returns a dict whose keys are the
  `[[tool.devstribution.outputs]]` names. Write output files inside `ctx.output_dir`.
- Use `ctx.progress(fraction, message)` in long loops (it is also where Cancel takes effect),
  `ctx.log(message)` for the user, `ctx.secrets.get(name)` for secrets declared in
  `[tool.devstribution.permissions] secrets`. Never hard-code credentials.
- No install scripts and no downloading code at runtime. Data files the app needs (models,
  browsers) go in `[[tool.devstribution.resources]]` with their sha256.
- Office automation: `win32com.client.DispatchEx(...)`, never `Dispatch(...)`, then
  `ctx.track_com(obj)` so the instance is closed even if the run is stopped.
- Do not put `.key` files, `.env` files or other secrets in this folder.

## Loop
1. `devstribution dev . -i name=value` runs the app like the launcher does. Its error messages
   say what to change; fix and re-run until it passes.
2. `devstribution pack .` checks everything again, signs and writes `dist/<app>-<version>.dsapp`.
   Bump `[project].version` for every release.
