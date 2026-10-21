import glob
from faster_whisper import WhisperModel
from scipy.io import wavfile
import numpy as np

files = sorted(glob.glob('wake_word_data/positive/*.wav'))
print(f'Auditing {len(files)} positive files with faster-whisper...')
model = WhisperModel('tiny.en', device='cpu', compute_type='int8')

non_nexus = []
suspicious_long = []
transcripts = {}

for i, f in enumerate(files):
    sr, data = wavfile.read(f)
    if data.ndim > 1:
        data = data.mean(axis=1)
    audio = data.astype(np.float32) / 32768.0
    segments, _ = model.transcribe(audio, beam_size=1)
    text = ' '.join(s.text for s in segments).strip().lower()
    transcripts[f] = text
    
    # Check if text contains extra words or non-nexus words
    words = text.split()
    if not any(k in text for k in ['nexus', 'nexas', 'nexis', 'nexes', 'nexos']):
        non_nexus.append((f, text))
    elif len(words) > 3:
        suspicious_long.append((f, text))
        
    if (i + 1) % 50 == 0 or (i + 1) == len(files):
        print(f'Checked {i+1}/{len(files)}...')

print(f'\nTotal without pure nexus variants: {len(non_nexus)}')
for f, t in non_nexus:
    print(f'  {f}: "{t}"')

print(f'\nTotal suspiciously long phrases (>3 words): {len(suspicious_long)}')
for f, t in suspicious_long[:25]:
    print(f'  {f}: "{t}"')
