$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Drawing
$fixture=Join-Path $env:TEMP ('akagi-result-test-'+[Guid]::NewGuid().ToString('N')+'.png')
try {
  foreach($withTitle in @($false,$true)) {
    $b=[System.Drawing.Bitmap]::new(2457,1001)
    $g=[System.Drawing.Graphics]::FromImage($b)
    try {
      $g.Clear([System.Drawing.Color]::Black)
      foreach($asset in @(@('result-confirm.png',1835,880),@('result-title.png',1150,45))) {
        if($asset[0] -eq 'result-title.png' -and -not $withTitle){continue}
        $t=[System.Drawing.Bitmap]::new((Join-Path $PSScriptRoot $asset[0]))
        try {$g.DrawImageUnscaled($t,$asset[1],$asset[2])} finally {$t.Dispose()}
      }
      $b.Save($fixture,[System.Drawing.Imaging.ImageFormat]::Png)
    } finally {$g.Dispose();$b.Dispose()}
    if(-not ('ResultScreen' -as [type])){. (Join-Path $PSScriptRoot 'result-match.ps1') -ImagePath $fixture | Out-Null}
    $r=[ResultScreen]::Find($fixture,$PSScriptRoot)
    if(($r[0] -ge .65) -ne $withTitle){throw "Terminal title gate failed: $withTitle score $($r[0])"}
    "PASS terminal title required=$withTitle"
  }
} finally {if(Test-Path -LiteralPath $fixture){Remove-Item -LiteralPath $fixture}}
