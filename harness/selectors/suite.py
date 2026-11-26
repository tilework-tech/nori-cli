"""Selector-stack acceptance (#795, #797, #798, #799) on one build.

Usage: suite.py <bins-dir> <label> <out-dir> [scenario,...]

Every check carries the PR that introduces the behaviour. On a build that
does not contain that PR the observation is still recorded, as "info".
"""
import pyte
from drv import *

# Stack order: main..#794 (66a76859) -> #795 -> #791 -> #797 -> #798 -> #799
LEVEL = {"66a76859": 794, "c6cef012": 795, "27ccdfda": 796, "d4239329": 797,
         "575e4af0": 798, "7a797061": 799,
         "0f47a50b": 797, "f2630c0e": 798, "d387c9ec": 799}

bins = Path(sys.argv[1]).resolve()
label = sys.argv[2]
OUT = Path(sys.argv[3]).resolve() / label
ONLY = sys.argv[4].split(",") if len(sys.argv) > 4 else None
R = Results(OUT, label)
level = LEVEL[label]
HOST_CFG = "[tui]\nmax_sessions = 40\n[host]\nmax_sessions = 40\n"
TITLES = ["Synthetic parser fix", "Pear ledger audit", "Plum row cleanup", "Cedar count report",
          "Orchard map refresh", "Apple grade sorter", "Quince inventory sync", "Fig tree pruning plan",
          "Olive press checklist", "Walnut shell tally", "Cherry bloom tracker", "Lemon grove irrigation",
          "Peach crate labels", "Apricot drying rack", "Hazel hedge survey", "Mulberry leaf counter",
          "Persimmon ripeness log", "Chestnut roast timer"]


def obs(sid, cid, detail):
    R.check(sid, f"{cid} [observation]", True, detail, None)


def check(sid, cid, ok, detail="", pr=None):
    applies = pr is None or level >= pr
    R.check(sid, cid if applies else f"{cid} [info: build predates #{pr}]", ok if applies else True,
            detail if applies else f"observed={bool(ok)} {detail}", pr)


def cells(t):
    """Interpret the pane's ANSI capture: rows of (char, fg, bg, bold)."""
    scr = pyte.Screen(t.cols, t.rows)
    pyte.Stream(scr).feed(t.screen(ansi=True).replace("\n", "\r\n"))
    return [[(scr.buffer[y][x].data, scr.buffer[y][x].fg, scr.buffer[y][x].bg, scr.buffer[y][x].bold)
             for x in range(t.cols)] for y in range(t.rows)]


def row_text(row):
    return "".join(c[0] for c in row).rstrip()


def focus_report(t):
    """Facts about the focused row: its marks, its shading, and its neighbours'."""
    grid = cells(t)
    focus = [i for i, r in enumerate(grid) if "▏" in row_text(r)]
    if not focus:
        return None
    y = focus[0]
    row = grid[y]
    x = next(i for i, c in enumerate(row) if c[0] == "▏")
    xr = next((i for i in range(len(row) - 1, x, -1) if row[i][0] == "▕"), len(row))
    span = row[x + 1:xr]
    ptr = next((c for c in span if c[0] == "›"), None)
    label_cells = [c for c in span[3:] if c[0].strip() and c[0] not in "▏▕›"]
    text_bgs = {c[2] for c in span if c[0].strip()}
    # unfocused list rows: same columns, text, outside the focus block
    others = [grid[i][x + 1:xr] for i in range(len(grid)) if i not in focus
              and row_text(grid[i])[:x + 3].strip() == "" and row_text(grid[i]).strip()
              and abs(i - y) <= 8]
    other_bgs = {c[2] for r in others[:3] for c in r if c[0].strip()}
    return {"y": y, "x": x, "rows": len(focus), "text": row_text(row),
            "rail_fg": row[x][1], "rail_bold": row[x][3], "right_rail": "▕" in row_text(row),
            "ptr_fg": ptr and ptr[1], "ptr_bold": ptr and ptr[3],
            "label_fg": sorted({c[1] for c in label_cells}), "label_bold": any(c[3] for c in label_cells[:6]),
            "focus_bg": sorted(text_bgs), "other_bg": sorted(other_bgs)}


