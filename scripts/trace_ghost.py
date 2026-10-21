#!/usr/bin/env python3
"""TEMPORARY ghost-mode trace tool (delete after the voice-entry fix lands).

Shows every single execution stage of the ghost voice-entry pipeline and
pinpoints which link is missing — statically (is the code even there?)
and from a real run log (did each stage fire?).

Usage:
    nexus trace-ghost                  # static checks only
    nexus trace-ghost --log run.log    # static checks + log analysis

Log file: paste a `nexus start` console session into a file. Note the
run.ps1 console filters mic/pairing chatter — the stages below use
markers that always display (wake banners, intent lines, session lines).

Exit code: 0 = all static links present; 1 = something structural missing.
Log verdicts are advisory (printed, don't affect the code).
"""
import glob
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

GREEN = "\033[32m"
RED = "\033[31m"
DIM = "\033[2m"
RESET = "\033[0m"


def ok(msg):
    print(f"{GREEN}  [OK]{RESET} {msg}")


def missing(msg, hint=""):
    print(f"{RED}  [MISSING]{RESET} {msg}")
    if hint:
        print(f"         {DIM}->{RESET} {hint}")


def check_file_contains(path, pattern, label, hint=""):
    """True when regex `pattern` is found in repo-relative `path`."""
    full = os.path.join(ROOT, path)
    try:
        with open(full, encoding="utf-8", errors="replace") as f:
            src = f.read()
    except FileNotFoundError:
        missing(f"{label} ({path} not found)", hint)
        return False
    if re.search(pattern, src, re.DOTALL):
        ok(f"{label} ({path})")
        return True
    missing(f"{label} ({path})", hint)
    return False


def static_checks():
    print("\n== Ghost pipeline links (static: is the code there?) ==\n")
    results = []
    R = results.append

    # 1. Entry: initial ring emit (waves without mouse motion).
    R(check_file_contains(
        "src-tauri/src/ghost.rs",
        r'ghost:ring.*visible.*true',
        "1. ghost_enter emits initial ghost:ring",
        "fix: emit ghost:ring {visible:true} in ghost_enter (doc 44 issue 1)",
    ))
    # 2. Orb learns: main-window ghost:session listener flips ghostActive
    #    (session-scoped; ghost:ring is positional-only since the
    #    mouse-takeover re-scope).
    R(check_file_contains(
        "frontend/src/App.tsx",
        r'listen.*ghost:session.*setGhostActive',
        "2. orb listens ghost:session -> ghostActive",
        "fix: ghost:session listener in App.tsx init (mouse-takeover re-scope)",
    ))
    # 3. Store field exists.
    R(check_file_contains(
        "frontend/src/store/assistant.ts",
        r'ghostActive:\s*boolean',
        "3. store has ghostActive flag",
        "fix: add ghostActive + setter to assistant.ts",
    ))
    # 4. Hot-mic module + its three hooks.
    R(check_file_contains(
        "frontend/src/net/ghostHotMic.ts",
        r'maybeGhostRelisten',
        "4a. ghostHotMic module present",
        "fix: add net/ghostHotMic.ts (doc 44 issue 2)",
    ))
    R(check_file_contains(
        "frontend/src/net/orchestrator.ts",
        r'ghostHotMic.*maybeGhostRelisten',
        "4b. turn-end hooks (finishSpokenResult + done)",
        "fix: call maybeGhostRelisten() after both resets",
    ))
    R(check_file_contains(
        "frontend/src/audio/recorder.ts",
        r'ghostHotMic.*recordSilentMiss|recordSilentMiss.*ghostHotMic',
        "4c. silent-miss anti-nag in capture path",
        "fix: ghost-aware empty-transcript branch in processTranscript",
    ))
    # 5. THE capture link: relisten must start Rust capture.
    R(check_file_contains(
        "frontend/src/main.tsx",
        r'start_stt_capture',
        "5. relisten starts Rust capture (start_stt_capture)",
        "fix: triggerFollowupListen must invoke start_stt_capture — "
        "without this the orb shows listening while Rust captures "
        "nothing (doc 44 follow-up finding)",
    ))
    # 6. Waves visual.
    R(check_file_contains(
        "frontend/src/avatar/Avatar.tsx",
        r'ghostWaveBars|ghost-waves',
        "6a. orb waves visual + pinch transition",
        "fix: Avatar ghost phase machine (doc 43)",
    ))
    R(check_file_contains(
        "frontend/src/styles.css",
        r'\.ghost-waves|\.ghost-bar',
        "6b. waves CSS present",
        "fix: ghost-waves/ghost-bar rules in styles.css",
    ))
    # 7. Scrollbar fix (stage.html CSS).
    R(check_file_contains(
        "frontend/stage.html",
        r'overflow\s*:\s*hidden',
        "7. stage.html kills scrollbars",
        "fix: margin:0 + overflow:hidden + transparent bg in stage.html",
    ))
    # 8. Heartbeat client marker.
    R(check_file_contains(
        "frontend/src/stage/main.tsx",
        r'stage-shell-v1',
        "8. stage heartbeat carries build marker",
        "fix: client marker in StageApp heartbeat (doc 44 issue 3)",
    ))
    # 9. Bundled UI is current (the binary ships dist, not src).
    dist_hits = 0
    for js in glob.glob(os.path.join(ROOT, "frontend", "dist", "assets", "*.js")):
        try:
            with open(js, encoding="utf-8", errors="replace") as f:
                chunk = f.read()
        except OSError:
            continue
        if "ghostActive" in chunk:
            dist_hits += 1
    if dist_hits:
        ok(f"9. built dist bundle contains ghost UI ({dist_hits} asset(s))")
        R(True)
    else:
        missing("9. built dist bundle contains ghost UI",
                "run a frontend build — src fixes never reach the binary otherwise")
        R(False)

    passed = sum(1 for r in results if r)
    print(f"\n{DIM}static: {passed}/{len(results)} links present{RESET}")
    return passed == len(results)


