import os
import subprocess
import glob
import wave
import numpy as np
import scipy.io.wavfile as wavfile
from pathlib import Path
from faster_whisper import WhisperModel

ROOT = Path(__file__).resolve().parent.parent
POS_DIR = ROOT / "wake_word_data" / "positive"
POS_DIR.mkdir(parents=True, exist_ok=True)

# Generate synthetic audio using PowerShell SAPI
ps_script = """
Add-Type -AssemblyName System.Speech
$synth = New-Object System.Speech.Synthesis.SpeechSynthesizer

$voices = @()
foreach ($v in $synth.GetInstalledVoices()) {
    if ($v.Enabled) {
        $voices += $v.VoiceInfo.Name
    }
}

$phrases = @(
    "NEXUS",
    "Nexus",
    "nexus",
    "Hey NEXUS",
    "hey nexus",
    "Hey, Nexus",
    "Okay NEXUS",
    "okay nexus"
)

$rates = @(-3, -2, -1, 0, 1, 2, 3, 4)
$outDir = "$env:TEMP\\synth_nexus"
if (-not (Test-Path $outDir)) { New-Item -ItemType Directory -Path $outDir | Out-Null }

$idx = 0
foreach ($voice in $voices) {
    $synth.SelectVoice($voice)
    foreach ($phrase in $phrases) {
        foreach ($r in $rates) {
            $idx++
            $file = Join-Path $outDir ("synth_{0}.wav" -f $idx)
            $synth.Rate = $r
            $synth.SetOutputToWaveFile($file)
            $synth.Speak($phrase)
            $synth.SetOutputToNull()
        }
    }
}
$synth.Dispose()
Write-Host "Generated $idx raw synthetic files"
"""

temp_ps = ROOT / "scripts" / "_gen_sapi.ps1"
temp_ps.write_text(ps_script, encoding="utf-8")

print("Generating synthetic voices via SAPI...")
subprocess.run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(temp_ps)], check=True)
if temp_ps.exists():
    temp_ps.unlink()

temp_dir = Path(os.environ.get("TEMP", ".")) / "synth_nexus"
raw_files = sorted(temp_dir.glob("synth_*.wav"))
print(f"Found {len(raw_files)} raw synthetic files.")

# Load Whisper to verify speech content
print("Verifying synthetic files with Whisper...")
whisper = WhisperModel("tiny.en", device="cpu", compute_type="int8")

valid_count = 0
existing_files = sorted(POS_DIR.glob("synth_pos_*.wav"))
for f in existing_files:
    f.unlink()

for idx, rf in enumerate(raw_files):
    # Use standard scipy wavfile
    sr, data = wavfile.read(str(rf))
    if data.ndim > 1:
        data = data.mean(axis=1)
    
    # Simple linear resample to 16000
    if sr != 16000:
        num_samples = int(len(data) * 16000 / sr)
        indices = np.linspace(0, len(data) - 1, num_samples)
        data = np.interp(indices, np.arange(len(data)), data).astype(np.int16)
        sr = 16000

    # Ensure 2.0s duration (pad or trim)
    target_len = 32000
    if len(data) < target_len:
        # Center in 2.0s
        pad_before = (target_len - len(data)) // 3
        pad_after = target_len - len(data) - pad_before
        data = np.pad(data, (pad_before, pad_after), mode='constant')
    elif len(data) > target_len:
        data = data[:target_len]

    out_path = POS_DIR / f"synth_pos_{idx:04d}.wav"
    wavfile.write(str(out_path), 16000, data)

    # Transcribe with Whisper
    segs, _ = whisper.transcribe(str(out_path), beam_size=1)
    text = " ".join(s.text for s in segs).strip().lower()

    if "nexus" in text or "lexus" in text:
        valid_count += 1
    else:
        out_path.unlink()

# Cleanup temp
import shutil
shutil.rmtree(temp_dir, ignore_errors=True)

print(f"Successfully added {valid_count} verified high-quality synthetic 'NEXUS' & 'Hey NEXUS' samples to positive dataset.")
