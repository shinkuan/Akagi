import http from 'node:http';
import fs from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {randomBytes} from 'node:crypto';
import {CDP} from './cdp.mjs';
import {Session,findRematch,findConfirmation} from './core.mjs';
import {LogReader} from './log-reader.mjs';
const exec=promisify(execFile);
const dir=path.dirname(fileURLToPath(import.meta.url));
const appDir=process.env.AKAGI_APP_DIR??path.resolve(dir,'..');
const configPath=process.env.AKAGI_CONFIG_PATH??path.join(appDir,'configs/config.toml');
const port=Number(process.env.AKAGI_NEXT_PORT??17831),origin=`http://127.0.0.1:${port}`;
const token=process.env.AKAGI_NEXT_TOKEN??randomBytes(24).toString('hex');
const s=new Session(),reader=new LogReader(appDir,process.env.AKAGI_LOG_DIR);
let connection='正在连接 Akagi',cdp=null,targetId=null,armedTarget=null,ready=false,stopping=false;
let candidate=null,candidateAt=0,nextScan=0;
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function audit(message){await fs.appendFile(path.join(dir,'activity.log'),`${new Date().toISOString()} ${message}\n`).catch(()=>{});}
async function validateConfig(){
  const cfg=await fs.readFile(configPath,'utf8');
  const section=name=>cfg.match(new RegExp('\\['+name+'\\]([\\s\\S]*?)(?=\\n\\[|$)'))?.[1]??'';
  if(!/enabled\s*=\s*true/.test(section('autoplay'))||!/kind\s*=\s*"Majsoul"/.test(section('platform'))||!/mode\s*=\s*"chromium"/.test(section('capture')))throw Error('请先在 Akagi 开启自动打牌，并选择雀魂和 Chromium 模式');
}

