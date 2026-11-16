param([Parameter(Mandatory=$true)][string]$ImagePath, [switch]$CreateReference)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false)
Add-Type -AssemblyName System.Drawing
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Drawing;
using System.Drawing.Imaging;
public static class RematchButton {
  const int W=120,H=37;
  static bool Gold(Color p) {return p.R>140 && p.G>95 && p.R>p.B*1.35 && p.G>p.B*1.15;}
  public static void Reference(string source,string target) {
    using(var b=new Bitmap(source)) using(var c=b.Clone(new Rectangle(1590,914,240,74),PixelFormat.Format32bppArgb)) c.Save(target,ImageFormat.Png);
  }
  public static double[] Find(string source,string reference) {
    using(var b=new Bitmap(source)) using(var t=new Bitmap(reference)) {
      bool[] mask=new bool[W*H];int goldCount=0;
      for(int y=0;y<H;y++)for(int x=0;x<W;x++){bool g=Gold(t.GetPixel(x*2,y*2));mask[y*W+x]=g;if(g)goldCount++;}
      if(goldCount<70)throw new Exception("Invalid button reference");
      double cw=Math.Min(b.Width,b.Height*16.0/9), ch=cw*9/16;
      double ox=(b.Width-cw)/2,oy=(b.Height-ch)/2;
      double baseCW=1037*16.0/9,baseOX=(2457-baseCW)/2;
      double ratio=ch/1037;
      double left=ox+(1590-baseOX)*ratio,top=oy+914*ratio;
      double best=0,bx=0,by=0;
      foreach(double size in new[]{.98,1.0,1.02})for(int dy=-8;dy<=8;dy++)for(int dx=-10;dx<=10;dx++){
        double x0=left+dx*ratio,y0=top+dy*ratio,ww=240*ratio*size,hh=74*ratio*size;
        if(x0<0||y0<0||x0+ww>=b.Width||y0+hh>=b.Height)continue;
        int intersection=0,union=0,blue=0;
        for(int y=0;y<H;y++)for(int x=0;x<W;x++){
          var p=b.GetPixel((int)(x0+x*ww/W),(int)(y0+y*hh/H));
          bool g=Gold(p),a=mask[y*W+x];
          if(g&&a)intersection++;if(g||a)union++;
          if(p.B>p.R*1.3 && p.B>p.G*1.1)blue++;
        }
        double score=union==0?0:(double)intersection/union;
        if(blue<(W*H)*.55)continue;
        if(score>best){best=score;bx=x0+ww/2;by=y0+hh/2;}
      }
      return new[]{best,bx,by,(double)b.Width,(double)b.Height};
    }
  }
}
'@
$reference=Join-Path $PSScriptRoot 'rematch-button.png'
if($CreateReference){[RematchButton]::Reference($ImagePath,$reference);exit}
$r=[RematchButton]::Find($ImagePath,$reference)
if($r[0] -ge .68){@{score=$r[0];x=$r[1];y=$r[2];width=$r[3];height=$r[4];label='verified-rematch-template'}|ConvertTo-Json -Compress}
else {@{score=$r[0];match=$false}|ConvertTo-Json -Compress}