GREEN = {"green", "00cd00", "008000"}


def is_green(fg):
    return fg in GREEN or fg == "2"


def full_row(t):
    """Background of every cell between the rails of each focused line, and the
    background of the prompt panel just outside them (the cell before the left rail)."""
    grid = cells(t)
    out = []
    for r in grid:
        text = row_text(r)
        if "▏" not in text or "▕" not in text:
            continue
        x = text.index("▏"); xr = text.rindex("▕")
        out.append({"inside": sorted({c[2] for c in r[x + 1:xr]}), "outside": r[x - 1][2] if x else None})
    return out


def full_row_ok(sid, tag, t, pr):
    rows = full_row(t)
    ok = bool(rows) and all(len(r["inside"]) == 1 and r["inside"][0] not in ("default", r["outside"])
                            for r in rows)
    check(sid, f"{tag}.full_row_contrast_against_panel", ok, rows, pr)


def marks_ok(sid, tag, rep, shaded, pr):
    check(sid, f"{tag}.rails_and_pointer", rep and rep["right_rail"] and rep["ptr_fg"] is not None
          and is_green(rep["rail_fg"]) and is_green(rep["ptr_fg"]), rep, pr)
    if not rep:
        return
    if shaded:
        check(sid, f"{tag}.shaded_bg_differs_from_unfocused_rows",
              rep["focus_bg"] and set(rep["focus_bg"]) - {"default"} and not set(rep["focus_bg"]) & set(rep["other_bg"]),
              f"focus_bg={rep['focus_bg']} other_bg={rep['other_bg']}", pr)
        check(sid, f"{tag}.shaded_marks_not_bold", not rep["ptr_bold"] and not rep["rail_bold"], rep, pr)
    else:
        check(sid, f"{tag}.unshaded_no_bg", rep["focus_bg"] == ["default"], rep["focus_bg"], pr)
        check(sid, f"{tag}.unshaded_marks_bold", rep["ptr_bold"] and rep["rail_bold"], rep, pr)


def start_tui(sid, shaded, cols=120, rows=32, extra_cfg="", **envx):
    fx = setup(bins, f"{label}-{sid}", extra_cfg)
    t = term(fx, sid, OUT / sid, shaded, cols, rows, **envx)
    t.run("nori --agent mock-model")
    t.wait_for("Mock ACP options", 60)
    time.sleep(1.0)
    return fx, t


def open_cmd(t, cmd, expect):
    slow_type(t, cmd)
    t.keys("Enter")
    return t.wait_for(expect, 10)


def focused_label(t):
    rows = focus_rows(t)
    return plain(rows[0]) if rows else ""


# ---------------------------------------------------------------- scenarios

def s_settings(shaded):
    sid = f"L1-settings-{'shaded' if shaded else 'unshaded'}"
    fx, t = start_tui(sid, shaded)
    open_cmd(t, "/settings", "Configuration")
    time.sleep(0.4)
    t.capture("01-open")
    marks_ok(sid, "settings", focus_report(t), shaded, 795)
    check(sid, "pointer_beside_number", re.search(r"▏› 1\. ", focused_label(t)), focused_label(t), 795)
    check(sid, "no_legacy_bullet", "◉" not in t.screen(), "", 795)
    t.keys("j", "j"); time.sleep(0.3)
    check(sid, "j_moves_down", "› 3." in focused_label(t), focused_label(t), 795)
    t.keys("k"); time.sleep(0.3)
    check(sid, "k_moves_up", "› 2." in focused_label(t), focused_label(t), 795)
    t.capture("02-after-jk")
    footer = [l for l in t.screen().splitlines() if "enter select" in l]
    check(sid, "footer_words", footer and "↑↓/j/k move" in footer[0] and "/ search" in footer[0], footer, 799)
    slow_type(t, "x")
    check(sid, "stray_letter_does_not_filter", "⌕" not in t.screen() and "› 2." in focused_label(t),
          focused_label(t), 795)
    for key, typed, want in (("f", "term", "Terminal Notifications"), ("C-f", "flow", "Resize Reflow"),
                             ("/", "vert", "Vertical Footer")):
        if key == "/":
            slow_type(t, "/")
        else:
            t.keys(key)
        time.sleep(0.3)
        slow_type(t, typed)
        scr = t.screen()
        tag = {"f": "f", "C-f": "ctrl_f", "/": "slash"}[key]
        check(sid, f"search_{tag}_filters", "⌕" in scr and want in scr and typed in scr, scr.splitlines()[-14:])
        if tag == "f":
            t.capture("03-search-f")
            marks_ok(sid, "search_result", focus_report(t), shaded, 795)
        t.keys("Escape"); time.sleep(0.4)
        check(sid, f"search_{tag}_esc_ends_search", "⌕" not in t.screen() and "Configuration" in t.screen())
    t.close()


