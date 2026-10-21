# Whisper Model Files

Place the whisper model here. The STT module looks for `ggml-base.en.bin`.

## Download

### Option 1: ggml-base.en.bin (recommended, ~75 MB)
English-only, good accuracy, fast on CPU.

Download from: https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin

```powershell
cd src-tauri/resources/whisper
curl -LO https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin
```

### Option 2: ggml-small.en.bin (~488 MB)
Better accuracy, slower. Same API — just swap the filename.

```powershell
cd src-tauri/resources/whisper
curl -LO https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin
```

Then update `stt.rs` to use `ggml-small.en.bin` instead of `ggml-base.en.bin`,
or set the `NEXUS_WHISPER_MODEL` env var to the absolute path.

### Option 3: ggml-tiny.en.bin (~40 MB)
Fastest, lowest accuracy. Good for testing.

```powershell
cd src-tauri/resources/whisper
curl -LO https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en.bin
```

## Model not committed to git

The `.gitignore` excludes `.bin` files in this directory. The model is too
large for git and must be downloaded separately on each build machine.

## Environment variable override

Set `NEXUS_WHISPER_MODEL` to an absolute path to use a model file outside
the app bundle (e.g. a shared model on a multi-user system).
