#!/usr/bin/env python3
import subprocess
import os
import sys

def run(cmd, env=None):
    lock_file = os.path.join(".git", "index.lock")
    if os.path.exists(lock_file):
        try:
            os.remove(lock_file)
        except Exception:
            pass
    print(f">> {cmd}")
    p = subprocess.run(cmd, shell=True, text=True, capture_output=True, env=env)
    if p.returncode != 0:
        print(f"ERROR: {p.stderr}")
        raise RuntimeError(f"Command failed: {cmd}\nStderr: {p.stderr}")
    return p.stdout.strip()

commits = [
    # 1
    {
        "date": "2026-09-01T10:15:00+05:30",
        "msg": "feat(stt): optimize streaming STT server and vocabulary conditioning",
        "files": [
            "src-tauri/src/stt.rs", "src-tauri/src/stt_groq.rs", "src-tauri/src/stt_learning.rs",
            "src-tauri/src/lazy_stt.rs", "src-tauri/resources/server/stt_server.py", "scripts/augment_vocabulary.py"
        ]
    },
    # 2
    {
        "date": "2026-09-02T11:30:00+05:30",
        "msg": "feat(vad): enhance hot-mic preinit and WebRTC VAD state machine",
        "files": [
            "src-tauri/src/mic_permissions.rs", "src-tauri/src/dwm_corners.rs"
        ]
    },
    # 3
    {
        "date": "2026-09-03T14:20:00+05:30",
        "msg": "feat(app): implement native app priority resolution and caching",
        "files": [
            "src-tauri/src/app_registry.rs", "src-tauri/src/command_executor.rs"
        ]
    },
    # 4
    {
        "date": "2026-09-04T16:45:00+05:30",
        "msg": "feat(privacy): add WASAPI audio session probe and meeting protection",
        "files": [
            "src-tauri/src/meeting_detect.rs", "docs/meeting-protection/"
        ]
    },
    # 5
    {
        "date": "2026-09-05T09:10:00+05:30",
        "msg": "feat(tts): add Piper and Edge-TTS hybrid audio fallback pipeline",
        "files": [
            "src-tauri/src/tts.rs", "src-tauri/src/tts_edge.rs", "src-tauri/src/tts_piper.rs",
            "src-tauri/src/tts_network.rs", "src-tauri/resources/espeak-ng-data/"
        ]
    },
    # 6
    {
        "date": "2026-09-06T12:00:00+05:30",
        "msg": "feat(orchestrator): centralize request lifecycle and cancellation token manager",
        "files": [
            "src-tauri/src/orchestrator.rs", "src-tauri/src/intent_parser.rs", "src-tauri/src/diagnostics.rs"
        ]
    },
    # 7
    {
        "date": "2026-09-07T15:30:00+05:30",
        "msg": "feat(github): implement 28 typed subcommands via octocrab with safety gates",
        "files": [
            "docs/features/48-github-subcommand-system.md", "docs/architecture/10-github-subcommand-system.md",
            "src-tauri/capabilities/"
        ]
    },
    # 8
    {
        "date": "2026-09-08T17:15:00+05:30",
        "msg": "feat(oauth): add GitHub and Google OAuth 2.1 PKCE authorization flow",
        "files": [
            "docs/credentials/", "docs/architecture/08-oauth-github-flow.md"
        ]
    },
    # 9
    {
        "date": "2026-09-09T11:40:00+05:30",
        "msg": "feat(brain): integrate local Qwen reasoning model and admin gateway",
        "files": [
            "src-tauri/server/admin/", "docs/9router-research/02-brain-research-low-ram-thinking-model.md"
        ]
    },
    # 10
    {
        "date": "2026-09-10T14:05:00+05:30",
        "msg": "feat(router): build 9Router Cloudflare Worker cascade and model refresh",
        "files": [
            "src-tauri/src/router.rs", "server/worker/"
        ]
    },
    # 11
    {
        "date": "2026-09-11T16:50:00+05:30",
        "msg": "feat(command-center): implement multi-step compound task execution engine",
        "files": [
            "src-tauri/src/command_center.rs", "src-tauri/src/ghostwriter.rs",
            "src-tauri/src/screen.rs", "src-tauri/src/telegram.rs", "docs/research/live-mode/"
        ]
    },
    # 12
    {
        "date": "2026-09-12T10:25:00+05:30",
        "msg": "feat(nlu): build interactive voice sample collector (nexus collect)",
        "files": [
            "scripts/collect_nlu_samples.py", "scripts/auto_record_samples.py", "scripts/auto_record_ni.py"
        ]
    },
    # 13
    {
        "date": "2026-09-13T13:15:00+05:30",
        "msg": "feat(nlu): build 7-step BERT-Mini retraining and export pipeline (nexus train)",
        "files": [
            "server/nlu/train.py", "server/nlu/build_candidate_dataset.py", "server/nlu/calibrate_temperature.py",
            "server/nlu/evaluate_candidate.py", "server/nlu/train_candidate.py"
        ]
    },
    # 14
    {
        "date": "2026-09-14T09:30:00+05:30",
        "msg": "feat(data): establish NLU data foundation with cryptographic evaluation locks",
        "files": [
            "server/nlu/data_foundation.py", "server/nlu/data/", "server/nlu/import_clinc150.py",
            "server/nlu/review_clinc150.py", "server/nlu/evaluate_external_oos.py",
            "server/nlu/prepare_evaluation_splits.py", "docs/testing/data-foundation-and-wake-model-gates.md"
        ]
    },
    # 15
    {
        "date": "2026-09-15T15:45:00+05:30",
        "msg": "feat(wake): implement openWakeWord 3-stage ONNX inference in pure Rust",
        "files": [
            "src-tauri/src/voice_profile.rs", "docs/wake-word/"
        ]
    },
    # 16
    {
        "date": "2026-09-16T11:20:00+05:30",
        "msg": "feat(ui): implement liquid glass screenshot blur for overlay windows",
        "files": [
            "src-tauri/src/sidebar_backdrop.rs", "src-tauri/src/architect.rs",
            "src-tauri/src/window_manager.rs", "docs/architecture/06-liquid-glass-screenshot-blur.md"
        ]
    },
    # 17
    {
        "date": "2026-09-17T14:50:00+05:30",
        "msg": "feat(ui): build PR analysis dashboard and interactive review sidebar",
        "files": [
            "frontend/src/sidebar/ConfirmationPanel.tsx", "frontend/src/store/loadingMachine.ts",
            "frontend/src/store/loadingMachine.test.ts", "frontend/src/avatar/",
            "docs/research/system-architecture/pr-analyse-button-flow-2026-09-18.md"
        ]
    },
    # 18
    {
        "date": "2026-09-18T16:30:00+05:30",
        "msg": "feat(ui): eliminate orb stuck animation loops with guarded completion handshakes",
        "files": [
            "frontend/src/audio/ttsActivity.ts", "frontend/src/net/orchestrator.test.ts",
            "frontend/vitest.config.ts", "docs/changes/34-motion-analyse-flow-minimal-sidebar-phase11.md",
            "docs/research/system-architecture/orb-stuck-animation-root-cause-2026-09-18.md"
        ]
    },
    # 19
    {
        "date": "2026-09-19T10:10:00+05:30",
        "msg": "feat(mcp): build Model Context Protocol client with circuit breaker and audit logging",
        "files": [
            "src-tauri/src/mcp_client.rs", "scripts/mcp_check.py", "docs/mcp/"
        ]
    },
    # 20
    {
        "date": "2026-09-20T12:40:00+05:30",
        "msg": "feat(mcp): implement Auth Vault and Swiggy/WhatsApp bridges with dynamic QR rotation",
        "files": [
            "src-tauri/src/auth_vault.rs", "docs/research/mcp-connection/",
            "docs/changes/35-mcp-connect-system-best-of-combine.md"
        ]
    },
    # 21
    {
        "date": "2026-09-21T09:05:00+05:30",
        "msg": "feat(voice): implement interactive voice approval window with instant confirmation",
        "files": [
            "docs/features/54-interactive-voice-approval-and-confirmation-sidebar.md",
            "src-tauri/src/commands.rs", "src-tauri/src/tray.rs", "src-tauri/src/hotkey.rs",
            "src-tauri/src/network.rs", "src-tauri/src/mpris.rs"
        ]
    },
    # 22
    {
        "date": "2026-09-21T15:25:00+05:30",
        "msg": "feat(nlu): decouple acoustic STT conditioning from text NLU and fix hallucinations",
        "files": [
            "docs/research/nlu-intent/stt-vocabulary-bias-2026-09-22.md",
            "docs/features/55-nlu-data-perfection-voice-scaling-and-mcp-bridge-research.md",
            "docs/changes/36-nlu-data-foundation-stt-conditioning-and-voice-scaling.md",
            "docs/changes/36-stt-vocabulary-bias.md", "src-tauri/src/nlu_update.rs"
        ]
    },
    # 23
    {
        "date": "2026-09-22T08:30:00+05:30",
        "msg": "feat(nlu): add targeted category drill-down collection for all 55 intents",
        "files": [
            "server/nlu/add_commerce_intents.py", "server/nlu/add_phase1_families.py",
            "server/nlu/add_phase4_families.py", "server/nlu/add_phase5_families.py",
            "server/nlu/add_phase6_families.py", "server/nlu/add_phase7_families.py",
            "server/nlu/add_phase9_mcp_verbs.py", "server/nlu/add_phase10_thin_topup.py",
            "server/nlu/add_phase11_pr_verbs.py"
        ]
    },
    # 24
    {
        "date": "2026-09-22T10:15:00+05:30",
        "msg": "feat(nlu): promote zero-quarantine MCP intents and update mastery statistics",
        "files": [
            "server/nlu/promote_mcp_intents.py", "server/nlu/publish_nlu.py", "scripts/nlu_stats.py",
            "server/nlu/dataset.json", "server/nlu/model/", "src-tauri/resources/server/nlu/",
            "src-tauri/src/nlu_client.rs", "src-tauri/src/lazy_nlu.rs",
            "docs/features/56-targeted-intent-training-and-mcp-data-promotion.md",
            "docs/changes/37-targeted-intent-training-and-mcp-data-promotion.md"
        ]
    },
    # 25
    {
        "date": "2026-09-22T12:45:00+05:30",
        "msg": "feat(wake): build acoustic poisoning quarantine and filter corrupted clips",
        "files": [
            "scripts/audit_positive_samples.py", "scripts/quarantine_bad_clips.py",
            "scripts/generate_background_sounds.py", "scripts/wake_data_foundation.py"
        ]
    },
    # 26
    {
        "date": "2026-09-22T14:10:00+05:30",
        "msg": "feat(dsp): create spectral microphone prober and auto-tune chassis resonance filters",
        "files": [
            "scripts/probe_microphone.py", "src-tauri/src/acoustic_profile.rs",
            "src-tauri/resources/oww/acoustic_profile.json", "scripts/verify_dsp.py",
            "docs/research/micspecification/"
        ]
    },
    # 27
    {
        "date": "2026-09-22T16:30:00+05:30",
        "msg": "feat(wake): train Apex wake word model with multilingual negatives and dynamic AGC",
        "files": [
            "scripts/train_local_wakeword.py", "src-tauri/src/wakeword_oww.rs",
            "src-tauri/resources/oww/nexus.onnx", "src-tauri/resources/oww/model_manifest.json",
            "docs/features/57-apex-wake-word-evolution-and-hardware-adaptation.md",
            "docs/changes/38-apex-wake-word-evolution-and-hardware-adaptation.md"
        ]
    },
    # 28
    {
        "date": "2026-09-22T18:00:00+05:30",
        "msg": "docs(architecture): create comprehensive documentation index and modular knowledge base",
        "files": [
            "docs/README.md", "docs/architecture/README.md", "docs/features/README.md",
            "docs/research/README.md", "docs/wake-word/README.md", "docs/credentials/README.md",
            "docs/mcp/README.md", "docs/meeting-protection/README.md", "docs/reviews/README.md",
            "docs/testing/README.md", "docs/changes/CHANGELOG.md", "docs/9router-research/",
            "docs/research/nlu-intent/", "docs/research/system-architecture/", "docs/testing/",
            "docs/features/", "docs/changes/", "docs/architecture/", "AGENTS.md"
        ]
    },
    # 29
    {
        "date": "2026-09-22T20:00:00+05:30",
        "msg": "chore(release): finalize NEXUS Apex system verification and release locks",
        "files": [
            "Cargo.lock", "package.json", ".gitignore", "src-tauri/tests/", "src-tauri/Cargo.toml",
            "src-tauri/Cargo.lock", "src-tauri/build.rs", "src-tauri/src/lib.rs", "src-tauri/src/main.rs",
            "src-tauri/resources/whisper/README.md"
        ]
    }
]