def s_settings_narrow(shaded):
    sid = f"L2-settings-narrow-{'shaded' if shaded else 'unshaded'}"
    for cols in (60, 40):
        fx, t = start_tui(f"{sid}-{cols}", shaded, cols=cols)
        open_cmd(t, "/settings", "Configuration")
        time.sleep(0.5)
        t.capture(f"01-{cols}cols")
        rep = focus_report(t)
        lines = [l for l in t.screen().splitlines() if "▏" in l]
        right_edge = all(len(l.rstrip()) <= cols and l.rstrip().endswith("▕") for l in lines)
        check(sid, f"{cols}.right_rail_on_every_focused_line", lines and right_edge, lines, 795)
        marks_ok(sid, f"{cols}", rep, shaded, 795)
        if cols == 60:
            resize(t, 40)
            t.capture("02-resized-60-to-40")
            obs(sid, "resize_60_to_40_rail_lines", [plain(l) for l in t.screen().splitlines() if "▏" in l])
        t.close()


def s_model_config():
    sid = "L3-model-config-agent"
    fx, t = start_tui(sid, True)
    open_cmd(t, "/model", "Select a value")
    time.sleep(0.4)
    t.capture("01-model")
    check(sid, "model.current_marker", "Mock Default Model (current)" in t.screen(), focused_label(t), 798)
    marks_ok(sid, "model", focus_report(t), True, 795)
    footer = [l for l in t.screen().splitlines() if "esc close" in l]
    check(sid, "model.footer_offers_search", footer and "/ search" in footer[0], footer, 799)
    t.keys("j"); time.sleep(0.3)
    check(sid, "model.j_moves", "Mock Fast Model" in focused_label(t), focused_label(t))
    t.keys("f"); time.sleep(0.3); slow_type(t, "fast")
    t.capture("02-model-search-f")
    scr = t.screen()
    rows = [l for l in scr.splitlines() if re.match(r"\s+(▏› )?\s*\d\. ", l)]
    check(sid, "model.f_search_with_two_values", "⌕" in scr and len(rows) == 1 and "Mock Fast Model" in rows[0],
          rows, 799)
    t.keys("Escape"); time.sleep(0.3); t.keys("Escape"); time.sleep(0.4)
    t.keys("C-u"); time.sleep(0.2)
    open_cmd(t, "/config", "Session config")
    time.sleep(0.4)
    t.capture("03-config")
    footer = [l for l in t.screen().splitlines() if "esc close" in l]
    check(sid, "config.no_search_in_footer", footer and "search" not in footer[0], footer, 799)
    t.keys("f"); time.sleep(0.3)
    check(sid, "config.f_does_not_search", "⌕" not in t.screen(), "")
    t.keys("j"); time.sleep(0.2); t.keys("Enter"); time.sleep(0.6)
    t.capture("04-thought-level")
    check(sid, "thought.current_marker_on_focus", "Medium (current)" in focused_label(t), focused_label(t), 798)
    check(sid, "thought.no_bare_current", not re.search(r"[^(]current", plain(t.screen())), "")
    t.keys("f"); time.sleep(0.3)
    check(sid, "thought.three_values_no_search", "⌕" not in t.screen(), "", 799)
    t.keys("Escape"); time.sleep(0.3); t.keys("Escape"); time.sleep(0.4); t.keys("C-u")
    open_cmd(t, "/agent", "Mock ACP")
    time.sleep(0.6)
    t.capture("05-agent")
    scr = t.screen()
    check(sid, "agent.current_marker", "(current)" in scr, [l for l in scr.splitlines() if "current" in l], 798)
    obs(sid, "agent.recording_toggle_hint", [l for l in scr.splitlines() if "rec" in l])
    t.keys("f"); time.sleep(0.3)
    check(sid, "agent.no_search", "⌕" not in t.screen(), "")
    t.close()