async function discover(){
  // Discover only the browser Akagi launched, never another debugging session.
  const ps="[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'akagi.exe' -or ($_.Name -in @('msedge.exe','chrome.exe','brave.exe') -and $_.CommandLine -match '--remote-debugging-port=' -and $_.CommandLine -notmatch '--type=') } | Select-Object Name,ExecutablePath,CommandLine | ConvertTo-Json -Compress";
  const {stdout}=await exec('powershell.exe',['-NoProfile','-Command',ps],{windowsHide:true,timeout:10000,encoding:'utf8'});
  const data=stdout.trim()?JSON.parse(stdout):[];
  const processes=[data].flat();
  if(!processes.some(p=>p.Name==='akagi.exe'&&p.ExecutablePath?.toLowerCase()===path.join(appDir,'akagi.exe').toLowerCase()))throw Error('Akagi 已关闭，停止续局');
  const browser=processes.find(p=>p.Name!=='akagi.exe'&&p.CommandLine?.toLowerCase().includes(appDir.toLowerCase()));
  const p=browser?.CommandLine.match(/--remote-debugging-port=(\d+)/)?.[1];
  if(!p)throw Error('未找到 Akagi 受控浏览器，请先启用 Chromium 模式');
  const targets=await (await fetch(`http://127.0.0.1:${p}/json/list`,{signal:AbortSignal.timeout(3000)})).json();
  const matches=targets.filter(t=>t.type==='page'&&/^https:\/\/game\.maj-soul\.com\//.test(t.url));
  if(matches.length!==1)throw Error('请只保留一个雀魂游戏标签页');
  return matches[0];
}
async function connect(){
  const tab=await discover();
  if(!cdp||targetId!==tab.id){cdp?.close();cdp=await CDP.connect(tab.webSocketDebuggerUrl);targetId=tab.id;}
  return tab;
}
async function readScreen(){
  const shot=await cdp.send('Page.captureScreenshot',{format:'png'});
  const imagePath=path.join(dir,'current-screen.png');
  await fs.writeFile(imagePath,Buffer.from(shot.data,'base64'));
  try {
    const match=await exec('powershell.exe',['-NoProfile','-ExecutionPolicy','Bypass','-File',path.join(dir,'button-match.ps1'),'-ImagePath',imagePath],{windowsHide:true,timeout:15000,encoding:'utf8',maxBuffer:1024*1024});
    const button=JSON.parse(match.stdout.trim().replace(/^\uFEFF/,''));
    if(button.x!==undefined)return {...button,kind:'rematch'};
    if(['result','settling'].includes(s.phase)&&s.resultSteps<3){
      const result=await exec('powershell.exe',['-NoProfile','-ExecutionPolicy','Bypass','-File',path.join(dir,'result-match.ps1'),'-ImagePath',imagePath],{windowsHide:true,timeout:15000,encoding:'utf8',maxBuffer:1024*1024});
      const hit=JSON.parse(result.stdout.trim().replace(/^\uFEFF/,''));if(hit.x!==undefined)return hit;
    }
    if(s.phase==='result'){
      const result=await exec('powershell.exe',['-NoProfile','-ExecutionPolicy','Bypass','-File',path.join(dir,'result-match.ps1'),'-ImagePath',imagePath],{windowsHide:true,timeout:15000,encoding:'utf8',maxBuffer:1024*1024});
      const hit=JSON.parse(result.stdout.trim().replace(/^﻿/,''));if(hit.x!==undefined)return hit;
    }
    const {stdout}=await exec('powershell.exe',['-NoProfile','-ExecutionPolicy','Bypass','-File',path.join(dir,'ocr.ps1'),'-ImagePath',imagePath],{windowsHide:true,timeout:15000,encoding:'utf8',maxBuffer:1024*1024});
    const ocr=JSON.parse(stdout.trim().replace(/^\uFEFF/,''));
    const confirmation=findConfirmation(ocr);if(confirmation)return confirmation;
    const dialog=await exec('powershell.exe',['-NoProfile','-ExecutionPolicy','Bypass','-File',path.join(dir,'result-match.ps1'),'-ImagePath',imagePath,'-Dialog'],{windowsHide:true,timeout:15000,encoding:'utf8',maxBuffer:1024*1024});
    const verified=findConfirmation(ocr,JSON.parse(dialog.stdout.trim()));if(verified)return verified;
    const rematch=findRematch(ocr);return rematch?{...rematch,kind:'rematch'}:null;
  } finally {await fs.unlink(imagePath).catch(()=>{});}
}
async function ingest(){
  const {events,changed}=await reader.read();
  if(changed&&s.enabled)s.stop('Akagi 已重启，请重新开启续局');
  for(const e of events)s.event(e);
}
async function scan(){
  const generation=s.generation;
  await validateConfig();
  const tab=await connect();
  if(tab.id!==armedTarget)throw Error('游戏标签页已更换，请重新开启续局');
  const hit=await readScreen();
  if(!s.enabled||!['result','settling','confirming'].includes(s.phase)||s.generation!==generation)return;
  if(s.phase==='confirming'&&hit?.kind!=='confirm'){candidate=null;return;}
  if(!hit){candidate=null;connection='已连接；等待“再来一局”按钮';return;}
  const same=candidate&&candidate.kind===hit.kind&&candidate.width===hit.width&&candidate.height===hit.height&&Math.abs(candidate.x-hit.x)<12&&Math.abs(candidate.y-hit.y)<12;
  if(!same){candidate=hit;candidateAt=Date.now();return;}
  if(Date.now()-candidateAt<1500)return;
  // Logs are checked again after OCR: a manual new game can start meanwhile.
  await ingest();
  if(!s.enabled||!['result','settling','confirming'].includes(s.phase)||s.generation!==generation)return;
  const metrics=await cdp.evaluate('({width:innerWidth,height:innerHeight,dpr:devicePixelRatio})');
  const scaleX=hit.width/metrics.width,scaleY=hit.height/metrics.height;
  if(Math.abs(scaleX-scaleY)>.03)throw Error('游戏窗口尺寸变化，已停止以避免误点');
  const x=hit.x/scaleX,y=hit.y/scaleY;
  // Claim the single pending request BEFORE input. Never repeat an uncertain click.
  if(hit.kind==='confirm')s.queued();else if(hit.kind==='result-confirm'){s.settling();nextScan=Date.now()+5000;}else s.confirming();
  candidate=null;await audit(hit.kind==='result-confirm'?'终局排名与确认按钮已识别，确认一次':hit.kind==='confirm'?'续局确认框已识别，确认一次匹配':'续局按钮已识别，尝试一次点击');
  await cdp.send('Input.dispatchMouseEvent',{type:'mouseMoved',x,y});
  await sleep(180);
  if(!s.enabled||s.generation!==generation)return;
  await cdp.send('Input.dispatchMouseEvent',{type:'mousePressed',x,y,button:'left',clickCount:1});
  await sleep(100);
  await cdp.send('Input.dispatchMouseEvent',{type:'mouseReleased',x,y,button:'left',clickCount:1});
}
async function run(){
  try {await ingest();ready=true;connection='就绪，开关默认关闭';}catch{connection='等待 Akagi 日志';}
  while(!stopping){
    try{
      await ingest();ready=true;s.checkTimeout();
      if(s.enabled&&['result','settling','confirming'].includes(s.phase)&&Date.now()>=nextScan){nextScan=Date.now()+2500;await scan();}
    }catch(e){
      connection=e.message;
      if(s.enabled){s.stop('连接或识别异常，已停止：'+e.message);await audit(s.reason);}
      cdp?.close();cdp=null;
    }
    await sleep(1000);
  }
}
const server=http.createServer(async(req,res)=>{
  const send=(status,data,type='application/json')=>{res.writeHead(status,{'Content-Type':type+'; charset=utf-8','Cache-Control':'no-store','X-Content-Type-Options':'nosniff','Content-Security-Policy':"default-src 'self'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; frame-ancestors 'none'"});res.end(typeof data==='string'?data:JSON.stringify(data));};
  if(req.headers.host!==`127.0.0.1:${port}`)return send(403,{error:'Host rejected'});
  if(req.method==='GET'&&req.url==='/health')return send(200,{app:'akagi-next',version:1});
  if(req.headers['x-akagi-token']!==token)return send(403,{error:'请重新打开面板'});
  if(req.method==='GET'&&req.url==='/state')return send(200,{...s.status(),connection,ready});
  if(req.method!=='POST'||req.url!=='/control'||req.headers.origin!==origin)return send(403,{error:'Request rejected'});
  try{
    let body='';for await(const c of req){body+=c;if(body.length>2048)throw Error('请求过大');}
    const input=JSON.parse(body);
    if(input.enabled===false){s.stop();candidate=null;await audit('用户关闭续局');return send(200,s.status());}
    if(input.enabled!==true)throw Error('开关值无效');
    if(s.enabled)throw Error('请先关闭续局再修改次数');
    if(!ready)throw Error('Akagi 尚未连接');
    const generation=s.generation;
    await validateConfig();
    await connect();armedTarget=targetId;
    await ingest();
    if(s.generation!==generation)throw Error('操作已取消，请重新开启');
    if(!s.inGame&&!s.ended)throw Error('请先手动开始一场游戏，再开启自动续局');
    s.arm(input);candidate=null;connection='已连接雀魂';await audit(`开启续局；次数限制=${s.limited}，额外场数=${s.limit}`);
    send(200,s.status());
  }catch(e){send(400,{error:e.message});}
});
server.on('error',e=>{console.error(e);process.exit(1);});
server.listen(port,'127.0.0.1',()=>void run());
process.on('SIGTERM',()=>{stopping=true;s.stop();cdp?.close();server.close();});
const parentPid=Number(process.env.AKAGI_PARENT_PID);
if(parentPid>0)setInterval(()=>{
  try{process.kill(parentPid,0);}catch{s.stop();process.exit(0);}
},2000).unref();