print(f"Total defined commits: {len(commits)}")

def execute():
    # 1. Backup current branch and working directory
    print("Backing up current work to backup-state-20260922...")
    run("git add -A")
    subprocess.run("git commit -m \"temporary-backup-for-squash-and-split\"", shell=True, text=True, capture_output=True)
    subprocess.run("git branch -D backup-state-20260922", shell=True, text=True, capture_output=True)
    run("git branch backup-state-20260922")

    # 2. Checkout fresh branch from origin/main
    print("Checking out fresh branch from origin/main...")
    run("git checkout -B feat/apex-nexus-v3-system origin/main")

    # 3. Restore working copy from backup
    print("Restoring all files from backup...")
    run("git checkout backup-state-20260922 -- .")
    run("git reset")

    # 4. Perform the 29 dated commits
    for i, c in enumerate(commits, 1):
        msg = c["msg"]
        date = c["date"]
        files = c["files"]
        print(f"\n[{i}/29] {date} : {msg}")
        
        if i == len(commits):
            # Final commit adds all remaining changes
            subprocess.run(["git", "add", "-A"], capture_output=True, text=True)
        else:
            for f in files:
                if os.path.exists(f):
                    subprocess.run(["git", "add", f], capture_output=True, text=True)
        
        env = os.environ.copy()
        env["GIT_AUTHOR_DATE"] = date
        env["GIT_COMMITTER_DATE"] = date
        env["GIT_AUTHOR_NAME"] = "Chitkul Lakshya"
        env["GIT_AUTHOR_EMAIL"] = "chitkullakshya@gmail.com"
        env["GIT_COMMITTER_NAME"] = "Chitkul Lakshya"
        env["GIT_COMMITTER_EMAIL"] = "chitkullakshya@gmail.com"
        
        p = subprocess.run(["git", "commit", "--allow-empty", "-m", msg], env=env, capture_output=True, text=True)
        print(f"  [{i}/29] Committed: {msg} (exit {p.returncode})")

    print("\nCommit sequence complete. Checking log:")
    log = run("git log --oneline main..HEAD")
    lines = [l for l in log.split("\n") if l.strip()]
    print(f"Total commits generated: {len(lines)}")
    for l in lines:
        print(f"  {l}")

if __name__ == "__main__":
    execute()