def s_permission(shaded):
    sid = f"P1-permission-{'shaded' if shaded else 'unshaded'}"
    fx, t = start_tui(sid, shaded, MOCK_AGENT_REQUEST_PERMISSION="1",
                      MOCK_AGENT_PERMISSION_COMMAND="rm -rf ./synthetic-build")
    slow_type(t, "tidy the orchard")
    t.keys("Enter")
    # Answer the instant the prompt appears: the grace period must swallow it.
    deadline = time.time() + 20
    while "Would you like to allow" not in t.screen() and time.time() < deadline:
        time.sleep(0.02)
    T.tmux("send-keys", "-t", t.name, "Enter")
    time.sleep(0.15)
    check(sid, "grace.early_enter_ignored", "Would you like to allow" in t.screen(), t.screen()[-300:])
    time.sleep(0.6)
    t.capture("01-prompt")
    scr = t.screen()
    check(sid, "header.command_shown", "Command: rm -rf ./synthetic-build" in scr, "")
    rep = focus_report(t)
    marks_ok(sid, "prompt", rep, shaded, 797)
    if shaded:
        full_row_ok(sid, "prompt", t, 797)
    check(sid, "label_not_green_or_bold", rep and not any(is_green(f) for f in rep["label_fg"])
          and not rep["label_bold"], rep, 797)
    t.keys("j"); time.sleep(0.3)
    check(sid, "j_moves", "Reject" in focused_label(t), focused_label(t), 797)
    t.capture("01b-prompt-after-j")
    marks_ok(sid, "prompt_after_j", focus_report(t), shaded, 797)
    if shaded:
        full_row_ok(sid, "prompt_after_j", t, 797)
    t.keys("k"); time.sleep(0.3)
    check(sid, "k_moves", "Allow" in focused_label(t), focused_label(t), 797)
    t.keys("f"); time.sleep(0.3)
    check(sid, "f_no_search", "⌕" not in t.screen() and "Would you like" in t.screen(), "")
    t.keys("Enter"); time.sleep(1.0)
    check(sid, "late_enter_accepted", "Would you like to allow" not in t.screen(), t.screen()[-300:])
    t.close()


def s_approve_all(shaded):
    sid = f"P2-approve-all-{'shaded' if shaded else 'unshaded'}"
    fx, t = start_tui(sid, shaded)
    open_cmd(t, "/approvals", "Approvals for this session")
    time.sleep(0.4)
    t.capture("01-approvals")
    marks_ok(sid, "approvals", focus_report(t), shaded, 795)
    t.keys("j"); time.sleep(0.3); t.keys("Enter")
    t.wait_for("Approve every request", 5)
    time.sleep(0.6)
    t.capture("02-confirm")
    grid = cells(t)
    red = [row_text(r) for r in grid if any(c[1] in ("red", "cd0000") for c in r if c[0].strip())]
    check(sid, "red_warning_kept", any("harmful" in r for r in red), red)
    rep = focus_report(t)
    marks_ok(sid, "confirm", rep, shaded, 797)
    if shaded:
        full_row_ok(sid, "confirm", t, 797)
    t.keys("j"); time.sleep(0.3)
    t.capture("02b-confirm-after-j")
    check(sid, "confirm.j_moves", "Approve all, and stop" in focused_label(t), focused_label(t), 797)
    marks_ok(sid, "confirm_after_j", focus_report(t), shaded, 797)
    if shaded:
        full_row_ok(sid, "confirm_after_j", t, 797)
    t.keys("k"); time.sleep(0.3)
    check(sid, "label_not_green_or_bold", rep and not any(is_green(f) for f in rep["label_fg"])
          and not rep["label_bold"], rep, 797)
    footer = [l for l in t.screen().splitlines() if "enter confirm" in l]
    check(sid, "footer_words", footer and "↑↓/j/k move" in footer[0], footer, 799)
    t.keys("End"); time.sleep(0.3)
    obs(sid, "end_key_in_short_prompt", focused_label(t))
    t.close()


