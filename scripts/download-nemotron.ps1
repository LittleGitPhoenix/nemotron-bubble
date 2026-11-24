[CmdletBinding()]
param(
    [switch]$Multilingual
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$ProjectRoot = Resolve-Path (Join-Path $PSScriptRoot "..")

if ($Multilingual) {
    $ModelDir = Join-Path $ProjectRoot "models\nemotron_multi"
    $RepoFolder = "nemotron-3.5-asr-streaming-0.6b-onnx"
} else {
    $ModelDir = Join-Path $ProjectRoot "models\nemotron"
    $RepoFolder = "nemotron-speech-streaming-en-0.6b"
}

$BaseUrl = "https://huggingface.co/altunenes/parakeet-rs/resolve/main/$RepoFolder"
$Files = @(
    "encoder.onnx",
    "encoder.onnx.data",
    "decoder_joint.onnx",
    "tokenizer.model"
)

New-Item -ItemType Directory -Force -Path $ModelDir | Out-Null

foreach ($File in $Files) {
    $Target = Join-Path $ModelDir $File
    if (Test-Path $Target) {
        Write-Host "Already exists: $Target"
        continue
    }

    # Download to a .part file and only rename on success, so an interrupted
    # download is retried next run instead of being skipped as "already exists".
    $Partial = "$Target.part"
    if (Test-Path $Partial) { Remove-Item $Partial -Force }

    $Url = "$BaseUrl/$File`?download=true"
    Write-Host "Downloading $File..."
    Invoke-WebRequest -Uri $Url -OutFile $Partial -MaximumRedirection 10
    Move-Item -Path $Partial -Destination $Target -Force
}

foreach ($File in $Files) {
    $Item = Get-Item (Join-Path $ModelDir $File)
    Write-Host ("  {0,-20} {1,15:N0} bytes" -f $File, $Item.Length)
}

Write-Host "Nemotron files are ready in $ModelDir"
