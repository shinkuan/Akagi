param([Parameter(Mandatory=$true)][string]$ImagePath, [switch]$Gold)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$originalPath = $ImagePath
$crop = $null
if ($Gold) {
  Add-Type -AssemblyName System.Drawing
  Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Drawing;
using System.Drawing.Imaging;
public static class GoldButtonImage {
  public static int[] Convert(string source, string target) {
    using (var input = new Bitmap(source)) {
      int left = input.Width / 2, top = (int)(input.Height * .6);
      int w = input.Width-left, h = input.Height-top;
      using (var output = new Bitmap(w,h)) {
        for(int y=0;y<h;y++) for(int x=0;x<w;x++) {
          Color p=input.GetPixel(x+left,y+top);
          bool gold=p.R>140 && p.G>95 && p.R>p.B*1.35 && p.G>p.B*1.15;
          output.SetPixel(x,y,gold?Color.Black:Color.White);
        }
        output.Save(target,ImageFormat.Png);
      }
      return new[]{input.Width,input.Height,left,top};
    }
  }
}
'@
  $ImagePath = [System.IO.Path]::Combine([System.IO.Path]::GetTempPath(), 'akagi-ocr-' + [Guid]::NewGuid().ToString('N') + '.png')
  $crop = [GoldButtonImage]::Convert($originalPath, $ImagePath)
}
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$null = [Windows.Storage.StorageFile,Windows.Storage,ContentType=WindowsRuntime]
$null = [Windows.Graphics.Imaging.BitmapDecoder,Windows.Graphics.Imaging,ContentType=WindowsRuntime]
$null = [Windows.Media.Ocr.OcrEngine,Windows.Foundation,ContentType=WindowsRuntime]
$asTask = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.IsGenericMethod })[0]
function Await($Operation, $ResultType) {
  $task = $asTask.MakeGenericMethod($ResultType).Invoke($null, @($Operation))
  $task.Wait()
  return $task.Result
}
$file = Await ([Windows.Storage.StorageFile]::GetFileFromPathAsync($ImagePath)) ([Windows.Storage.StorageFile])
$stream = Await ($file.OpenAsync([Windows.Storage.FileAccessMode]::Read)) ([Windows.Storage.Streams.IRandomAccessStream])
$decoder = Await ([Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($stream)) ([Windows.Graphics.Imaging.BitmapDecoder])
$maxDimension = [Windows.Media.Ocr.OcrEngine]::MaxImageDimension
if ([Math]::Max($decoder.PixelWidth, $decoder.PixelHeight) -gt $maxDimension) {
  $null = [Windows.Graphics.Imaging.BitmapTransform,Windows.Graphics.Imaging,ContentType=WindowsRuntime]
  $transform = New-Object Windows.Graphics.Imaging.BitmapTransform
  $ratio = $maxDimension / [double][Math]::Max($decoder.PixelWidth, $decoder.PixelHeight)
  $transform.ScaledWidth = [uint32]([Math]::Floor($decoder.PixelWidth * $ratio))
  $transform.ScaledHeight = [uint32]([Math]::Floor($decoder.PixelHeight * $ratio))
  $operation = $decoder.GetSoftwareBitmapAsync([Windows.Graphics.Imaging.BitmapPixelFormat]::Bgra8, [Windows.Graphics.Imaging.BitmapAlphaMode]::Premultiplied, $transform, [Windows.Graphics.Imaging.ExifOrientationMode]::IgnoreExifOrientation, [Windows.Graphics.Imaging.ColorManagementMode]::DoNotColorManage)
  $bitmap = Await $operation ([Windows.Graphics.Imaging.SoftwareBitmap])
} else {
  $bitmap = Await ($decoder.GetSoftwareBitmapAsync()) ([Windows.Graphics.Imaging.SoftwareBitmap])
}
$engine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromUserProfileLanguages()
if ($null -eq $engine) { throw 'Windows OCR language unavailable' }
$result = Await ($engine.RecognizeAsync($bitmap)) ([Windows.Media.Ocr.OcrResult])
$lines = @($result.Lines | ForEach-Object {
  @{ text=$_.Text; words=@($_.Words | ForEach-Object { @{text=$_.Text; x=$_.BoundingRect.X; y=$_.BoundingRect.Y; width=$_.BoundingRect.Width; height=$_.BoundingRect.Height} }) }
})
$outputWidth=$bitmap.PixelWidth
$outputHeight=$bitmap.PixelHeight
if ($Gold) {
  $factor=($crop[0]-$crop[2])/[double]$bitmap.PixelWidth
  foreach($line in $lines) {foreach($word in $line.words) {
    $word.x=$word.x*$factor+$crop[2];$word.y=$word.y*$factor+$crop[3]
    $word.width*=$factor;$word.height*=$factor
  }}
  $outputWidth=$crop[0];$outputHeight=$crop[1]
}
@{lines=$lines;width=$outputWidth;height=$outputHeight} | ConvertTo-Json -Depth 6 -Compress
$bitmap.Dispose()
$stream.Dispose()
if($Gold){Remove-Item -LiteralPath $ImagePath}
