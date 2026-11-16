$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Drawing
$fixture=Join-Path ([System.IO.Path]::GetTempPath()) ('akagi-button-test-'+[Guid]::NewGuid().ToString('N')+'.png')
$assetBitmap=[System.Drawing.Bitmap]::new((Join-Path $PSScriptRoot 'rematch-button.png'))
try {
  foreach($size in @(@(1469,1037),@(2477,1314),@(1280,720))) {
    $w=$size[0];$h=$size[1]
    $b=[System.Drawing.Bitmap]::new($w,$h)
    $g=[System.Drawing.Graphics]::FromImage($b)
    $g.Clear([System.Drawing.Color]::Black)
    $cw=[Math]::Min($w,$h*16.0/9);$ch=$cw*9/16;$ratio=$ch/1037
    $x=($w-$cw)/2+(1590-(2457-1037*16.0/9)/2)*$ratio
    $y=($h-$ch)/2+914*$ratio
    $rect=[System.Drawing.RectangleF]::new([single]$x,[single]$y,[single](240*$ratio),[single](74*$ratio))
    $g.DrawImage($assetBitmap,$rect)
    $b.Save($fixture,[System.Drawing.Imaging.ImageFormat]::Png);$g.Dispose();$b.Dispose()
    if(-not ('RematchButton' -as [type])) { . (Join-Path $PSScriptRoot 'button-match.ps1') -ImagePath $fixture | Out-Null }
    $r=[RematchButton]::Find($fixture,(Join-Path $PSScriptRoot 'rematch-button.png'))
    if($r[0] -lt .68){throw "Scale test failed ${w}x${h}: $($r[0])"}
    "PASS ${w}x${h}: $($r[0])"
  }
  $b=[System.Drawing.Bitmap]::new(1280,720);$b.Save($fixture,[System.Drawing.Imaging.ImageFormat]::Png);$b.Dispose()
  $r=[RematchButton]::Find($fixture,(Join-Path $PSScriptRoot 'rematch-button.png'))
  if($r[0] -ge .68){throw 'Blank image matched'}
  'PASS blank image rejected'
} finally {$assetBitmap.Dispose();if(Test-Path -LiteralPath $fixture){Remove-Item -LiteralPath $fixture}}