def s_popups():
    sid = "U1-small-popups"
    fx, t = start_tui(sid, True)
    obs = {}
    for name, text in (("slash", "/se"), ("at", "@pl"), ("dollar", "$")):
        slow_type(t, text)
        time.sleep(0.8)
        t.capture(f"popup-{name}")
        lines = t.screen().splitlines()
        obs[name] = lines
        check(sid, f"{name}.no_rails", not any("▏" in l for l in lines), [l for l in lines if l.strip()][-6:])
        t.keys("C-u"); time.sleep(0.2)
        t.keys("Escape"); time.sleep(0.3)
    (OUT / sid / "popups.json").write_text(json.dumps(obs, indent=1, ensure_ascii=False))
    t.close()


def host_up(sid, n=18):
    fx = setup(bins, f"{label}-{sid}", HOST_CFG)
    e = env(fx)

    def h(*a):
        return subprocess.run([str(fx["nori"]), "host", *a], cwd=fx["proj"], env=e, capture_output=True,
                              text=True, timeout=60, stdin=subprocess.DEVNULL)
    h("--port", "0")
    for title in TITLES[:n]:
        h("new", "--agent", "mock-model", "--prompt", title)
    deadline = time.time() + 60
    while h("ls").stdout.count("ready") < n and time.time() < deadline:
        time.sleep(0.5)
    return fx, h


def s_sessions(shaded):
    sid = f"L4-sessions-{'shaded' if shaded else 'unshaded'}"
    fx, h = host_up(sid)
    try:
        t = term(fx, sid, OUT / sid, shaded, 120, 30)
        t.run("nori attach")
        t.wait_for("Sessions 18", 30)
        time.sleep(0.8)
        t.capture("01-attach-list")
        marks_ok(sid, "attach_list", focus_report(t), shaded, 795)
        t.keys("j", "j", "j"); time.sleep(0.3)
        check(sid, "j_moves", "Cedar count report" in focused_label(t), focused_label(t), 795)
        t.keys("k"); time.sleep(0.3)
        check(sid, "k_moves", "Plum row cleanup" in focused_label(t), focused_label(t), 795)
        t.keys("End"); time.sleep(0.3)
        check(sid, "end_scrolls_to_last", "Chestnut roast timer" in focused_label(t), focused_label(t), 795)
        t.keys("Home"); time.sleep(0.3)
        for key, typed, want in (("f", "walnut", "Walnut shell tally"), ("C-f", "fig", "Fig tree pruning plan"),
                                 ("/", "olive", "Olive press checklist")):
            slow_type(t, "/") if key == "/" else t.keys(key)
            time.sleep(0.3); slow_type(t, typed)
            tag = {"f": "f", "C-f": "ctrl_f", "/": "slash"}[key]
            check(sid, f"search_{tag}", "⌕" in t.screen() and want in focused_label(t), focused_label(t), 795)
            t.keys("Escape"); time.sleep(0.4)
        slow_type(t, "q")
        check(sid, "stray_letter_does_not_filter", "⌕" not in t.screen(), "")
        t.keys("Home", "j", "j", "Enter")
        t.wait_for("controlling", 20)
        time.sleep(1.0)
        open_cmd(t, "/sessions", "Sessions 18")
        time.sleep(0.6)
        t.capture("02-sessions-in-tui")
        scr = t.screen()
        check(sid, "on_screen_marker", "Plum row cleanup (on screen)" in scr,
              [l for l in scr.splitlines() if "Plum" in l], 798)
        check(sid, "no_current_for_session", "(current)" not in scr and not re.search(r"\bcurrent\b", scr),
              [l for l in scr.splitlines() if "current" in l], 798)
        for cols in (60, 40, 32):
            resize(t, cols)
            t.capture(f"03-{cols}cols")
            lines = [plain(l) for l in t.screen().splitlines() if "▏" in l]
            if cols == 32:
                obs(sid, "32.table_wider_than_terminal", t.screen().splitlines()[2:6])
                continue
            check(sid, f"{cols}.right_rail_visible", lines and all(l.rstrip().endswith("▕") for l in lines),
                  lines, 795)
        resize(t, 40)
        t.keys("C-f"); time.sleep(0.3); slow_type(t, "plum")
        t.capture("04-40cols-on-screen")
        check(sid, "40.on_screen_kept_narrow", "(on screen)" in focused_label(t), focused_label(t), 798)
        t.close()
    finally:
        h("stop")


