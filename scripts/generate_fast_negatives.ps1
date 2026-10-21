# Generate hard multi-syllabic and fast conversational negatives
Add-Type -AssemblyName System.Speech

$words = @(
    "documentation created and synchronized",
    "documentation created",
    "synchronized",
    "synchronous",
    "synchronization",
    "documentation",
    "recognition",
    "connection",
    "transaction",
    "section",
    "action",
    "production",
    "direction",
    "collection",
    "definition",
    "resolution",
    "provision",
    "introduction",
    "instruction",
    "construction",
    "destruction",
    "function",
    "conjunction",
    "punctuation",
    "pronunciation",
    "conversation",
    "organization",
    "configuration",
    "authentication",
    "authorization",
    "registration",
    "administration",
    "demonstration",
    "investigation",
    "recommendation",
    "communication",
    "classification",
    "verification",
    "specification",
    "modification",
    "notification",
    "application",
    "operation",
    "generation",
    "integration",
    "migration",
    "iteration",
    "acceleration",
    "nexus focus",
    "next session",
    "next test",
    "next up",
    "open access",
    "excessive",
    "nixes",
    "texas tech",
    "context switch",
    "complex system",
    "reflexes",
    "suspicious activity",
    "precious moment",
    "delicious food"
)

$outDir = "wake_word_data\negative"
if (-not (Test-Path $outDir)) {
    New-Item -ItemType Directory -Path $outDir | Out-Null
}

$synth = New-Object System.Speech.Synthesis.SpeechSynthesizer

# Test available rates (-2 to +4 for normal to fast speech)
$rates = @(0, 2, 4)

$count = 0
foreach ($w in $words) {
    $cleanName = ($w -replace '\s+', '_') -replace '[^\w_]', ''
    foreach ($r in $rates) {
        $count++
        $filename = Join-Path $outDir ("synth_fast_{0}_r{1}.wav" -f $cleanName, $r)
        $synth.Rate = $r
        $synth.SetOutputToWaveFile($filename)
        $synth.Speak($w)
    }
}

$synth.Dispose()
Write-Host ("Generated {0} fast/multi-syllabic negative audio samples in {1}" -f $count, $outDir)
