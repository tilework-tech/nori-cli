"""Run two independent stdio ACP fixtures through local launch definitions."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

nori, rust_peer = map(os.path.abspath, sys.argv[1:])
python_peer = Path(__file__).resolve().parents[1] / "recovery" / "adapter.py"
args = ["one argument", "", "literal;$HOME", "*.txt", "--", "雪"]
prefix = ["wrapper option", ""]


def write_script(path, body):
    path.write_text(f"#!{sys.executable}\n" + body)
    path.chmod(0o755)


with tempfile.TemporaryDirectory() as directory:
    home = Path(directory)
    runtime = home / "prepared runtime"
    runtime.mkdir()
    (runtime / "fixture").write_text("runtime fixture")
    wrapper = home / "wrap agent"
    write_script(wrapper, """
import json, os, sys
from pathlib import Path
Path("wrapper.json").write_text(json.dumps(sys.argv[1:]))
os.execv(sys.argv[3], sys.argv[3:])
""")
    for peer, argv in [("fern", [rust_peer]), ("aster", [sys.executable, str(python_peer)])]:
        server = home / peer
        write_script(server, """
import json, os, sys
from pathlib import Path
runtime = os.environ.get("NORI_AGENT_RUNTIME_DIR")
Path("launch.json").write_text(json.dumps({
    "args": sys.argv[1:], "setting": os.environ.get("EXAMPLE_SETTING"),
    "inherited": os.environ.get("EXAMPLE_INHERITED"),
    "file": (Path(runtime) / "fixture").read_text() if runtime else None,
    "cwd": os.getcwd(),
}))
""" + f"command = {argv!r} + sys.argv[1:]\nos.execv(command[0], command)\n")
        for mode in ("env", "wrapped", "wrapper only", "plain"):
            definition = [
                "[default_models]", f'{peer} = "selected model"',
                "[[agents]]", f'name = "{peer}"', f'slug = "{peer}"',
                'model_override = { arg = "--model" }',
                "[agents.distribution.local]", f"command = {json.dumps(str(server))}",
                f"args = {json.dumps(args)}",
            ]
            if mode in ("env", "wrapped"):
                definition += ['env = { EXAMPLE_SETTING = "child only" }',
                               'env_remove = ["EXAMPLE_INHERITED"]']
            if mode in ("wrapped", "wrapper only"):
                definition += [f"wrapper = {json.dumps([str(wrapper), *prefix])}"]
            if mode == "wrapped":
                definition += [f"runtime_dir = {json.dumps(str(runtime))}"]
            (home / "config.toml").write_text("\n".join(definition) + "\n")
            wrap_log = home / "wrapper.json"
            wrap_log.unlink(missing_ok=True)
            environment = {
                "PATH": os.defpath, "NORI_HOME": directory, "HOME": directory,
                "NORI_MANAGED_BY_NPM": "1", "EXAMPLE_INHERITED": "parent",
                "EXAMPLE_SETTING": "inherited", "MOCK_AGENT_ECHO_PROMPT": "1",
            }
            result = subprocess.run([nori, "--agent", peer, "exec", "synthetic turn"],
                                    env=environment, cwd=home, capture_output=True,
                                    text=True, timeout=20)
            assert result.returncode == 0, (peer, mode, result.stderr)
            assert result.stdout.strip() == "synthetic turn", (peer, mode, result.stdout)
            actual = json.loads((home / "launch.json").read_text())
            expected = {
                "args": [*args, "--model", "selected model"], "setting": "child only" if mode in ("env", "wrapped") else "inherited",
                "inherited": None if mode in ("env", "wrapped") else "parent",
                "file": "runtime fixture" if mode == "wrapped" else None,
                "cwd": str(home.resolve()),
            }
            assert actual == expected, (peer, mode, actual, expected)
            if mode in ("wrapped", "wrapper only"):
                assert json.loads(wrap_log.read_text()) == [*prefix, str(server), *expected["args"]]
            else:
                assert not wrap_log.exists()

    missing = home / "not prepared"
    definition += [f"runtime_dir = {json.dumps(str(missing))}"]
    (home / "config.toml").write_text("\n".join(definition) + "\n")
    (home / "launch.json").unlink()
    result = subprocess.run([nori, "--agent", peer, "exec", "synthetic turn"],
                            env=environment, cwd=home, capture_output=True, text=True, timeout=20)
    assert result.returncode != 0 and "runtime_dir" in result.stderr, result.stderr
    assert not missing.exists() and not (home / "launch.json").exists()


    write_script(wrapper, 'import os, sys\nprint("synthetic banner", flush=True)\nos.execv(sys.argv[1], sys.argv[1:])\n')
    (home / "config.toml").write_text("\n".join([
        "[[agents]]", 'name = "Wrapped"', 'slug = "wrapped"',
        "[agents.distribution.local]", f"command = {json.dumps(rust_peer)}",
        f"wrapper = {json.dumps([str(wrapper)])}",
    ]))
    result = subprocess.run([nori, "--agent", "wrapped", "exec", "synthetic turn"],
                            env=environment, cwd=home, capture_output=True, text=True, timeout=10)
    assert result.returncode != 0 and "ACP JSON-RPC" in result.stderr, result.stderr
