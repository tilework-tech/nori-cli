"""Sample /proc during launch_matrix.py; record the real adapter, Pi and wrapper processes per case."""
import json, os, sys, time
seen = {}
end = time.time() + float(sys.argv[2])
while time.time() < end and not os.path.exists(sys.argv[1] + ".stop"):
    for pid in os.listdir("/proc"):
        if not pid.isdigit(): continue
        try:
            env = dict(e.split("=", 1) for e in open(f"/proc/{pid}/environ", "rb").read().decode(errors="replace").split("\0") if "=" in e)
            if "FIXTURE_CAPTURE" not in env: continue
            argv = open(f"/proc/{pid}/cmdline", "rb").read().decode(errors="replace").split("\0")[:-1]
            seen[(int(pid), tuple(argv))] = {"pid": int(pid), "argv": argv, "cwd": os.readlink(f"/proc/{pid}/cwd"),
                "case": env["FIXTURE_CAPTURE"].split("/")[-4] if env["FIXTURE_CAPTURE"].count("/") > 4 else env["FIXTURE_CAPTURE"],
                "has": sorted(k for k in env if k in ("ACCEPT_AMBIENT_KEY", "ACCEPT_LOOPBACK_KEY", "NORI_ACCEPT_VALUE",
                    "PI_ACP_PI_COMMAND", "PI_CODING_AGENT_DIR", "NORI_AGENT_RUNTIME_DIR"))}
        except (OSError, ValueError):
            pass
    time.sleep(0.01)
json.dump(list(seen.values()), open(sys.argv[1], "w"), indent=0)
