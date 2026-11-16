param([Parameter(Mandatory=$true)][string]$ImagePath, [switch]$Dialog)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false)
Add-Type -AssemblyName System.Drawing
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Drawing;
public static class ResultScreen {
  static bool Ink(Color p,bool title) {
    return title ? p.R>195 && p.G>195 && p.B>195 : p.R<170 && p.B>p.G*1.1 && p.R>p.G*1.2;
  }
  static double[] Match(Bitmap b,string file,double left,double top,bool title) {
    using(var t=new Bitmap(file)) {
      int w=t.Width/2,h=t.Height/2,count=0; bool[] mask=new bool[w*h];
      for(int y=0;y<h;y++)for(int x=0;x<w;x++){bool v=Ink(t.GetPixel(x*2,y*2),title);mask[y*w+x]=v;if(v)count++;}
      if(count<30)throw new Exception("Invalid result reference");
      double cw=Math.Min(b.Width,b.Height*16.0/9),ch=cw*9/16,ratio=ch/1001;
      double ox=(b.Width-cw)/2,oy=(b.Height-ch)/2,baseOx=(2457-1001*16.0/9)/2;
      double best=0,bx=0,by=0;
      foreach(double size in new[]{.98,1.0,1.02})for(int dy=-6;dy<=6;dy++)for(int dx=-6;dx<=6;dx++) {
        double x0=ox+(left-baseOx+dx)*ratio,y0=oy+(top+dy)*ratio,ww=t.Width*ratio*size,hh=t.Height*ratio*size;
        if(x0<0||y0<0||x0+ww>=b.Width||y0+hh>=b.Height)continue;
        int intersection=0,union=0,yellow=0;
        for(int y=0;y<h;y++)for(int x=0;x<w;x++){
          var p=b.GetPixel((int)(x0+x*ww/w),(int)(y0+y*hh/h));bool v=Ink(p,title),a=mask[y*w+x];
          if(v&&a)intersection++;if(v||a)union++;
          if(p.R>190&&p.G>140&&p.B<p.G*.8)yellow++;
        }
        if(!title&&yellow<w*h*.55)continue;
        double score=union==0?0:(double)intersection/union;
        if(score>best){best=score;bx=x0+ww/2;by=y0+hh/2;}
      }
      return new[]{best,bx,by};
    }
  }
  public static double[] FindDialog(string source,string dir) {
    using(var b=new Bitmap(source)){
      var button=Match(b,System.IO.Path.Combine(dir,"dialog-confirm.png"),920,698,false);
      return new[]{button[0],button[1],button[2],(double)b.Width,(double)b.Height};
    }
  }
  public static double[] Find(string source,string dir) {
    using(var b=new Bitmap(source)){
      var title=Match(b,System.IO.Path.Combine(dir,"result-title.png"),1150,45,true);
      if(title[0]<.65)return new[]{title[0],0.0,0.0,(double)b.Width,(double)b.Height};
      var button=Match(b,System.IO.Path.Combine(dir,"result-confirm.png"),1835,880,false);
      return new[]{Math.Min(title[0],button[0]),button[1],button[2],(double)b.Width,(double)b.Height};
    }
  }
}
'@
$r=if($Dialog){[ResultScreen]::FindDialog($ImagePath,$PSScriptRoot)}else{[ResultScreen]::Find($ImagePath,$PSScriptRoot)}
if($r[0] -ge .65){@{score=$r[0];x=$r[1];y=$r[2];width=$r[3];height=$r[4];kind='result-confirm';label='verified-terminal-result'}|ConvertTo-Json -Compress}
else {@{score=$r[0];match=$false}|ConvertTo-Json -Compress}