def s_resume():
    sid = "L5-resume-picker"
    fx, h = host_up(sid)
    h("stop")
    t = term(fx, sid, OUT / sid, True, 120, 30)
    t.run("nori resume")
    t.wait_for("Resume a previous session", 30)
    time.sleep(1.0)
    t.capture("01-picker")
    ids = [m.group(1) for l in t.screen().splitlines()
           for m in [re.search(r"([0-9a-f]{8})-[0-9a-f]{4}-", l[:50])] if m]
    marks_ok(sid, "resume", focus_report(t), True, 795)
    check(sid, "lists_all_18", len(ids) >= 18, len(ids))
    first, last = ids[0], ids[-1]
    t.keys("j", "j"); time.sleep(0.3)
    check(sid, "j_moves", ids[2] in focused_label(t), focused_label(t))
    t.keys("k"); time.sleep(0.3)
    check(sid, "k_moves", ids[1] in focused_label(t), focused_label(t))
    t.keys("End"); time.sleep(0.3)
    check(sid, "end_jumps_to_last", last in focused_label(t), focused_label(t), 799)
    t.keys("Home"); time.sleep(0.3)
    check(sid, "home_jumps_to_first", first in focused_label(t), focused_label(t), 799)
    for key, target in (("f", ids[5]), ("C-f", ids[7]), ("/", ids[9])):
        slow_type(t, "/") if key == "/" else t.keys(key)
        time.sleep(0.3); slow_type(t, target[:6])
        tag = {"f": "f", "C-f": "ctrl_f", "/": "slash"}[key]
        if tag == "f":
            t.capture("02-search")
        check(sid, f"search_{tag}", "⌕" in t.screen() and target in focused_label(t), focused_label(t),
              799 if tag == "ctrl_f" else None)
        t.keys("Escape"); time.sleep(0.4)
    slow_type(t, "x")
    check(sid, "stray_letter_does_not_filter", "⌕" not in t.screen(), "")
    t.close()


SCENARIOS = {
    "L1s": lambda: s_settings(True), "L1u": lambda: s_settings(False),
    "L2s": lambda: s_settings_narrow(True), "L2u": lambda: s_settings_narrow(False),
    "L3": s_model_config, "P1s": lambda: s_permission(True), "P1u": lambda: s_permission(False),
    "P2s": lambda: s_approve_all(True), "P2u": lambda: s_approve_all(False), "U1": s_popups,
    "L4s": lambda: s_sessions(True), "L4u": lambda: s_sessions(False), "L5": s_resume,
}

for key, fn in SCENARIOS.items():
    if ONLY and key not in ONLY:
        continue
    try:
        fn()
    except Exception as exc:  # record and keep going; a crash is itself a failure
        R.check(key, "scenario_completed", False, repr(exc)[:600])
T.kill_server()
print(f"{label}: {sum(r['passed'] for r in R.rows)}/{len(R.rows)} passed")
