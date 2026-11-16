param([Parameter(Mandatory=$true)][string]$Destination, [Parameter(Mandatory=$true)][string]$NodeExe)
$ErrorActionPreference='Stop'
$repo=Split-Path $PSScriptRoot -Parent
$source=Join-Path $repo 'next-game'
$target=Join-Path ([System.IO.Path]::GetFullPath($Destination)) 'next-game'
if(-not (Test-Path -LiteralPath $NodeExe)){throw 'Node.js runtime not found'}
New-Item -ItemType Directory -Path (Join-Path $target 'runtime') -Force | Out-Null
$files=@('server.mjs','core.mjs','log-reader.mjs','cdp.mjs','ocr.ps1','button-match.ps1','rematch-button.png','result-match.ps1','result-title.png','result-confirm.png','dialog-confirm.png','README.md')
foreach($file in $files){Copy-Item -LiteralPath (Join-Path $source $file) -Destination (Join-Path $target $file) -Force}
Copy-Item -LiteralPath $NodeExe -Destination (Join-Path $target 'runtime\node.exe') -Force
"Packaged continuation worker: $target"