# --- Log analysis --------------------------------------------------------

STAGES = [
    ("wake", re.compile(r"wake-word: NEXUS detected|OWW wake confirmed|WAKE WORD HEARD", re.I),
     "wake word fired"),
    ("transcript", re.compile(r"stt-capture:\s*transcript\s*=\s*'([^']*)'"),
     "command transcribed"),
    ("intent", re.compile(r"deterministic:\s*(\w+)"),
     "intent parsed"),
    ("session", re.compile(r"ghost:\s*session ACTIVE"),
     "ghost session entered"),
    ("stage", re.compile(r"creating 'stage' window|stage:\s*shown"),
     "stage window shown"),
    ("heartbeat", re.compile(r"stage:\s*frontend alive\s*\(client=([^)]*)\)"),
     "stage UI alive + build marker"),
    ("tts_done", re.compile(r"audio playback completed"),
     "entry speech finished"),
    ("relisten", re.compile(r"stt-capture:\s*started"),
     "mic reopened after turn (hot-mic)"),
    ("yield", re.compile(r"session yielded|stand_down|ghost_exit|session ended", re.I),
     "session ended (yield/exit)"),
]

FIX_FOR_MISSING = {
    "heartbeat": "stage frontend old or dead — rebuild; check client marker value",
    "relisten": "triggerFollowupListen never starts Rust capture — "
                "invoke start_stt_capture there (the #1 live find)",
    "yield": "no yield seen — session may still be open (or log truncated)",
}


def analyze_log(path):
    print(f"\n== Log analysis: {path} ==\n")
    try:
        with open(path, encoding="utf-8", errors="replace") as f:
            lines = f.read().splitlines()
    except OSError as e:
        print(f"  cannot read log: {e}")
        return

    def ts_of(line):
        m = re.match(r"\[(\d+):(\d+):(\d+)\]", line)
        if not m:
            return None
        h, mi, s = (int(x) for x in m.groups())
        return h * 3600 + mi * 60 + s

    # (stage, timestamp|None, detail)
    found = {}
    for name, rx, _label in STAGES:
        for ln in lines:
            m = rx.search(ln)
            if m:
                detail = m.group(1).strip() if m.lastindex else ""
                found.setdefault(name, []).append((ts_of(ln), detail))
    # Relisten only counts AFTER speech finished: the entry capture itself
    # matches the same marker, so order by timestamp (hot-mic = capture
    # started later than the last TTS completion).
    if "relisten" in found and "tts_done" in found:
        tts_times = [t for t, _ in found["tts_done"] if t is not None]
        cap_times = [t for t, _ in found["relisten"] if t is not None]
        if tts_times and cap_times:
            last_tts = max(tts_times)
            post = [t for t in cap_times if t > last_tts]
            if not post:
                del found["relisten"]
    for name, _rx, label in STAGES:
        hits = found.get(name, [])
        if hits:
            ts, detail = hits[0]
            extra = f" — {detail}" if detail else ""
            if name == "relisten" and len(hits) > 1:
                # relisten must come AFTER speech; count post-TTS ones
                ok(f"{label} x{len(hits)}{extra}")
            else:
                ok(f"{label}{extra}")
        else:
            missing(label, FIX_FOR_MISSING.get(name, ""))
    # Verdicts across stages.
    print()
    if "session" in found and "relisten" not in found:
        print(f"{RED}  VERDICT:{RESET} session opened but mic never reopened — "
              "the capture gap. Speak produces silence; a later mouse move "
              "looks like a takeover but is really starvation.")
    if "heartbeat" in found and "relisten" in found and "yield" not in found:
        print(f"{GREEN}  VERDICT:{RESET} full loop alive: entry -> UI -> relisten, "
              "session still open.")
    if "session" not in found and "transcript" in found:
        print(f"{RED}  VERDICT:{RESET} heard but never entered — check intent "
              "line (want EnterGhostControl, not EnterGhostwriter).")
    if "transcript" not in found:
        print(f"{RED}  VERDICT:{RESET} no transcript at all — mic/capture issue "
              "upstream of ghost entirely.")


def main():
    args = sys.argv[1:]
    log_path = None
    if "--log" in args:
        i = args.index("--log")
        if i + 1 < len(args):
            log_path = args[i + 1]
    print("Ghost voice-entry trace (TEMPORARY diagnostic — see header).")
    good = static_checks()
    if log_path:
        analyze_log(log_path)
    else:
        print("\n(hint: add --log <run.log> to check a real session too)")
    sys.exit(0 if good else 1)


if __name__ == "__main__":
    main()
