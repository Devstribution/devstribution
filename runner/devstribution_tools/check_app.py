"""Static checks of an app's code, run by `devstribution dev` and `devstribution pack`.

Usage: python -I check_app.py <app_dir>   (spec as JSON on stdin)
  spec: {"kind": "form", "entry": "main:run", "inputs": [{"name": "x", "type": "file"}],
         "excluded_dirs": [".venv", ...]}
Prints {"errors": [...], "warnings": [...]} as JSON. Never imports or runs the app's code.
"""

import ast
import json
import os
import sys
from pathlib import Path

# Annotations accepted for each input type (after removing `Optional[...]` / `| None`).
COMPATIBLE = {
    "file": {"Path", "pathlib.Path", "str", "PurePath"},
    "folder": {"Path", "pathlib.Path", "str", "PurePath"},
    "text": {"str"},
    "choice": {"str", "Literal"},
    "secret": {"str"},
    "number": {"int", "float"},
    "bool": {"bool"},
    "date": {"date", "datetime.date", "str"},
}

OFFICE_PROGIDS = ("excel.", "word.", "outlook.", "powerpoint.", "access.")


def module_file(app_dir: Path, module: str):
    base = app_dir.joinpath(*module.split("."))
    for candidate in (base.with_suffix(".py"), base / "__init__.py"):
        if candidate.is_file():
            return candidate
    return None


def parse(path: Path, errors: list):
    try:
        return ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
    except SyntaxError as exc:
        errors.append(f"{path.name}:{exc.lineno}: syntax error: {exc.msg}")
    except UnicodeDecodeError:
        errors.append(f"{path.name}: not UTF-8")
    return None


def strip_optional(node):
    """`Optional[X]`, `X | None`, `Union[X, None]` → X; anything else unchanged."""
    if isinstance(node, ast.BinOp) and isinstance(node.op, ast.BitOr):
        sides = [s for s in (node.left, node.right) if not is_none(s)]
        return sides[0] if len(sides) == 1 else node
    if isinstance(node, ast.Subscript):
        name = ast.unparse(node.value)
        if name in ("Optional", "typing.Optional"):
            return node.slice
        if name in ("Union", "typing.Union") and isinstance(node.slice, ast.Tuple):
            parts = [p for p in node.slice.elts if not is_none(p)]
            return parts[0] if len(parts) == 1 else node
    return node


def is_none(node) -> bool:
    return isinstance(node, ast.Constant) and node.value is None


def annotation_names(node):
    node = strip_optional(node)
    if isinstance(node, ast.Constant) and isinstance(node.value, str):  # "Path" (string annotation)
        try:
            node = strip_optional(ast.parse(node.value, mode="eval").body)
        except SyntaxError:
            return None
    if isinstance(node, ast.Subscript):
        return {ast.unparse(node.value).replace("typing.", "")}
    if isinstance(node, ast.BinOp) and isinstance(node.op, ast.BitOr):
        left, right = annotation_names(node.left), annotation_names(node.right)
        return None if left is None or right is None else left | right
    return {ast.unparse(node)}


def check_function(func, kind: str, inputs: list, errors: list, warnings: list, label: str):
    if isinstance(func, ast.AsyncFunctionDef):
        errors.append(f"{label} is `async def`; use a normal `def`")
        return
    args = func.args
    if args.vararg or args.kwarg:
        errors.append(f"{label} must not use *args or **kwargs; list each input by name")
    params = {a.arg: a for a in args.posonlyargs + args.args + args.kwonlyargs}
    if kind != "form":
        defaults = len(args.defaults)
        required = (args.posonlyargs + args.args)[: len(args.posonlyargs + args.args) - defaults]
        if required:
            warnings.append(f"{label} is called without arguments but requires {[a.arg for a in required]}")
        return
    if "ctx" not in params:
        errors.append(f"{label} must accept a `ctx` parameter")
    declared = {i["name"]: i["type"] for i in inputs}
    for name in declared:
        if name not in params:
            errors.append(f"{label} is missing the parameter `{name}` declared in [[tool.devstribution.inputs]]")
    for name, arg in params.items():
        if name == "ctx":
            continue
        if name not in declared:
            errors.append(
                f"{label} has the parameter `{name}`, which is not declared in "
                "[[tool.devstribution.inputs]] in pyproject.toml"
            )
            continue
        if arg.annotation is None:
            continue
        names = annotation_names(arg.annotation)
        allowed = COMPATIBLE[declared[name]]
        if names is None or not names <= allowed | {"Any", "typing.Any"}:
            errors.append(
                f"{label}: parameter `{name}: {ast.unparse(arg.annotation)}` does not match "
                f"input type \"{declared[name]}\" (use {' or '.join(sorted(allowed))})"
            )


def dispatch_warnings(app_dir: Path, excluded: set, errors: list):
    found = []
    for root, dirs, files in os.walk(app_dir):
        dirs[:] = sorted(d for d in dirs if d not in excluded)
        for name in sorted(files):
            if not name.endswith(".py"):
                continue
            path = Path(root, name)
            tree = parse(path, errors)
            if tree is None:
                continue
            rel = path.relative_to(app_dir).as_posix()
            for node in ast.walk(tree):
                if not isinstance(node, ast.Call) or not node.args:
                    continue
                func = node.func
                called = func.attr if isinstance(func, ast.Attribute) else getattr(func, "id", None)
                first = node.args[0]
                if (
                    called in ("Dispatch", "EnsureDispatch")
                    and isinstance(first, ast.Constant)
                    and isinstance(first.value, str)
                    and first.value.lower().startswith(OFFICE_PROGIDS)
                ):
                    found.append(
                        f"{rel}:{node.lineno}: {called}({first.value!r}) attaches to an instance the "
                        f"user already has open, so cleanup would close their documents; use "
                        f"win32com.client.DispatchEx({first.value!r}) and ctx.track_com(obj)"
                    )
    return found


def check(app_dir: Path, spec: dict) -> dict:
    errors: list = []
    warnings: list = []
    module, _, function = spec["entry"].partition(":")
    path = module_file(app_dir, module)
    if path is None:
        errors.append(f"entry module `{module}` not found (expected {module.replace('.', '/')}.py)")
    else:
        tree = parse(path, errors)
        if tree is not None and function:
            funcs = [
                n for n in tree.body
                if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef)) and n.name == function
            ]
            if not funcs:
                errors.append(f"{path.name} has no top-level function `{function}` (see `entry`)")
            else:
                label = f"{path.name}: {function}()"
                check_function(funcs[-1], spec["kind"], spec.get("inputs", []), errors, warnings, label)
    excluded = set(spec.get("excluded_dirs", []))
    warnings.extend(dispatch_warnings(app_dir, excluded, errors))
    return {"errors": list(dict.fromkeys(errors)), "warnings": list(dict.fromkeys(warnings))}


if __name__ == "__main__":
    spec = json.loads(sys.stdin.read())
    json.dump(check(Path(sys.argv[1]).resolve(), spec), sys.stdout)
